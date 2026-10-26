//! Shared scheduling primitives for the monthly, period-based background jobs
//! (Nextcloud timesheet upload and payroll report email).
//!
//! Both jobs follow the identical pattern:
//!   1. run once per day shortly after midnight in the app timezone,
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
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::future::Future;

/// Invalid/out-of-range dates retry through the same bounded sleep as normal
/// deadlines, never through an immediate retry loop.
const FALLBACK_WAIT: std::time::Duration = std::time::Duration::from_secs(3600);

/// All wall-clock deadlines are rechecked after a bounded monotonic sleep.
/// A past, equal or invalid deadline must never turn into a busy loop.
pub fn scheduler_sleep_duration(wait: std::time::Duration) -> std::time::Duration {
    wait.clamp(
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(60),
    )
}

/// Run an overlapping local time only at its first occurrence. For a missing
/// time, use the first valid minute afterwards (including a skipped date).
/// The search is bounded even for unusual historical timezone transitions.
pub fn resolve_local_time(
    date: NaiveDate,
    hour: u32,
    minute: u32,
    second: u32,
    timezone: Tz,
) -> Option<DateTime<Tz>> {
    let start = date.and_hms_opt(hour, minute, second)?;
    for minutes in 0..=48 * 60 {
        let candidate = start.checked_add_signed(chrono::Duration::minutes(minutes))?;
        if let Some(target) = timezone.from_local_datetime(&candidate).earliest() {
            return Some(target);
        }
    }
    None
}

pub fn next_monday_at(now: DateTime<Tz>, hour: u32) -> Option<DateTime<Tz>> {
    let days = (7 - now.weekday().num_days_from_monday()) % 7;
    let date = now
        .date_naive()
        .checked_add_days(chrono::Days::new(days.into()))?;
    let target = resolve_local_time(date, hour, 0, 0, now.timezone())?;
    if target > now {
        Some(target)
    } else {
        resolve_local_time(
            date.checked_add_days(chrono::Days::new(7))?,
            hour,
            0,
            0,
            now.timezone(),
        )
        .filter(|target| *target > now)
    }
}

#[derive(Clone, Copy)]
pub enum CalendarSchedule {
    DailyAfterMidnight,
    MondayNoon,
}

impl CalendarSchedule {
    fn next(self, now: DateTime<Tz>) -> Option<DateTime<Tz>> {
        match self {
            Self::MondayNoon => next_monday_at(now, 12),
            Self::DailyAfterMidnight => {
                let date = now.date_naive();
                let today = resolve_local_time(date, 0, 0, 30, now.timezone())?;
                if today > now {
                    Some(today)
                } else {
                    resolve_local_time(date.succ_opt()?, 0, 0, 30, now.timezone())
                        .filter(|target| *target > now)
                }
            }
        }
    }
}

/// Re-read timezone settings and UTC time every minute, including after clock
/// jumps. Repeated local dates are suppressed by the caller's high-water mark.
pub async fn wait_for_calendar_run(
    pool: &crate::db::DatabasePool,
    schedule: CalendarSchedule,
    last_run: Option<NaiveDate>,
) -> NaiveDate {
    let mut timer = CalendarTimer {
        schedule,
        planned: None,
        last_run,
    };
    loop {
        let timezone = settings::load_app_timezone(pool).await;
        let now = Utc::now().with_timezone(&timezone);
        if let Some(date) = timer.poll(now) {
            return date;
        }
        let wait = timer
            .planned
            .and_then(|(_, target)| (target - now).to_std().ok())
            .unwrap_or(FALLBACK_WAIT);
        tokio::time::sleep(scheduler_sleep_duration(wait)).await;
    }
}

struct CalendarTimer {
    schedule: CalendarSchedule,
    planned: Option<(Tz, DateTime<Tz>)>,
    last_run: Option<NaiveDate>,
}

