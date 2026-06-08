//! Background task: check on the configured deadline day of each month
//! whether users have submitted all past weeks' time entries.
//! Users with weekly_hours = 0 are skipped (non-booking users).

use crate::db::DatabasePool;
use crate::services::settings::{
    app_today, load_setting, load_smtp_config, DEFAULT_TIMEZONE, SUBMISSION_REMINDERS_ENABLED_KEY,
    TIMEZONE_KEY,
};
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use std::time::Duration;

const SUBMISSION_DEADLINE_DAY_KEY: &str = "submission_deadline_day";

/// Returns the duration to wait until the next occurrence of `day_of_month` at 07:00 local time.
pub fn duration_until_next_deadline(
    now: chrono::DateTime<chrono_tz::Tz>,
    day_of_month: u8,
) -> Duration {
    let day = day_of_month as u32;
    let today = now.date_naive();

    // Try this month's deadline day
    let candidate_day = day.min(crate::time_calc::last_day_of_month(
        today.year(),
        today.month(),
    ));
    let Some(candidate) = NaiveDate::from_ymd_opt(today.year(), today.month(), candidate_day)
    else {
        return Duration::from_secs(60);
    };

    if let Some(target) = resolve_local_datetime(candidate, 7, now.timezone()) {
        if target > now {
            return (target - now).to_std().unwrap_or(Duration::from_secs(60));
        }
    }

    // Already past or ambiguous – schedule next month
    let next_deadline_date = advance_one_month(today, day);
    let next_deadline =
        (7..=23).find_map(|hour| resolve_local_datetime(next_deadline_date, hour, now.timezone()));
    next_deadline
        .and_then(|deadline| (deadline - now).to_std().ok())
        .unwrap_or(Duration::from_secs(60))
}

/// Resolve a naive date + hour to a local datetime, handling DST gaps/ambiguities.
fn resolve_local_datetime(
    date: NaiveDate,
    hour: u32,
    timezone: chrono_tz::Tz,
) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    let naive = date.and_hms_opt(hour, 0, 0)?;
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt),
        chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest),
        chrono::LocalResult::None => {
            // Hour falls in a DST gap; try one hour later
            let fallback = date.and_hms_opt(hour + 1, 0, 0)?;
            timezone.from_local_datetime(&fallback).earliest()
        }
    }
}

fn advance_one_month(date: NaiveDate, desired_day: u32) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    let actual_day = desired_day.min(crate::time_calc::last_day_of_month(year, month));
    NaiveDate::from_ymd_opt(year, month, actual_day).unwrap_or(date)
}

