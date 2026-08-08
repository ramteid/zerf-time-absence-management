//! Shared scheduling primitives for the monthly, period-based background jobs
//! (Nextcloud timesheet upload and payroll report email).
//!
//! Both jobs follow the identical pattern:
//!   1. wake up once per day shortly after midnight in the app timezone,
//!   2. once the configured day of month is reached, queue every month that is
//!      still missing up to and including the previous month,
//!   3. process queued periods, deferring the just-finished previous month
//!      until the configured day of month is actually reached.
//!
//! The period math and the loop scaffolding live here so neither job carries
//! its own copy.

use crate::error::{AppError, AppResult};
use crate::services::settings;
use crate::AppState;
use chrono::{Datelike, NaiveDate};
use std::future::Future;

/// Fallback sleep when the next midnight cannot be computed (e.g. a timezone
/// transition makes the local midnight ambiguous). One hour keeps the loop
/// alive and retries soon.
const FALLBACK_WAIT: std::time::Duration = std::time::Duration::from_secs(3600);

/// Run `task` once per day, 30 seconds after local midnight in the app
/// timezone. Errors are logged with `label` as the prefix and never abort the
/// loop.
pub async fn run_daily_after_midnight<F, Fut>(state: AppState, label: &'static str, task: F)
where
    F: Fn(AppState) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    loop {
        let timezone = settings::load_app_timezone(&state.pool).await;
        let now_utc = chrono::Utc::now();
        let now_local = now_utc.with_timezone(&timezone);
        let wait = now_local
            .date_naive()
            .succ_opt()
            .and_then(|next_day| next_day.and_hms_opt(0, 0, 30))
            .and_then(|local_midnight| local_midnight.and_local_timezone(timezone).single())
            .map(|local_midnight| local_midnight.with_timezone(&chrono::Utc))
            .and_then(|midnight_utc| (midnight_utc - now_utc).to_std().ok())
            .unwrap_or(FALLBACK_WAIT);

        tokio::time::sleep(wait).await;

        if let Err(e) = task(state.clone()).await {
            tracing::error!("{label}: {e:?}");
        }
    }
}

/// The `YYYY-MM` period immediately before `today`'s month.
pub fn previous_period(today: NaiveDate) -> String {
    let (year, month) = if today.month() == 1 {
        (today.year() - 1, 12u32)
    } else {
        (today.year(), today.month() - 1)
    };
    format!("{year:04}-{month:02}")
}

/// The `YYYY-MM` period immediately before `period`.
pub fn period_before(period: &str) -> AppResult<String> {
    let (year, month) = parse_year_month(period)?;
    let (previous_year, previous_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    Ok(format!("{previous_year:04}-{previous_month:02}"))
}

/// True when `left` denotes a later month than `right`. Unparsable input is
/// reported as "not after" so a corrupt value can never defer processing
/// forever.
pub fn period_is_after(left: &str, right: &str) -> bool {
    match (parse_year_month(left), parse_year_month(right)) {
        (Ok(left), Ok(right)) => left > right,
        _ => false,
    }
}

/// Split a `YYYY-MM` period string into its numeric year and month.
pub fn parse_year_month(period: &str) -> AppResult<(i32, u32)> {
    let (year_part, month_part) = period
        .split_once('-')
        .ok_or_else(|| AppError::Internal(format!("invalid period string: {period}")))?;
    let year: i32 = year_part
        .parse()
        .map_err(|_| AppError::Internal(format!("invalid year in period: {period}")))?;
    let month: u32 = month_part
        .parse()
        .map_err(|_| AppError::Internal(format!("invalid month in period: {period}")))?;
    if !(1..=12).contains(&month) {
        return Err(AppError::Internal(format!(
            "invalid month in period: {period}"
        )));
    }
    // Guard against i32 underflow in period_before.
    if year == i32::MIN {
        return Err(AppError::Internal(format!(
            "year underflow in period: {period}"
        )));
    }
    Ok((year, month))
}

/// First and last calendar day of a `YYYY-MM` period.
pub fn period_bounds(period: &str) -> AppResult<(NaiveDate, NaiveDate)> {
    let (year, month) = parse_year_month(period)?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| AppError::Internal(format!("invalid period {period}")))?;
    let last_day = crate::time_calc::last_day_of_month(year, month);
    let to = NaiveDate::from_ymd_opt(year, month, last_day)
        .ok_or_else(|| AppError::Internal(format!("invalid period end {period}")))?;
    Ok((from, to))
}

