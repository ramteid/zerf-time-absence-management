//! Monthly timesheet PDF upload to Nextcloud.
//!
//! The loop checks once per day (at midnight in the app timezone). When:
//!   - report upload is enabled
//!   - a share URL is configured
//!   - today's day-of-month >= the configured upload day
//!   - the previous month has not already been uploaded this cycle
//! it builds a combined PDF for all time-tracking users and uploads it.
//!
//! `report_upload_last_period` is set to "YYYY-MM" ONLY after a successful
//! upload, so a failed upload is retried on the next daily check. On restart
//! the setting prevents re-uploading a month that was already exported.

use crate::AppState;
use chrono::{Datelike, NaiveDate};

/// Determine which period (previous month as "YYYY-MM") should be uploaded,
/// or `None` when no upload is due yet.
///
/// Logic:
///  - Return `None` when `today.day() < day_of_month` (upload day not reached).
///  - Compute previous month relative to `today`.
///  - Return `None` when `last_period` already equals the previous month string
///    (already uploaded this cycle).
///  - Otherwise return `Some(prev_month_string)`.
pub fn due_period(today: NaiveDate, day_of_month: u8, last_period: &str) -> Option<String> {
    if today.day() < u32::from(day_of_month) {
        return None;
    }
    // Compute the first day of the previous month.
    let (prev_year, prev_month) = if today.month() == 1 {
        (today.year() - 1, 12u32)
    } else {
        (today.year(), today.month() - 1)
    };
    let period = format!("{:04}-{:02}", prev_year, prev_month);
    if last_period == period {
        return None; // Already uploaded this period.
    }
    Some(period)
}

/// Background loop: checks once per day whether the monthly PDF upload is due.
pub async fn run_loop(state: AppState) {
    loop {
        // Sleep until just after midnight in the app timezone.
        let tz = crate::services::settings::load_app_timezone(&state.pool).await;
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

/// Perform one upload attempt if the conditions are met.
async fn run_once(state: &AppState) -> crate::error::AppResult<()> {
    use crate::services::settings;

    let enabled = settings::load_setting(
        &state.pool,
        settings::REPORT_UPLOAD_ENABLED_KEY,
        "false",
    )
    .await?
        == "true";
    if !enabled {
        return Ok(());
    }

    let url = settings::load_setting(&state.pool, settings::REPORT_UPLOAD_URL_KEY, "").await?;
    if url.is_empty() {
        return Ok(());
    }

    let day_of_month: u8 = settings::load_setting(
        &state.pool,
        settings::REPORT_UPLOAD_DAY_OF_MONTH_KEY,
        "5",
    )
    .await?
    .parse()
    .unwrap_or(5);

    let last_period =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_LAST_PERIOD_KEY, "")
            .await?;

    let today = settings::app_today(&state.pool).await;

    let period = match due_period(today, day_of_month, &last_period) {
        Some(p) => p,
        None => return Ok(()), // Nothing due yet.
    };

    // Parse the period string into date bounds.
    let (year, month) = parse_year_month(&period)?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| crate::error::AppError::Internal(format!("bad period {period}")))?;
    let last_day = crate::time_calc::last_day_of_month(year, month);
    let to = NaiveDate::from_ymd_opt(year, month, last_day)
        .ok_or_else(|| crate::error::AppError::Internal(format!("bad period end {period}")))?;

    tracing::info!("Report upload: building PDF for {period}");

    let bytes = crate::services::reports::build_all_users_timesheet_pdf(state, from, to).await?;
    if bytes.is_empty() {
        return Err(crate::error::AppError::Internal(
            "Generated PDF is empty — refusing to upload.".into(),
        ));
    }

    let (base, token) = crate::services::nextcloud::parse_share_url(&url)?;
    let password =
        settings::load_setting(&state.pool, settings::REPORT_UPLOAD_PASSWORD_KEY, "").await?;
    let pw = if password.is_empty() {
        None
    } else {
        Some(password.as_str())
    };

    let filename = format!("zerf-timesheets-{period}.pdf");
    crate::services::nextcloud::upload_file(&base, &token, pw, &filename, bytes).await?;

    // Mark this period as done ONLY after successful upload.
    state
        .db
        .settings
        .save_setting(settings::REPORT_UPLOAD_LAST_PERIOD_KEY, &period)
        .await?;

    tracing::info!("Report upload: uploaded {filename} to Nextcloud");
    Ok(())
}

fn parse_year_month(period: &str) -> crate::error::AppResult<(i32, u32)> {
    let (y, m) = period.split_once('-').ok_or_else(|| {
        crate::error::AppError::Internal(format!("invalid period string: {period}"))
    })?;
    let year: i32 = y.parse().map_err(|_| {
        crate::error::AppError::Internal(format!("invalid year in period: {period}"))
    })?;
    let month: u32 = m.parse().map_err(|_| {
        crate::error::AppError::Internal(format!("invalid month in period: {period}"))
    })?;
    Ok((year, month))
}

async fn notify_admins_upload_failed(state: &AppState, message: &str) {
    let all_users = match state.db.users.find_all_ordered().await {
        Ok(u) => u,
        Err(_) => return,
    };
    let title = format!("Nextcloud report upload failed: {message}");
    for user in all_users.into_iter().filter(|u| u.active && u.is_admin()) {
        crate::services::notifications::create(
            state,
            user.id,
            "report_upload_failed",
            &title,
            "",
            None,
            None,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn due_period_returns_none_before_upload_day() {
        // day_of_month=5, today is the 4th → not due yet.
        assert_eq!(due_period(d(2026, 6, 4), 5, ""), None);
    }

    #[test]
    fn due_period_returns_prev_month_on_upload_day() {
        assert_eq!(
            due_period(d(2026, 6, 5), 5, ""),
            Some("2026-05".to_string())
        );
    }

    #[test]
    fn due_period_returns_prev_month_after_upload_day() {
        assert_eq!(
            due_period(d(2026, 6, 20), 5, ""),
            Some("2026-05".to_string())
        );
    }

    #[test]
    fn due_period_wraps_january_to_december() {
        // January → previous month is December of previous year.
        assert_eq!(
            due_period(d(2026, 1, 10), 5, ""),
            Some("2025-12".to_string())
        );
    }

    #[test]
    fn due_period_skips_already_done_period() {
        // Already uploaded May 2026.
        assert_eq!(due_period(d(2026, 6, 10), 5, "2026-05"), None);
    }

    #[test]
    fn due_period_does_not_skip_different_period() {
        // Last uploaded March; May is now due.
        assert_eq!(
            due_period(d(2026, 6, 10), 5, "2026-03"),
            Some("2026-05".to_string())
        );
    }

    #[test]
    fn due_period_first_run_empty_last_period() {
        assert_eq!(
            due_period(d(2026, 6, 15), 5, ""),
            Some("2026-05".to_string())
        );
    }
}