/// Collect ISO week labels (e.g. "2026-W03") where the user has unsubmitted
/// workdays, from their start_date up to (but not including) the current week.
///
/// A workday is complete when it is covered by either:
///   - at least one submitted/approved time entry (crediting or non-crediting), OR
///   - an approved absence.
///
/// A workday with any draft or rejected entry is incomplete even if another
/// entry on the same day is submitted.
async fn find_unsubmitted_weeks(
    pool: &DatabasePool,
    user_id: i64,
    user_start: NaiveDate,
    workdays_per_week: i16,
) -> Vec<NaiveDate> {
    let today = app_today(pool).await;

    // Monday of the current week.
    let current_week_monday = crate::time_calc::week_monday(today);
    // Only check fully elapsed weeks. A week is fully elapsed when its Sunday
    // is strictly in the past (all 7 days have passed). The current week is
    // always excluded because the user can still log time for today.
    let last_checked_monday = current_week_monday - chrono::Duration::days(7);
    let check_to = last_checked_monday + chrono::Duration::days(6);
    if user_start > check_to {
        return vec![];
    }

    // Align to full weeks: start from the Monday of the user_start week.
    let first_monday = crate::time_calc::week_monday(user_start);

    // Load holidays in the check range.
    let holiday_set: std::collections::HashSet<NaiveDate> =
        crate::repository::HolidayDb::new(pool.clone())
            .get_dates_in_range(first_monday, check_to)
            .await
            .unwrap_or_default();

    let time_db = crate::repository::TimeEntryDb::new(pool.clone());
    let absence_db = crate::repository::AbsenceDb::new(pool.clone());

    // Load submitted/approved time entry dates.
    let submitted_dates: std::collections::HashSet<NaiveDate> = time_db
        .get_submitted_dates_in_range(user_id, first_monday, check_to)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Load dates with incomplete entries (draft/rejected).
    let incomplete_dates: std::collections::HashSet<NaiveDate> = time_db
        .get_incomplete_dates_in_range(user_id, first_monday, check_to)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Load approved absence date ranges and expand to a date set.
    let absence_rows: Vec<(NaiveDate, NaiveDate, String)> = absence_db
        .approved_ranges_in_period(user_id, first_monday, check_to)
        .await
        .unwrap_or_default();

    let absent_days =
        crate::services::reports::expand_absence_date_set(&absence_rows, first_monday, check_to);

    // Check each fully elapsed week using the same three-step logic as
    // `check_weeks_all_submitted` in services/reports.rs so that reminders
    // are only sent for weeks that are genuinely not yet submitted.
    let mut incomplete_week_mondays = Vec::new();
    let mut week_monday = first_monday;
    while week_monday <= last_checked_monday {
        // Step 1: any incomplete (draft/rejected) entry anywhere in the
        // Mon–Sun window means the week is not fully submitted.
        let has_incomplete = (0..7i64)
            .any(|d| incomplete_dates.contains(&(week_monday + chrono::Duration::days(d))));

        // Step 2: if at least one day has a submitted/approved entry (and no
        // incomplete entries per step 1), treat the whole week as submitted.
        let has_submitted =
            (0..7i64).any(|d| submitted_dates.contains(&(week_monday + chrono::Duration::days(d))));

        // Step 3: if nothing was submitted, the week is only "excused" when
        // every contract workday is either before the contract start, a
        // public holiday, covered by an approved absence, or in the future.
        let all_excused = !has_submitted
            && (0..i64::from(workdays_per_week)).all(|d| {
                let day = week_monday + chrono::Duration::days(d);
                day < user_start
                    || holiday_set.contains(&day)
                    || absent_days.contains(&day)
                    || day >= today
            });

        let week_incomplete = has_incomplete || (!has_submitted && !all_excused);
        if week_incomplete {
            incomplete_week_mondays.push(week_monday);
        }
        week_monday += chrono::Duration::days(7);
    }

    incomplete_week_mondays
}

