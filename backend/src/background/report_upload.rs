//! Per-employee monthly timesheet PDF upload to Nextcloud.
//!
//! Flow:
//!   1. Each midnight tick: if today.day() >= configured upload day, populate
//!      the export queue for the previous month (idempotent, guarded by the
//!      `report_upload_queue_period` app_setting).
//!   2. Process eligible pending queue entries: for each (user, period), check
//!      whether all weeks of the period are fully submitted.  If yes, build
//!      a per-user PDF, create the per-month subfolder, upload the file, and
//!      remove the queue entry.  Entries for not-yet-submitted months are left
//!      in the queue for the next daily check (catch-up for late submitters).
//!      Before the configured upload day, the scheduled run still catches up
//!      older months but defers the just-finished previous month.
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

use crate::error::{AppError, AppResult};
use crate::services::{
    nextcloud,
    reports::{all_weeks_ready_for_timesheet_export, build_timesheet_section},
    settings,
    users::repo_user_to_auth_user,
};
use crate::time_calc::last_day_of_month;
use crate::AppState;
use chrono::{Datelike, NaiveDate};

/// Background loop: checks once per day (midnight in app timezone).
pub async fn run_loop(state: AppState) {
    loop {
        let tz = settings::load_app_timezone(&state.pool).await;
        let now_utc = chrono::Utc::now();
        let now_local = now_utc.with_timezone(&tz);
        let wait = now_local
            .date_naive()
            .succ_opt()
            .and_then(|d| d.and_hms_opt(0, 0, 30))
            .and_then(|dt| dt.and_local_timezone(tz).single())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .and_then(|midnight_utc| (midnight_utc - now_utc).to_std().ok())
            .unwrap_or(std::time::Duration::from_secs(3600));

        tokio::time::sleep(wait).await;

        if let Err(e) = run_once(&state).await {
            tracing::error!("Report upload: {e:?}");
        }
    }
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

/// Daily scheduled run: populate queue if Stichtag is reached, then process pending entries.
async fn run_once(state: &AppState) -> AppResult<()> {
    let (enabled, url, day_of_month, password) = load_upload_settings(state).await?;
    if !enabled || url.is_empty() {
        return Ok(());
    }

    let today = settings::app_today(&state.pool).await;
    let target_period = prev_period(today);
    let upload_day_reached = today.day() >= u32::from(day_of_month);
    if upload_day_reached {
        populate_queue_for_prev_month(state, today).await?;
    }

    let (base, token) = nextcloud::parse_share_url(&url)?;
    let pw = if password.is_empty() {
        None
    } else {
        Some(password.as_str())
    };
    let process_through_period = if upload_day_reached {
        None
    } else {
        Some(period_before(&target_period)?)
    };
    process_pending_entries(state, &base, &token, pw, process_through_period.as_deref()).await;

    Ok(())
}

/// Returns the list of YYYY-MM periods that need to be queued, starting from
/// the month after `last_queued` through `target` (inclusive).
///
/// Behaviour by case:
///
/// - Empty `last_queued` or a parse failure: returns `[target]` (first-ever run).
/// - `last_queued >= target` (valid YYYY-MM): returns `[]` (already up to date
///   or the stored value is unexpectedly in the future).
/// - Otherwise: every month from `last_queued + 1` through `target`.
fn periods_to_backfill(last_queued: &str, target: &str) -> Vec<String> {
    if last_queued.is_empty() {
        return vec![target.to_string()];
    }
    match parse_year_month(last_queued) {
        Err(_) => vec![target.to_string()], // Corrupt setting; just do the current period.
        Ok((mut y, mut m)) => {
            // If last_queued is already at or past the target there is nothing to queue.
            // We rely on the parsed (y, m) values rather than string comparison to avoid
            // lexicographic ordering surprises with malformed data.
            let (ty, tm) = match parse_year_month(target) {
                Ok(v) => v,
                Err(_) => return vec![],
            };
            if (y, m) >= (ty, tm) {
                return vec![];
            }
            let mut periods = Vec::new();
            loop {
                // Advance one month.
                if m == 12 {
                    y += 1;
                    m = 1;
                } else {
                    m += 1;
                }
                let p = format!("{y:04}-{m:02}");
                periods.push(p.clone());
                if (y, m) == (ty, tm) {
                    break;
                }
                // Safety: stop if we somehow overshoot.
                if (y, m) > (ty, tm) {
                    // Pop the overshot period we just pushed.
                    periods.pop();
                    break;
                }
            }
            periods
        }
    }
}

/// Populate the export queue for all months from the period after the last
/// queued period through `prev_period(today)`, inclusive. Guards against
/// re-population via the `report_upload_queue_period` setting.
///
/// If the upload feature was disabled for several months and is re-enabled,
/// or if the server missed a month boundary, this call backfills every
/// intervening period so no timesheet is silently skipped.
async fn populate_queue_for_prev_month(state: &AppState, today: NaiveDate) -> AppResult<()> {
    let target = prev_period(today);
    let last_queued =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_QUEUE_PERIOD_KEY, "").await?;

    let periods_to_queue = periods_to_backfill(&last_queued, &target);
    if periods_to_queue.is_empty() {
        return Ok(());
    }

    for period in &periods_to_queue {
        let (year, month) = parse_year_month(period)?;
        let from = NaiveDate::from_ymd_opt(year, month, 1)
            .ok_or_else(|| AppError::Internal(format!("invalid period {period}")))?;
        let last_day = last_day_of_month(year, month);
        let to = NaiveDate::from_ymd_opt(year, month, last_day)
            .ok_or_else(|| AppError::Internal(format!("invalid period end {period}")))?;

        // Include deactivated users who had entries/absences in the period so
        // the archive export is complete (see ReportDb::timesheet_members_for_period).
        let members = state
            .db
            .reports
            .timesheet_members_for_period(from, to)
            .await?;
        let ids: Vec<i64> = members.iter().map(|u| u.id).collect();

        state.db.export_queue.populate(period, &ids).await?;
        tracing::info!("Report upload: queued {} export(s) for {period}", ids.len());
    }

    // Record the furthest period reached so future runs know where to start from.
    state
        .db
        .settings
        .save_setting(settings::REPORT_UPLOAD_QUEUE_PERIOD_KEY, &target)
        .await?;

    Ok(())
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
        if let Some(max_period) = process_through_period {
            if period_is_after(&entry.period, max_period) {
                tracing::debug!(
                    "Report upload: deferring user {} period {} until configured upload day",
                    entry.user_id,
                    entry.period
                );
                continue;
            }
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

    let (year, month) = parse_year_month(&entry.period)?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| AppError::Internal(format!("invalid period {}", entry.period)))?;
    let last_day = last_day_of_month(year, month);
    let to = NaiveDate::from_ymd_opt(year, month, last_day)
        .ok_or_else(|| AppError::Internal(format!("invalid period end {}", entry.period)))?;

    let is_historical_only = user.archived_at.is_some() || !user.tracks_time;
    let submission_exempt = !crate::roles::has_submission_obligation(&user.role, user.weekly_hours);
    let reports_db = crate::repository::ReportDb::new(state.pool.clone());

    let start_date_review_blocks_upload =
        entry.requires_start_date_review && user.start_date > from;
    if start_date_review_blocks_upload
        || reports_db
            .has_report_content_before_start_date(user.id, from, to, user.start_date)
            .await?
    {
        let reason = if start_date_review_blocks_upload {
            "a start-date change queued this period for review"
        } else {
            "stored report rows exist before the current start date"
        };
        let msg = format!(
            "User {} ({} {}) has current start date {} inside or after period {}, and {reason}. \
             Correct the start date or historical rows before retrying the timesheet PDF export.",
            user.id, user.first_name, user.last_name, user.start_date, entry.period
        );
        tracing::warn!(target: "zerf::report_upload", "{msg}");
        crate::services::notifications::notify_admins_system_error(
            state,
            &format!("report_upload_pre_start_{}_{}", user.id, entry.period),
            "Report PDF upload blocked",
            &msg,
        )
        .await;
        return Ok(());
    }

    if !is_historical_only
        && !submission_exempt
        && reports_db
            .has_requested_absences_in_period(user.id, from, to)
            .await?
    {
        let msg = format!(
            "User {} ({} {}) has pending absence requests in period {}. \
             Decide those requests before retrying the timesheet PDF export.",
            user.id, user.first_name, user.last_name, entry.period
        );
        tracing::warn!(target: "zerf::report_upload", "{msg}");
        return Ok(());
    }

    if is_historical_only
        && reports_db
            .has_unresolved_time_entries_in_range(user.id, from, to)
            .await?
    {
        let msg = format!(
            "User {} ({} {}) is archived or has time tracking disabled, but period {} still contains draft, submitted, or unresolved rejected time entries. \
             Resolve those rows before retrying the timesheet PDF export.",
            user.id, user.first_name, user.last_name, entry.period
        );
        tracing::warn!(target: "zerf::report_upload", "{msg}");
        crate::services::notifications::notify_admins_system_error(
            state,
            &format!("report_upload_unsettled_time_{}_{}", user.id, entry.period),
            "Report PDF upload blocked",
            &msg,
        )
        .await;
        return Ok(());
    }

    // Skip the normal day-coverage gate for historical-only users only after
    // confirming the month has no unresolved time-entry workflow rows. Missing
    // workdays can be a legitimate historical shape after archive/disable, but
    // draft, submitted, and still-rejected rows are undecided data.
    let submitted = if is_historical_only {
        true
    } else {
        all_weeks_ready_for_timesheet_export(
            &state.pool,
            user.id,
            from,
            to,
            user.start_date,
            submission_exempt,
            user.workdays_per_week,
        )
        .await?
    };

    if !submitted {
        // Not ready yet — leave in queue for the next daily check.
        return Ok(());
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

    // Build path: <period>/<period>_Stundenzettel_<First>_<Last>.pdf  (spaces → underscores)
    let first = user.first_name.replace(' ', "_");
    let last = user.last_name.replace(' ', "_");
    let folder = entry.period.clone();
    let filename = format!("{}_Stundenzettel_{}_{}.pdf", entry.period, first, last);
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
    let day_of_month: u8 =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_DAY_OF_MONTH_KEY, "5")
            .await?
            .parse()
            .unwrap_or(5);
    let password =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_PASSWORD_KEY, "").await?;
    Ok((enabled, url, day_of_month, password))
}