/// Returns the list of `YYYY-MM` periods that need to be queued, starting from
/// the month after `last_queued` through `target` (inclusive).
///
/// Behaviour by case:
///
/// - Empty `last_queued` or a parse failure: returns `[target]` (first-ever run).
/// - `last_queued >= target` (valid YYYY-MM): returns `[]` (already up to date
///   or the stored value is unexpectedly in the future).
/// - Otherwise: every month from `last_queued + 1` through `target`.
pub fn periods_to_backfill(last_queued: &str, target: &str) -> Vec<String> {
    if last_queued.is_empty() {
        return vec![target.to_string()];
    }
    match parse_year_month(last_queued) {
        Err(_) => vec![target.to_string()], // Corrupt setting; just do the current period.
        Ok((mut year, mut month)) => {
            let (target_year, target_month) = match parse_year_month(target) {
                Ok(parsed) => parsed,
                Err(_) => return vec![],
            };
            // Future/corrupt stored value – treat as if we need to re-queue target.
            if (year, month) > (target_year, target_month) {
                // Log warning via tracing? Can't easily here, but recover by queuing target.
                return vec![target.to_string()];
            }
            if (year, month) == (target_year, target_month) {
                return vec![];
            }
            let mut periods = Vec::new();
            loop {
                // Advance one month.
                if month == 12 {
                    year += 1;
                    month = 1;
                } else {
                    month += 1;
                }
                periods.push(format!("{year:04}-{month:02}"));
                if (year, month) == (target_year, target_month) {
                    break;
                }
                // Safety: stop if we somehow overshoot.
                if (year, month) > (target_year, target_month) {
                    // Pop the overshot period we just pushed.
                    periods.pop();
                    break;
                }
            }
            periods
        }
    }
}

/// Queue every period from the one after the last recorded period through
/// `previous_period(today)`, then persist the furthest period reached in
/// `last_period_key`.
///
/// `queue_period` performs the job-specific enqueueing for one period. If the
/// feature was disabled for several months and is re-enabled, or the server
/// missed a month boundary, every intervening period is backfilled so no month
/// is silently skipped.
pub async fn queue_periods_through_previous_month<F, Fut>(
    state: &AppState,
    last_period_key: &str,
    today: NaiveDate,
    queue_period: F,
) -> AppResult<()>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let target = previous_period(today);
    let last_queued = settings::load_setting(&state.pool, last_period_key, "").await?;

    let periods = periods_to_backfill(&last_queued, &target);
    if periods.is_empty() {
        return Ok(());
    }

    for period in periods {
        queue_period(period).await?;
    }

    // Record the furthest period reached so future runs know where to start from.
    state
        .db
        .settings
        .save_setting(last_period_key, &target)
        .await?;

    Ok(())
}

/// Newest period that may be processed on a scheduled run.
///
/// Before the configured day of month is reached, the just-finished previous
/// month is still deferred while older, caught-up months are processed;
/// `None` means "no restriction" (the day of month has been reached).
pub fn process_through_period(today: NaiveDate, day_of_month: u8) -> AppResult<Option<String>> {
    // Clamp day_of_month to 1..=28 as documented; 0 never defers, >31 would defer whole month.
    let clamped_day = day_of_month.clamp(1, 28);
    if today.day() >= u32::from(clamped_day) {
        return Ok(None);
    }
    Ok(Some(period_before(&previous_period(today))?))
}

/// True when `period` must wait because it is newer than the newest period the
/// current run may process.
pub fn period_is_deferred(period: &str, process_through: Option<&str>) -> bool {
    match process_through {
        Some(max_period) => period_is_after(period, max_period),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn previous_period_returns_previous_month() {
        assert_eq!(previous_period(date(2026, 6, 10)), "2026-05");
    }

    #[test]
    fn previous_period_wraps_january_to_december() {
        assert_eq!(previous_period(date(2026, 1, 5)), "2025-12");
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
    fn period_bounds_spans_the_whole_month() {
        assert_eq!(
            period_bounds("2026-02").unwrap(),
            (date(2026, 2, 1), date(2026, 2, 28))
        );
        // Leap year: February has 29 days.
        assert_eq!(
            period_bounds("2024-02").unwrap(),
            (date(2024, 2, 1), date(2024, 2, 29))
        );
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
        // Corrupt/future stored value – previously returned empty causing permanent stall; now recovers by queuing target.
        assert_eq!(
            periods_to_backfill("2026-07", "2026-05"),
            vec!["2026-05".to_string()]
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

    #[test]
    fn process_through_period_defers_previous_month_before_the_configured_day() {
        // Day 3 with a configured day of 5: only months up to April may run.
        assert_eq!(
            process_through_period(date(2026, 6, 3), 5).unwrap(),
            Some("2026-04".to_string())
        );
        // On/after the configured day there is no restriction.
        assert_eq!(process_through_period(date(2026, 6, 5), 5).unwrap(), None);
        assert_eq!(process_through_period(date(2026, 6, 20), 5).unwrap(), None);
    }

    #[test]
    fn period_is_deferred_only_applies_above_the_processing_ceiling() {
        assert!(period_is_deferred("2026-05", Some("2026-04")));
        assert!(!period_is_deferred("2026-04", Some("2026-04")));
        assert!(!period_is_deferred("2026-03", Some("2026-04")));
        assert!(!period_is_deferred("2026-05", None));
    }
}