/// Run one check pass for all active non-assistant users.
/// Assistant users have no fixed target schedule and are excluded from
/// submission completeness reminders by role policy.
pub async fn run_check(state: &crate::AppState) {
    let pool = &state.pool;

    // Respect the admin toggle; default is enabled (true).
    let reminders_enabled = load_setting(pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true")
        .await
        .unwrap_or_else(|_| "true".to_string());
    if reminders_enabled == "false" {
        tracing::debug!(target:"zerf::submission_reminders", "Submission reminders are disabled, skipping check");
        return;
    }

    let language = match crate::i18n::load_ui_language(pool).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(target:"zerf::submission_reminders", "load language failed: {e}");
            crate::i18n::Language::default()
        }
    };

    let app_url = state
        .cfg
        .public_url
        .clone()
        .unwrap_or_else(|| "http://localhost".to_string());
    let timezone = load_setting(pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
        .await
        .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
    let today = app_today(pool).await;

    let rows: Vec<crate::repository::ActiveUserRow> =
        match state.db.users.get_active_non_assistant_users().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(target:"zerf::submission_reminders", "fetch users failed: {e}");
                return;
            }
        };
    tracing::debug!(
        target: "zerf::assistant_role",
        reminder_candidate_count = rows.len(),
        today = %today,
        timezone = %timezone,
        "submission reminder pass loaded non-assistant candidates"
    );

    // Load SMTP config once for all users
    let smtp = load_smtp_config(pool).await.map(std::sync::Arc::new);

    for crate::repository::ActiveUserRow {
        id: user_id,
        email: user_email,
        first_name,
        last_name,
        start_date: user_start,
        workdays_per_week,
    } in rows
    {
        let missing_weeks =
            find_unsubmitted_weeks(pool, user_id, user_start, workdays_per_week).await;

        if missing_weeks.is_empty() {
            continue;
        }

        let missing_labels: Vec<String> = missing_weeks
            .iter()
            .map(|monday| crate::i18n::format_week_label(&language, *monday))
            .collect();

        let weeks_str = missing_labels.join(", ");
        let title = crate::i18n::translate(&language, "submission_reminder_title", &[]);
        let body = crate::i18n::translate(
            &language,
            "submission_reminder_body",
            &[("weeks", weeks_str.clone())],
        );
        let timestamp =
            crate::i18n::format_datetime_in_timezone(&language, chrono::Utc::now(), &timezone);
        let email_body = format!(
            "{}\n\n{}",
            crate::i18n::translate(
                &language,
                "submission_reminder_email_body",
                &[
                    ("weeks", missing_labels.join("\n")),
                    ("app_url", app_url.clone()),
                ],
            ),
            timestamp,
        );

        // Use an app-timezone local-day dedupe key so reminders are unique per
        // user/day in configured timezone, not by UTC date.
        let dedupe_key = format!("submission_reminder:{}", today);
        // Only send the in-app signal and email when the row was actually inserted
        // (rows_affected == 0 means the conflict guard fired — reminder already sent today).
        match state
            .db
            .notifications
            .insert_idempotent_with_dedupe_key(
                user_id,
                "submission_reminder",
                &title,
                &body,
                None,
                None,
                Some(&dedupe_key),
            )
            .await
        {
            Ok(true) => {
                let _ = state
                    .notifications
                    .send(crate::services::notifications::NotificationSignal { user_id });
                // Send email best-effort
                crate::email::send_async(
                    smtp.clone(),
                    user_email,
                    format!("{} {}", first_name, last_name),
                    title,
                    email_body,
                );
            }
            Ok(_) => {
                // Conflict guard fired: reminder already sent today, skip email too.
            }
            Err(e) => {
                tracing::warn!(
                    target:"zerf::submission_reminders",
                    "insert notification failed for user {user_id}: {e}"
                );
            }
        }
    }
}