fn prev_period(today: NaiveDate) -> String {
    let (year, month) = if today.month() == 1 {
        (today.year() - 1, 12u32)
    } else {
        (today.year(), today.month() - 1)
    };
    format!("{:04}-{:02}", year, month)
}

fn period_before(period: &str) -> AppResult<String> {
    let (year, month) = parse_year_month(period)?;
    let (previous_year, previous_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    Ok(format!("{previous_year:04}-{previous_month:02}"))
}

fn period_is_after(left: &str, right: &str) -> bool {
    match (parse_year_month(left), parse_year_month(right)) {
        (Ok(left), Ok(right)) => left > right,
        _ => false,
    }
}

fn parse_year_month(period: &str) -> AppResult<(i32, u32)> {
    let (y, m) = period
        .split_once('-')
        .ok_or_else(|| AppError::Internal(format!("invalid period string: {period}")))?;
    let year: i32 = y
        .parse()
        .map_err(|_| AppError::Internal(format!("invalid year in period: {period}")))?;
    let month: u32 = m
        .parse()
        .map_err(|_| AppError::Internal(format!("invalid month in period: {period}")))?;
    Ok((year, month))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn prev_period_returns_previous_month() {
        assert_eq!(prev_period(d(2026, 6, 10)), "2026-05");
    }

    #[test]
    fn prev_period_wraps_january_to_december() {
        assert_eq!(prev_period(d(2026, 1, 5)), "2025-12");
    }

    #[test]
    fn period_before_returns_previous_month() {
        assert_eq!(period_before("2026-06").unwrap(), "2026-05");
    }

    #[test]
    fn period_before_wraps_january_to_december() {
        assert_eq!(period_before("2026-01").unwrap(), "2025-12");
    }

    #[test]
    fn period_is_after_compares_by_year_and_month() {
        assert!(period_is_after("2026-06", "2026-05"));
        assert!(period_is_after("2026-01", "2025-12"));
        assert!(!period_is_after("2026-05", "2026-05"));
        assert!(!period_is_after("2026-04", "2026-05"));
    }

    #[test]
    fn parse_year_month_extracts_year_and_month() {
        assert_eq!(parse_year_month("2026-05").unwrap(), (2026, 5));
    }

    #[test]
    fn parse_year_month_rejects_invalid() {
        assert!(parse_year_month("bad").is_err());
        assert!(parse_year_month("2026-xx").is_err());
    }

    #[test]
    fn periods_to_backfill_empty_last_queued_returns_only_target() {
        assert_eq!(periods_to_backfill("", "2026-05"), vec!["2026-05"]);
    }

    #[test]
    fn periods_to_backfill_same_period_returns_empty() {
        assert_eq!(
            periods_to_backfill("2026-05", "2026-05"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn periods_to_backfill_future_last_queued_returns_empty() {
        // Corrupt/future stored value — nothing to do.
        assert_eq!(
            periods_to_backfill("2026-07", "2026-05"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn periods_to_backfill_one_gap_returns_single_month() {
        assert_eq!(periods_to_backfill("2026-04", "2026-05"), vec!["2026-05"]);
    }

    #[test]
    fn periods_to_backfill_multi_month_gap_returns_all_months() {
        assert_eq!(
            periods_to_backfill("2026-03", "2026-06"),
            vec!["2026-04", "2026-05", "2026-06"]
        );
    }

    #[test]
    fn periods_to_backfill_crosses_year_boundary() {
        assert_eq!(
            periods_to_backfill("2025-11", "2026-02"),
            vec!["2025-12", "2026-01", "2026-02"]
        );
    }

    #[test]
    fn periods_to_backfill_corrupt_last_queued_returns_only_target() {
        assert_eq!(periods_to_backfill("bad-data", "2026-05"), vec!["2026-05"]);
    }
}