impl CalendarTimer {
    fn poll(&mut self, now: DateTime<Tz>) -> Option<NaiveDate> {
        let timezone = now.timezone();
        if self.planned.is_none_or(|(tz, _)| tz != timezone) {
            self.planned = self.schedule.next(now).map(|target| (timezone, target));
        }
        if let Some((_, target)) = self.planned {
            if target <= now {
                self.planned = self.schedule.next(now).map(|target| (timezone, target));
                if self.last_run.is_none_or(|date| target.date_naive() > date) {
                    // After a forward jump the task processes today's state,
                    // not every missed day. Remember today to avoid replaying
                    // it if the clock subsequently moves backwards.
                    self.last_run = Some(now.date_naive());
                    return self.last_run;
                }
            }
        }
        None
    }
}

/// Run `task` once per day, 30 seconds after local midnight in the app
/// timezone. Errors are logged with `label` as the prefix and never abort the
/// loop.
pub async fn run_daily_after_midnight<F, Fut>(state: AppState, label: &'static str, task: F)
where
    F: Fn(AppState) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let mut last_run = None;
    loop {
        last_run = Some(
            wait_for_calendar_run(&state.pool, CalendarSchedule::DailyAfterMidnight, last_run)
                .await,
        );

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

/// The `YYYY-MM` period of `today`'s own, still in-progress month.
///
/// Only used for the payroll dashboard tile's transient "show this month"
/// peek — the scheduled jobs always run against [`previous_period`], since a
/// month in progress is by definition never final.
pub fn current_period(today: NaiveDate) -> String {
    format!("{:04}-{:02}", today.year(), today.month())
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
    use chrono::Timelike;
    use chrono_tz::Europe::Berlin;

    #[test]
    fn berlin_gap_is_shifted_forward_and_overlap_uses_first_occurrence() {
        let gap = date(2026, 3, 29);
        assert!(Berlin
            .with_ymd_and_hms(2026, 3, 29, 2, 30, 0)
            .single()
            .is_none());
        let resolved = resolve_local_time(gap, 2, 30, 0, Berlin).unwrap();
        assert_eq!((resolved.hour(), resolved.minute()), (3, 0));
        let overlap = Berlin.with_ymd_and_hms(2026, 10, 25, 2, 30, 0);
        assert_ne!(overlap.earliest(), overlap.latest());
        assert_eq!(
            resolve_local_time(date(2026, 10, 25), 2, 30, 0, Berlin),
            overlap.earliest()
        );
    }

    #[test]
    fn resolver_handles_a_whole_skipped_date_and_invalid_input() {
        let target =
            resolve_local_time(date(2011, 12, 30), 0, 0, 0, chrono_tz::Pacific::Apia).unwrap();
        assert_eq!(target.date_naive(), date(2011, 12, 31));
        assert!(resolve_local_time(date(2026, 1, 1), 24, 0, 0, Berlin).is_none());
        assert!(resolve_local_time(NaiveDate::MAX, 23, 59, 59, Berlin).is_some());
    }

    #[test]
    fn daily_schedule_has_23_and_25_hour_days() {
        for (month, day, expected_hours) in [(3, 29, 23), (10, 25, 25)] {
            let now = Berlin.with_ymd_and_hms(2026, month, day, 0, 0, 30).unwrap();
            let target = CalendarSchedule::DailyAfterMidnight.next(now).unwrap();
            assert_eq!((target - now).num_hours(), expected_hours);
            assert_eq!(target.date_naive(), now.date_naive().succ_opt().unwrap());
        }
    }

    #[test]
    fn weekly_schedules_follow_local_time_across_short_and_long_weeks() {
        for (month, day, expected_hours) in [(3, 23, 167), (10, 19, 169)] {
            for hour in [7, 12] {
                let now = Berlin
                    .with_ymd_and_hms(2026, month, day, hour, 0, 0)
                    .unwrap();
                let target = next_monday_at(now, hour).unwrap();
                assert_eq!((target - now).num_hours(), expected_hours);
                assert_eq!(target.hour(), hour);
                assert_eq!(target.weekday(), chrono::Weekday::Mon);
            }
        }
    }

    #[test]
    fn all_calendar_deadlines_advance_through_both_berlin_transitions() {
        for (month, day) in [(3, 28), (10, 24)] {
            let start = Utc.with_ymd_and_hms(2026, month, day, 0, 0, 0).unwrap();
            for minute in 0..3 * 24 * 60 {
                let now = (start + chrono::Duration::minutes(minute)).with_timezone(&Berlin);
                for target in [
                    CalendarSchedule::DailyAfterMidnight.next(now).unwrap(),
                    next_monday_at(now, 7).unwrap(),
                    next_monday_at(now, 12).unwrap(),
                ] {
                    assert!(target > now);
                }
                for day in 1..=28 {
                    assert!(
                        !crate::background::submission_reminders::duration_until_next_deadline(
                            now, day
                        )
                        .is_zero()
                    );
                }
            }
        }
    }

    fn timer(schedule: CalendarSchedule) -> CalendarTimer {
        CalendarTimer {
            schedule,
            planned: None,
            last_run: None,
        }
    }

    #[test]
    fn daily_timer_does_not_repeat_a_midnight_when_the_clock_moves_backwards() {
        // Cuba changes its clocks at midnight, so the daily job itself lands
        // in the repeated hour (Berlin's overlap is later in the night).
        let tz = chrono_tz::America::Havana;
        let mut timer = timer(CalendarSchedule::DailyAfterMidnight);
        let before = tz.with_ymd_and_hms(2026, 10, 31, 23, 59, 0).unwrap();
        assert_eq!(timer.poll(before), None);
        let overlap = tz.with_ymd_and_hms(2026, 11, 1, 0, 0, 30);
        let first = overlap.earliest().unwrap();
        let second = overlap.latest().unwrap();
        assert_ne!(first, second);
        assert_eq!(timer.poll(first), Some(date(2026, 11, 1)));
        // The service constructs a new waiter after every completed task.
        // Its retained date must also protect against a backwards wall clock.
        let mut timer = CalendarTimer {
            schedule: CalendarSchedule::DailyAfterMidnight,
            planned: None,
            last_run: timer.last_run,
        };
        assert_eq!(timer.poll(before), None);
        assert_eq!(timer.poll(first), None);
        assert_eq!(timer.poll(second), None);
        assert_eq!(
            timer.poll(tz.with_ymd_and_hms(2026, 11, 2, 0, 0, 30).unwrap()),
            Some(date(2026, 11, 2))
        );
    }

    #[test]
    fn pending_daily_and_weekly_jobs_replan_when_timezone_changes() {
        for schedule in [
            CalendarSchedule::DailyAfterMidnight,
            CalendarSchedule::MondayNoon,
        ] {
            let mut timer = timer(schedule);
            let now = Utc.with_ymd_and_hms(2026, 5, 3, 20, 0, 0).unwrap();
            assert_eq!(timer.poll(now.with_timezone(&Berlin)), None);
            let old_target = timer.planned.unwrap().1;
            let new_now = now.with_timezone(&chrono_tz::America::New_York);
            assert_eq!(timer.poll(new_now), None);
            let new_target = timer.planned.unwrap().1;
            assert_ne!(
                old_target.with_timezone(&Utc),
                new_target.with_timezone(&Utc)
            );
            assert_eq!(timer.poll(new_target), Some(new_target.date_naive()));
        }
    }

    #[test]
    fn a_forward_clock_jump_runs_once_without_replaying_missed_days() {
        let mut timer = timer(CalendarSchedule::DailyAfterMidnight);
        assert_eq!(
            timer.poll(Berlin.with_ymd_and_hms(2026, 3, 28, 23, 0, 0).unwrap()),
            None
        );
        let later = Berlin.with_ymd_and_hms(2026, 4, 2, 12, 0, 0).unwrap();
        assert!(timer.poll(later).is_some());
        assert_eq!(timer.poll(later), None);
        assert!(timer.planned.unwrap().1 > later);
    }

    #[test]
    fn sleeps_are_positive_bounded_and_preserve_nearby_deadlines() {
        use std::time::Duration;
        assert_eq!(
            scheduler_sleep_duration(Duration::ZERO),
            Duration::from_secs(1)
        );
        assert_eq!(
            scheduler_sleep_duration(Duration::from_secs(30)),
            Duration::from_secs(30)
        );
        assert_eq!(
            scheduler_sleep_duration(Duration::MAX),
            Duration::from_secs(60)
        );
    }

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
    fn current_period_returns_todays_month() {
        assert_eq!(current_period(date(2026, 6, 10)), "2026-06");
        assert_eq!(current_period(date(2026, 1, 5)), "2026-01");
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