/// Background loop: sleep until the next deadline day at 07:00 then run check.
pub async fn run_loop(pool: DatabasePool, state: crate::AppState) {
    loop {
        let day_str = load_setting(&pool, SUBMISSION_DEADLINE_DAY_KEY, "")
            .await
            .unwrap_or_default();
        let day: Option<u8> = day_str.parse().ok().filter(|&d: &u8| (1..=28).contains(&d));

        if let Some(d) = day {
            let timezone = load_setting(&pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
                .await
                .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
            let tz = timezone
                .parse::<chrono_tz::Tz>()
                .unwrap_or(chrono_tz::Europe::Berlin);
            let wait = duration_until_next_deadline(Utc::now().with_timezone(&tz), d);
            tracing::info!(
                target:"zerf::submission_reminders",
                "Next submission reminder check scheduled in {:?}",
                wait
            );
            tokio::time::sleep(wait).await;
            tracing::info!(target:"zerf::submission_reminders", "Running submission reminder check");
            run_check(&state).await;
        } else {
            // No deadline configured – poll every hour
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Berlin;

    #[test]
    fn deadline_in_future_same_month() {
        // 2026-05-06 08:00 local, deadline day 15 -> should wait until 15th at 07:00
        let now = Berlin.with_ymd_and_hms(2026, 5, 6, 8, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 15);
        // Should be ~8 days 23 hours = 8*86400 + 23*3600 = 774000 seconds
        let secs = dur.as_secs();
        assert!(secs > 7 * 86400, "should be more than 7 days, got {secs}");
        assert!(secs < 10 * 86400, "should be less than 10 days, got {secs}");
    }

    #[test]
    fn deadline_today_but_not_yet() {
        // 2026-05-15 06:00 local, deadline day 15 -> should wait ~1 hour
        let now = Berlin.with_ymd_and_hms(2026, 5, 15, 6, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 15);
        let secs = dur.as_secs();
        assert!(secs >= 3500, "should be about 1 hour, got {secs}");
        assert!(secs <= 3700, "should be about 1 hour, got {secs}");
    }

    #[test]
    fn deadline_already_passed_schedules_next_month() {
        // 2026-05-15 08:00 local, deadline day 10 -> next: June 10 at 07:00
        let now = Berlin.with_ymd_and_hms(2026, 5, 15, 8, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 10);
        let secs = dur.as_secs();
        // ~25.96 days
        assert!(secs > 24 * 86400, "should be >24 days, got {secs}");
        assert!(secs < 27 * 86400, "should be <27 days, got {secs}");
    }

    #[test]
    fn deadline_day_clamped_to_month_end() {
        // Feb 2026: 28 days. Deadline day 28 on Feb 1 -> should target Feb 28
        let now = Berlin.with_ymd_and_hms(2026, 2, 1, 6, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 28);
        let secs = dur.as_secs();
        // ~27 days + 1 hour
        assert!(secs > 26 * 86400, "should be >26 days, got {secs}");
        assert!(secs < 28 * 86400, "should be <28 days, got {secs}");
    }

    #[test]
    fn deadline_december_wraps_to_january() {
        // 2026-12-20 08:00, deadline day 5 -> next: Jan 5, 2027 at 07:00
        let now = Berlin.with_ymd_and_hms(2026, 12, 20, 8, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 5);
        let secs = dur.as_secs();
        // ~15.96 days
        assert!(secs > 14 * 86400, "should be >14 days, got {secs}");
        assert!(secs < 17 * 86400, "should be <17 days, got {secs}");
    }

    // last_day_of_month tests moved to time_calc::tests (canonical location).

    #[test]
    fn advance_one_month_wraps_year() {
        let d = NaiveDate::from_ymd_opt(2026, 12, 15).unwrap();
        let next = advance_one_month(d, 15);
        assert_eq!(next, NaiveDate::from_ymd_opt(2027, 1, 15).unwrap());
    }

    #[test]
    fn advance_one_month_clamps_day() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let next = advance_one_month(d, 31);
        assert_eq!(next, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }

    #[test]
    fn deadline_after_month_end_clamps_to_shorter_next_month() {
        let now = Berlin.with_ymd_and_hms(2026, 3, 31, 8, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 31);
        let secs = dur.as_secs();
        assert!(secs > 29 * 86400, "should be well over 29 days, got {secs}");
        assert!(secs < 31 * 86400, "should be less than 31 days, got {secs}");
    }

    #[test]
    fn deadline_rollover_uses_next_year_when_month_wraps() {
        let now = Berlin.with_ymd_and_hms(2026, 12, 31, 8, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 5);
        let secs = dur.as_secs();
        assert!(secs > 4 * 86400, "should be more than 4 days, got {secs}");
        assert!(secs < 6 * 86400, "should be less than 6 days, got {secs}");
    }

    /// In Europe/Berlin, DST springs forward at 02:00 on the last Sunday of March.
    /// Hour 2 on that day does not exist locally (`LocalResult::None`), so
    /// `resolve_local_datetime` must fall through to the fallback hour.
    #[test]
    fn resolve_local_datetime_handles_dst_spring_forward_gap() {
        use chrono::Timelike;
        // 2026-03-29: clocks jump 02:00 → 03:00 in Europe/Berlin.
        let gap_date = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        // Hour 2 falls in the DST gap — the function must not panic and must
        // return Some (falling back to 03:00 which does exist).
        let result = resolve_local_datetime(gap_date, 2, Berlin);
        assert!(
            result.is_some(),
            "DST gap must fall through to fallback hour"
        );
        // The returned time must be in hour 3 (the first valid local hour).
        assert_eq!(result.unwrap().hour(), 3);
    }
}
