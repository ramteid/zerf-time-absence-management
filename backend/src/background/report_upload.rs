//! Per-employee monthly timesheet PDF upload to Nextcloud.
//!
//! Flow:
//!   1. Each midnight tick: if today.day() >= configured upload day, populate
//!      the export queue for the previous month (idempotent, guarded by the
//!      `report_upload_queue_period` app_setting).
//!   2. Process eligible pending queue entries: for each (user, period), check
//!      the shared month-finality gate (`services::reports::month_export_readiness`,
//!      with full approval required — the PDF Total row only counts approved,
//!      crediting minutes). If ready, build a per-user PDF, create the
//!      per-month subfolder, upload the file, and remove the queue entry.
//!      Entries for a month that is not submitted or not yet fully approved
//!      are left in the queue for the next daily check (catch-up for late
//!      submitters and pending approvals). Before the configured upload day,
//!      the scheduled run still catches up older months but defers the
//!      just-finished previous month.
//!      An entry whose user's current state would hide historical rows is left
//!      in the queue and surfaced to admins so a start-date or workflow mistake
//!      can be corrected without losing the pending export.
//!
//! Folder layout in the Nextcloud share:
//!   <period>/                                       e.g. 2026-05/
//!     <period>_Stundenzettel_<First>_<Last>.pdf     e.g. 2026-05_Stundenzettel_John_Smith.pdf
//!
//! The handler `run_now` (triggered by the admin "Upload now" button) bypasses
//! the day-of-month threshold: it populates the queue for the previous month
//! (idempotent) and processes all pending entries immediately.

use crate::background::schedule;
use crate::error::{AppError, AppResult};
use crate::services::{
    nextcloud,
    reports::{
        build_timesheet_section, month_export_readiness, MonthExportReadiness, PendingAbsences,
    },
    settings,
    users::repo_user_to_auth_user,
};
use crate::AppState;
use chrono::NaiveDate;

/// Background loop: checks once per day (midnight in app timezone).
pub async fn run_loop(state: AppState) {
    schedule::run_daily_after_midnight(state, "Report upload", |state| async move {
        run_once(&state).await
    })
    .await;
}

/// Triggered by the admin "Upload now" button.
/// Populates the queue for the previous month (idempotent) and processes all
/// pending entries, skipping the day-of-month threshold check.
pub async fn run_now(state: &AppState) -> AppResult<()> {
    let (enabled, url, _day, password) = load_upload_settings(state).await?;
    if !enabled {
        return Err(AppError::BadRequest(
            "Report PDF upload is not enabled.".into(),
        ));
    }
    if url.is_empty() {
        return Err(AppError::BadRequest(
            "No Nextcloud share URL configured for report upload.".into(),
        ));
    }

    let today = settings::app_today(&state.pool).await;
    populate_queue_for_prev_month(state, today).await?;

    let (base, token) = nextcloud::parse_share_url(&url)?;
    let pw = if password.is_empty() {
        None
    } else {
        Some(password.as_str())
    };
    process_pending_entries(state, &base, &token, pw, None).await;

    Ok(())
}

/// Daily scheduled run: always backfill queue, then process pending entries with deferral for previous month.
async fn run_once(state: &AppState) -> AppResult<()> {
    let (enabled, url, day_of_month, password) = load_upload_settings(state).await?;
    if !enabled || url.is_empty() {
        return Ok(());
    }

    let today = settings::app_today(&state.pool).await;
    let process_through_period = schedule::process_through_period(today, day_of_month)?;
    // Always populate queue for missed months (backfill), even before configured day.
    // The just-finished previous month will be deferred by process_pending_entries when before day.
    populate_queue_for_prev_month(state, today).await?;

    let (base, token) = nextcloud::parse_share_url(&url)?;
    let pw = if password.is_empty() {
        None
    } else {
        Some(password.as_str())
    };
    process_pending_entries(state, &base, &token, pw, process_through_period.as_deref()).await;

    Ok(())
}

/// Populate the export queue for all months from the period after the last
/// queued period through the previous month, inclusive. Guards against
/// re-population via the `report_upload_queue_period` setting.
async fn populate_queue_for_prev_month(state: &AppState, today: NaiveDate) -> AppResult<()> {
    schedule::queue_periods_through_previous_month(
        state,
        settings::REPORT_UPLOAD_QUEUE_PERIOD_KEY,
        today,
        |period| async move {
            let (from, to) = schedule::period_bounds(&period)?;

            // Include deactivated users who had entries/absences in the period so
            // the archive export is complete (see ReportDb::timesheet_members_for_period).
            let members = state
                .db
                .reports
                .timesheet_members_for_period(from, to)
                .await?;
            let ids: Vec<i64> = members.iter().map(|u| u.id).collect();

            state.db.export_queue.populate(&period, &ids).await?;
            tracing::info!("Report upload: queued {} export(s) for {period}", ids.len());
            Ok(())
        },
    )
    .await
}

