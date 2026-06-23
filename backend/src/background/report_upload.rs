//! Per-employee monthly timesheet PDF upload to Nextcloud.
//!
//! Flow:
//!   1. Each midnight tick: if today.day() >= configured upload day, populate
//!      the export queue for the previous month (idempotent, guarded by the
//!      `report_upload_queue_period` app_setting).
//!   2. Process all pending queue entries: for each (user, period), check
//!      whether all weeks of the period are fully submitted.  If yes, build
//!      a per-user PDF, create the per-month subfolder, upload the file, and
//!      remove the queue entry.  Entries for not-yet-submitted months are left
//!      in the queue for the next daily check (catch-up for late submitters).
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
    reports::{all_weeks_submitted_for_month, build_timesheet_section},
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
            notify_admins_upload_failed(&state, &e.to_string()).await;
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
    let pw = if password.is_empty() { None } else { Some(password.as_str()) };
    process_pending_entries(state, &base, &token, pw).await;

    Ok(())
}

/// Daily scheduled run: populate queue if Stichtag is reached, then process pending entries.
async fn run_once(state: &AppState) -> AppResult<()> {
    let (enabled, url, day_of_month, password) = load_upload_settings(state).await?;
    if !enabled || url.is_empty() {
        return Ok(());
    }

    let today = settings::app_today(&state.pool).await;
    if today.day() >= u32::from(day_of_month) {
        populate_queue_for_prev_month(state, today).await?;
    }

    let (base, token) = nextcloud::parse_share_url(&url)?;
    let pw = if password.is_empty() { None } else { Some(password.as_str()) };
    process_pending_entries(state, &base, &token, pw).await;

    Ok(())
}

/// Populate the export queue for the previous month if not already done.
/// Guards against re-population via the `report_upload_queue_period` setting.
async fn populate_queue_for_prev_month(state: &AppState, today: NaiveDate) -> AppResult<()> {
    let period = prev_period(today);
    let queue_period =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_QUEUE_PERIOD_KEY, "").await?;
    if queue_period == period {
        return Ok(()); // Already populated for this period.
    }

    let (year, month) = parse_year_month(&period)?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| AppError::Internal(format!("invalid period {period}")))?;
    let last_day = last_day_of_month(year, month);
    let to = NaiveDate::from_ymd_opt(year, month, last_day)
        .ok_or_else(|| AppError::Internal(format!("invalid period end {period}")))?;

    // Include deactivated users who had entries/absences in the period so the
    // archive export is complete (see ReportDb::timesheet_members_for_period).
    let members = state.db.reports.timesheet_members_for_period(from, to).await?;
    let ids: Vec<i64> = members.iter().map(|u| u.id).collect();

    state.db.export_queue.populate(&period, &ids).await?;
    state
        .db
        .settings
        .save_setting(settings::REPORT_UPLOAD_QUEUE_PERIOD_KEY, &period)
        .await?;

    tracing::info!("Report upload: queued {} export(s) for {period}", ids.len());
    Ok(())
}

/// Try to upload a PDF for each pending queue entry; leave unready entries in place.
async fn process_pending_entries(state: &AppState, base: &str, token: &str, pw: Option<&str>) {
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

    // Check whether all working weeks in the period have been submitted.
    let submitted = all_weeks_submitted_for_month(
        &state.pool,
        user.id,
        from,
        to,
        user.start_date,
        crate::roles::is_assistant_role(&user.role),
        user.workdays_per_week,
    )
    .await?;

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
    let enabled =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_ENABLED_KEY, "false")
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

fn parse_year_month(period: &str) -> AppResult<(i32, u32)> {
    let (y, m) = period.split_once('-').ok_or_else(|| {
        AppError::Internal(format!("invalid period string: {period}"))
    })?;
    let year: i32 = y
        .parse()
        .map_err(|_| AppError::Internal(format!("invalid year in period: {period}")))?;
    let month: u32 = m
        .parse()
        .map_err(|_| AppError::Internal(format!("invalid month in period: {period}")))?;
    Ok((year, month))
}

async fn notify_admins_upload_failed(state: &AppState, message: &str) {
    let title = format!("Report PDF upload failed: {message}");
    crate::services::notifications::notify_admins_system_error(
        state,
        crate::services::notifications::SYSTEM_ERROR_REPORT_UPLOAD_FAILED,
        &title,
    )
    .await;
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
    fn parse_year_month_extracts_year_and_month() {
        assert_eq!(parse_year_month("2026-05").unwrap(), (2026, 5));
    }

    #[test]
    fn parse_year_month_rejects_invalid() {
        assert!(parse_year_month("bad").is_err());
        assert!(parse_year_month("2026-xx").is_err());
    }
}