/// Try to upload a PDF for each pending queue entry; leave unready entries in place.
async fn process_pending_entries(
    state: &AppState,
    base: &str,
    token: &str,
    pw: Option<&str>,
    process_through_period: Option<&str>,
) {
    let entries = match state.db.export_queue.list_pending().await {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Report upload: failed to list queue: {e}");
            return;
        }
    };
    if entries.is_empty() {
        return;
    }

    let language = match crate::i18n::load_ui_language(&state.pool).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Report upload: failed to load UI language: {e}");
            return;
        }
    };

    for entry in entries {
        if schedule::period_is_deferred(&entry.period, process_through_period) {
            tracing::debug!(
                "Report upload: deferring user {} period {} until configured upload day",
                entry.user_id,
                entry.period
            );
            continue;
        }
        if let Err(e) = process_one_entry(state, &entry, base, token, pw, &language).await {
            tracing::warn!(
                "Report upload: skipping user {} period {}: {e}",
                entry.user_id,
                entry.period
            );
        }
    }
}

/// Process one queue entry: verify submission, build PDF, upload, delete entry.
async fn process_one_entry(
    state: &AppState,
    entry: &crate::repository::ExportQueueEntry,
    base: &str,
    token: &str,
    pw: Option<&str>,
    language: &crate::i18n::Language,
) -> AppResult<()> {
    // If the user was deleted, clean up the orphaned queue entry and move on.
    let user = match state.db.users.find_by_id(entry.user_id).await? {
        Some(u) => u,
        None => {
            state
                .db
                .export_queue
                .delete_entry(entry.user_id, &entry.period)
                .await?;
            return Ok(());
        }
    };

    let (from, to) = schedule::period_bounds(&entry.period)?;

    // A start-date change that still needs admin review takes priority over
    // the shared gate's plain pre-start-content check below: it has its own,
    // more specific message ("the change still requires review") even when no
    // stored row actually falls before the new start date yet.
    if entry.requires_start_date_review && user.start_date > from {
        let params: Vec<(&str, String)> = vec![
            ("user_id", user.id.to_string()),
            ("first_name", user.first_name.clone()),
            ("last_name", user.last_name.clone()),
            (
                "start_date",
                crate::i18n::format_date(language, user.start_date),
            ),
            ("period", entry.period.clone()),
        ];
        let text = crate::i18n::notification_text(
            language,
            "report_upload_blocked_title",
            "report_upload_pre_start_review_body",
            &params,
        );
        tracing::warn!(target: "zerf::report_upload", "{}", text.body);
        crate::services::notifications::enqueue_error(
            state,
            language,
            &format!("report_upload_pre_start_{}_{}", user.id, entry.period),
            &text.title,
            &text.body,
        )
        .await;
        return Ok(());
    }

    // Shared month-finality gate (see `services::reports::month_export_readiness`):
    // historical-only accounts only need settled time-entry rows, everyone else
    // needs decided absences, fully submitted weeks, and full approval — the
    // PDF's Total row counts only approved, crediting minutes, so a merely
    // submitted month would archive too few hours.
    // The archive is this person's own month, so a week nobody handed in
    // leaves a hole in the document and has to hold it back.
    match month_export_readiness(
        &state.pool,
        &user,
        from,
        to,
        true,
        true,
        // The archive prints every absence, so any undecided request would
        // change it.
        PendingAbsences::Any,
    )
    .await?
    {
        MonthExportReadiness::Ready => {}
        MonthExportReadiness::PreStartContent => {
            let params: Vec<(&str, String)> = vec![
                ("user_id", user.id.to_string()),
                ("first_name", user.first_name.clone()),
                ("last_name", user.last_name.clone()),
                (
                    "start_date",
                    crate::i18n::format_date(language, user.start_date),
                ),
                ("period", entry.period.clone()),
            ];
            let text = crate::i18n::notification_text(
                language,
                "report_upload_blocked_title",
                "report_upload_pre_start_content_body",
                &params,
            );
            tracing::warn!(target: "zerf::report_upload", "{}", text.body);
            crate::services::notifications::enqueue_error(
                state,
                language,
                &format!("report_upload_pre_start_{}_{}", user.id, entry.period),
                &text.title,
                &text.body,
            )
            .await;
            return Ok(());
        }
        MonthExportReadiness::UnresolvedTimeEntries => {
            let params: Vec<(&str, String)> = vec![
                ("user_id", user.id.to_string()),
                ("first_name", user.first_name.clone()),
                ("last_name", user.last_name.clone()),
                ("period", entry.period.clone()),
            ];
            let text = crate::i18n::notification_text(
                language,
                "report_upload_blocked_title",
                "report_upload_unsettled_time_body",
                &params,
            );
            tracing::warn!(target: "zerf::report_upload", "{}", text.body);
            crate::services::notifications::enqueue_error(
                state,
                language,
                &format!("report_upload_unsettled_time_{}_{}", user.id, entry.period),
                &text.title,
                &text.body,
            )
            .await;
            return Ok(());
        }
        MonthExportReadiness::PendingAbsenceRequests => {
            tracing::warn!(
                target: "zerf::report_upload",
                "User {} ({} {}) has pending absence requests in period {}. \
                 Decide those requests before retrying the timesheet PDF export.",
                user.id, user.first_name, user.last_name, entry.period
            );
            return Ok(());
        }
        // Not ready yet, but a routine, expected wait (nobody has to be
        // alerted): leave in the queue for the next daily check.
        MonthExportReadiness::WeeksNotSubmitted | MonthExportReadiness::UnapprovedTimeEntries => {
            return Ok(())
        }
    }

    // Build a single-user timesheet PDF.
    let auth_user = repo_user_to_auth_user(user.clone());
    let label = entry.period.clone();
    let section = build_timesheet_section(&state.pool, &auth_user, from, to, &label).await?;
    let bytes = crate::report_pdf::render_timesheet_pdf(&[section], from, to, language);
    if bytes.is_empty() {
        return Err(AppError::Internal(format!(
            "Generated PDF is empty for user {} period {}",
            user.id, entry.period
        )));
    }

    // Build path: <period>/<period>_Stundenzettel_<First>_<Last>_<Id>.pdf
    // Sanitize names to prevent path traversal and include user_id to avoid
    // collisions (e.g. "John O'Neil" vs "John O-Neil" both become John_O_Neil).
    fn sanitize_name(s: &str) -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .chars()
            .fold(String::new(), |mut acc, c| {
                if !(acc.ends_with('_') && c == '_') {
                    acc.push(c);
                }
                acc
            })
            .trim_matches('_')
            .to_string()
    }
    let first_raw = sanitize_name(&user.first_name);
    let last_raw = sanitize_name(&user.last_name);
    let first = if first_raw.is_empty() { "user".to_string() } else { first_raw };
    let last = if last_raw.is_empty() { "unknown".to_string() } else { last_raw };
    let folder = entry.period.clone();
    let filename = format!(
        "{}_Stundenzettel_{}_{}_{}.pdf",
        entry.period, first, last, user.id
    );
    let path = format!("{folder}/{filename}");

    // Create the per-month subfolder (MKCOL; 405 = already exists is fine for
    // write-only shares that disallow PROPFIND).
    nextcloud::create_folder(base, token, pw, &folder).await?;
    nextcloud::upload_file(base, token, pw, &path, bytes).await?;

    // Only remove from queue after a confirmed successful upload.
    state
        .db
        .export_queue
        .delete_entry(entry.user_id, &entry.period)
        .await?;

    tracing::info!(
        "Report upload: uploaded {} for user {} ({})",
        path,
        user.id,
        entry.period
    );
    Ok(())
}

async fn load_upload_settings(state: &AppState) -> AppResult<(bool, String, u8, String)> {
    let enabled = settings::load_setting(&state.pool, settings::REPORT_UPLOAD_ENABLED_KEY, "false")
        .await?
        == "true";
    let url = settings::load_setting(&state.pool, settings::REPORT_UPLOAD_URL_KEY, "").await?;
    let day_of_month: u8 = settings::load_setting(
        &state.pool,
        settings::REPORT_UPLOAD_DAY_OF_MONTH_KEY,
        "5",
    )
    .await?
    .parse()
    .unwrap_or(5)
    .clamp(1, 28);
    let password =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_PASSWORD_KEY, "").await?;
    Ok((enabled, url, day_of_month, password))
}
