//! Background task: check on the configured deadline day of each month
//! whether users have submitted all past weeks' time entries.
//! Users with weekly_hours = 0 are skipped (non-booking users).

use crate::db::DatabasePool;
use crate::services::settings::{
    app_today, load_setting, DEFAULT_TIMEZONE, SUBMISSION_REMINDERS_ENABLED_KEY, TIMEZONE_KEY,
};
use chrono::{Datelike, NaiveDate, TimeZone, Timelike, Utc};
use std::time::Duration;

const SUBMISSION_DEADLINE_DAY_KEY: &str = "submission_deadline_day";
const SETTINGS_POLL_INTERVAL: Duration = Duration::from_secs(3600);

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

pub fn scheduler_sleep_duration(deadline_wait: Duration) -> Duration {
    deadline_wait.min(SETTINGS_POLL_INTERVAL)
}

fn deadline_is_due_now(now: chrono::DateTime<chrono_tz::Tz>, day_of_month: u8) -> bool {
    let today = now.date_naive();
    let deadline_day = u32::from(day_of_month).min(crate::time_calc::last_day_of_month(
        today.year(),
        today.month(),
    ));
    today.day() == deadline_day && now.hour() >= 7
}

/// Collect the Mondays of fully elapsed weeks where the user has unsubmitted
/// workdays, from their start_date up to (but not including) the current week.
///
/// Completeness is evaluated per week by the canonical
/// `services::reports::check_weeks_all_submitted`, the same rule that drives
/// the dashboard Submissions tile, the team report, and the monthly PDF
/// upload, so the reminder can never disagree with those views:
///   - any draft or rejected entry anywhere in the week makes it incomplete;
///   - otherwise a single submitted/approved day hands the whole week in, no
///     matter how many days the person worked in it;
///   - a week with nothing booked at all is only complete when nothing was
///     due: every potential workday is a public holiday, covered by a
///     requested/approved/cancellation-pending absence, or lies before the
///     user's start date.
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
    let reports_db = crate::repository::ReportDb::new(pool.clone());

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

    // Load absence date ranges that cover the submission obligation and expand
    // them to a date set. Requested absences count here: while they are pending
    // the employee cannot log entries on those days, so reminders must not ask
    // for impossible time entries.
    let absence_rows: Vec<(NaiveDate, NaiveDate, String)> = reports_db
        .absence_ranges_in_period(user_id, first_monday, check_to)
        .await
        .unwrap_or_default();

    let category_flags = crate::services::reports::AbsenceCategoryFlags::load(pool)
        .await
        .unwrap_or_else(|_| crate::services::reports::AbsenceCategoryFlags {
            by_slug: Default::default(),
        });
    let absent_days = crate::services::reports::expand_absence_date_set(
        &absence_rows,
        first_monday,
        check_to,
        &category_flags,
    );

    // Evaluate each fully elapsed week with the canonical helper so the
    // reminder uses byte-for-byte the same rule as the Submissions tile,
    // the team report, and the monthly PDF upload.
    let mut incomplete_week_mondays = Vec::new();
    let mut week_monday = first_monday;
    while week_monday <= last_checked_monday {
        let week_is_complete = crate::services::reports::check_weeks_all_submitted(
            &[week_monday],
            &holiday_set,
            &absent_days,
            &submitted_dates,
            &incomplete_dates,
            user_start,
            workdays_per_week,
            None,
        );
        if !week_is_complete {
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
        "submission reminder pass loaded non-assistant candidates"
    );

    for crate::repository::ActiveUserRow {
        id: user_id,
        start_date: user_start,
        workdays_per_week,
        ..
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
        let text = crate::i18n::notification_event_text(
            &language,
            "submission_reminder",
            &[("weeks", weeks_str.clone())],
        );
        let email_body = crate::i18n::notification_email_body(
            &language,
            "submission_reminder",
            &[("weeks", missing_labels.join("\n"))],
        );

        // Idempotent per user per local day (configured timezone, not UTC).
        // The key must NOT depend on which weeks are still missing: `run_loop`
        // re-checks every hour while the deadline day lasts, so a content-derived
        // key would mint a fresh notification — and a fresh email — each time
        // someone submits one of their outstanding weeks, nagging them once per
        // hour precisely while they are working through the backlog.
        let dedupe_key = format!("submission_reminder:{}", today);
        crate::services::notifications::deliver(
            state,
            &crate::services::notifications::Outgoing::new(
                user_id,
                &language,
                "submission_reminder",
                &text.title,
                &text.body,
            )
            .email_body(&email_body)
            .dedupe_key(&dedupe_key),
        )
        .await;
    }
}

/// Both month-boundary reminders go out from 08:00 local time — the start of
/// the working day, not the middle of the night.
fn reminder_hour_reached(now: chrono::DateTime<chrono_tz::Tz>) -> bool {
    now.hour() >= 8
}

/// Days between the month-boundary reminders that run through the new month:
/// the 1st, the 4th, the 7th and so on.
const REMINDER_INTERVAL_DAYS: u32 = 3;

/// True on every third day from the 1st, once the reminder hour is reached.
///
/// Both month-boundary passes share this rhythm, and they have to: what they
/// ask for is exactly what holds the monthly reports up. Asking once would
/// leave a report blocked by something nobody is being reminded of any more —
/// and would lose the reminder entirely for a month whose 1st the server spent
/// restarting.
fn month_reminder_is_due_now(now: chrono::DateTime<chrono_tz::Tz>) -> bool {
    reminder_hour_reached(now)
        && (now.date_naive().day() - 1).is_multiple_of(REMINDER_INTERVAL_DAYS)
}

/// The date by which the finished month's hours have to be handed in.
///
/// The organisation's own deadline day, from the general settings: it is what
/// the working agreement asks for, it exists whether or not a payroll report is
/// configured, and it is the date the reminders name. Should it ever be unset,
/// the payroll send day stands in — the report goes out just after midnight on
/// that day, so the day before is the last one that still counts.
async fn month_submission_deadline(
    pool: &DatabasePool,
    today: NaiveDate,
) -> Option<NaiveDate> {
    let configured: Option<u8> = load_setting(pool, SUBMISSION_DEADLINE_DAY_KEY, "")
        .await
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|day: &u8| (1..=28).contains(day));
    if let Some(day) = configured {
        return NaiveDate::from_ymd_opt(today.year(), today.month(), u32::from(day));
    }
    let config = crate::services::payroll_report::load_config(pool).await.ok()?;
    if !config.enabled || config.day_of_month <= 1 {
        return None;
    }
    NaiveDate::from_ymd_opt(
        today.year(),
        today.month(),
        u32::from(config.day_of_month) - 1,
    )
}

/// Every third day of the new month: ask the assistants who still hold days of
/// the finished month to hand them in, and name the date it has to happen by.
///
/// Assistants get their own pass because the week question does not apply to
/// them — no target schedule means a week without a booking is no evidence of
/// anything missing. What *is* evidence is a booking they made and never handed
/// in, so that is the trigger, re-checked on every pass. A day that is
/// submitted and merely waiting for a decision is not their move any more and
/// never produces a reminder.
///
/// It repeats on the same rhythm as [`run_month_weeks_reminder`] for as long as
/// the booking is not handed in, because that booking is exactly what holds the
/// payroll report back: an entry that exists and is not approved means hours
/// missing from the document. Asking once would leave the report blocked by
/// something nobody is being reminded of any more. It stops the moment they
/// hand it in — waiting for a decision is then the approver's move.
pub async fn run_month_end_check(
    state: &crate::AppState,
    now_local: chrono::DateTime<chrono_tz::Tz>,
) {
    let pool = &state.pool;
    if !month_reminder_is_due_now(now_local) {
        return;
    }
    let reminders_enabled = load_setting(pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true")
        .await
        .unwrap_or_else(|_| "true".to_string());
    if reminders_enabled == "false" {
        return;
    }

    let today = now_local.date_naive();
    let Some(deadline) = month_submission_deadline(pool, today).await else {
        return;
    };
    let language = crate::i18n::load_ui_language(pool)
        .await
        .unwrap_or_default();
    let period = crate::background::schedule::previous_period(today);
    let Ok((from, to)) = crate::background::schedule::period_bounds(&period) else {
        return;
    };
    let month_label = crate::i18n::format_month(&language, from.year(), from.month());
    let deadline_label = crate::i18n::format_date(&language, deadline);

    let user_ids = match state
        .db
        .reports
        .user_ids_with_unsubmitted_time_entries_in_range(from, to)
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(target:"zerf::submission_reminders", "month-end query failed: {e}");
            return;
        }
    };

    for user_id in user_ids {
        // Assistants only, and never an archived account or one with time
        // tracking switched off — those rows can only be settled by an admin.
        match state.db.users.find_by_id(user_id).await {
            Ok(Some(user))
                if user.active
                    && user.archived_at.is_none()
                    && user.tracks_time
                    && crate::roles::is_assistant_role(&user.role) => {}
            _ => continue,
        }
        let params = [
            ("month", month_label.clone()),
            ("deadline", deadline_label.clone()),
        ];
        let text = crate::i18n::notification_event_text(
            &language,
            "month_end_submission_reminder",
            &params,
        );
        let email_body = crate::i18n::notification_email_body(
            &language,
            "month_end_submission_reminder",
            &params,
        );
        // Per day: the pass repeats every third day while the booking is still
        // not handed in, and the loop re-checks every hour within the day.
        let dedupe_key = format!("month_end_submission_reminder:{today}");
        crate::services::notifications::deliver(
            state,
            &crate::services::notifications::Outgoing::new(
                user_id,
                &language,
                "month_end_submission_reminder",
                &text.title,
                &text.body,
            )
            .email_body(&email_body)
            .dedupe_key(&dedupe_key),
        )
        .await;
    }
}

/// Every third day from the 1st: name the weeks of the finished month that are
/// still missing.
///
/// Only for people with a target schedule. For them a week without a booking
/// *is* evidence of something missing, so the app can point at it; for an
/// assistant it is not, and guessing would only create pressure over a week
/// they may simply not have worked. This is the organisation's own interest in
/// a closed month — the payroll report does not depend on these weeks at all.
///
/// A week that is handed in and merely waiting for a decision counts as done:
/// the employee has nothing left to do with it, and chasing them for somebody
/// else's approval would be noise. The list is rebuilt on every pass, so it
/// shrinks as they work through it.
pub async fn run_month_weeks_reminder(
    state: &crate::AppState,
    now_local: chrono::DateTime<chrono_tz::Tz>,
) {
    let pool = &state.pool;
    if !month_reminder_is_due_now(now_local) {
        return;
    }
    let reminders_enabled = load_setting(pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true")
        .await
        .unwrap_or_else(|_| "true".to_string());
    if reminders_enabled == "false" {
        return;
    }

    let today = now_local.date_naive();
    let Some(deadline) = month_submission_deadline(pool, today).await else {
        return;
    };
    let language = crate::i18n::load_ui_language(pool)
        .await
        .unwrap_or_default();
    let period = crate::background::schedule::previous_period(today);
    let Ok((from, to)) = crate::background::schedule::period_bounds(&period) else {
        return;
    };
    let month_label = crate::i18n::format_month(&language, from.year(), from.month());
    let deadline_label = crate::i18n::format_date(&language, deadline);

    let rows = match state.db.users.get_active_non_assistant_users().await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(target:"zerf::submission_reminders", "week reminder user query failed: {e}");
            return;
        }
    };

    for crate::repository::ActiveUserRow {
        id: user_id,
        start_date: user_start,
        workdays_per_week,
        ..
    } in rows
    {
        let missing = crate::services::reports::unsubmitted_weeks_in_month(
            pool,
            user_id,
            from,
            to,
            user_start,
            workdays_per_week,
            today,
        )
        .await
        .unwrap_or_default();
        if missing.is_empty() {
            continue;
        }
        let labels: Vec<String> = missing
            .iter()
            .map(|monday| crate::i18n::format_week_label(&language, *monday))
            .collect();
        // Past the deadline the reminder keeps going — the weeks are still
        // owed — but it stops naming a date that has already gone by, and
        // falls back to the plain "these weeks are missing" message.
        let (kind, params) = if today <= deadline {
            (
                "month_weeks_reminder",
                vec![
                    ("month", month_label.clone()),
                    ("deadline", deadline_label.clone()),
                    ("weeks", labels.join(", ")),
                ],
            )
        } else {
            ("submission_reminder", vec![("weeks", labels.join(", "))])
        };
        let mut email_params = params.clone();
        if let Some(weeks) = email_params.iter_mut().find(|(key, _)| *key == "weeks") {
            weeks.1 = labels.join("\n");
        }
        let text = crate::i18n::notification_event_text(&language, kind, &params);
        let email_body = crate::i18n::notification_email_body(&language, kind, &email_params);
        // Per day: the loop wakes hourly, and the list must not be re-sent each
        // hour while somebody is working through it.
        let dedupe_key = format!("{kind}:{today}");
        crate::services::notifications::deliver(
            state,
            &crate::services::notifications::Outgoing::new(
                user_id,
                &language,
                kind,
                &text.title,
                &text.body,
            )
            .email_body(&email_body)
            .dedupe_key(&dedupe_key),
        )
        .await;
    }
}

/// Background loop: sleep until the next deadline day at 07:00 then run check.
/// Fixed to not miss deadline when restarting after 07:00 – we check due-first.
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
            // Due check first, so a restart after 07:00 on the deadline day still
            // fires. Running it on every hourly wake-up is safe because the
            // per-day dedupe key in `run_check` collapses the repeats into one
            // notification and one email.
            if deadline_is_due_now(Utc::now().with_timezone(&tz), d) {
                tracing::info!(target:"zerf::submission_reminders", "Running submission reminder check");
                run_check(&state).await;
            }
            // Independent of the configured deadline day: the finished month's
            // leftovers are chased on the 1st, because the monthly exports
            // depend on them. Safe to call on every hourly wake-up — the
            // per-period dedupe key collapses the repeats.
            let now_local = Utc::now().with_timezone(&tz);
            run_month_end_check(&state, now_local).await;
            run_month_weeks_reminder(&state, now_local).await;

            let wait = duration_until_next_deadline(Utc::now().with_timezone(&tz), d);
            let sleep_for = scheduler_sleep_duration(wait);
            tracing::info!(
                target:"zerf::submission_reminders",
                "Next submission reminder check scheduled in {:?}",
                wait
            );
            tokio::time::sleep(sleep_for).await;
        } else {
            // No deadline configured – poll every hour. The month-end pass is
            // not tied to that setting, so it still has to run.
            let timezone = load_setting(&pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
                .await
                .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
            let tz = timezone
                .parse::<chrono_tz::Tz>()
                .unwrap_or(chrono_tz::Europe::Berlin);
            let now_local = Utc::now().with_timezone(&tz);
            run_month_end_check(&state, now_local).await;
            run_month_weeks_reminder(&state, now_local).await;
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Berlin;

    /// Both month-boundary passes share one rhythm — every third day from the
    /// 1st, from 08:00. They chase exactly what holds the monthly reports up,
    /// so asking once would leave a report blocked by something nobody is
    /// being reminded of any more.
    #[test]
    fn month_reminders_repeat_every_third_day_from_the_first() {
        for day in [1, 4, 7, 10, 31] {
            assert!(
                month_reminder_is_due_now(Berlin.with_ymd_and_hms(2026, 8, day, 8, 0, 0).unwrap()),
                "expected a reminder on the {day}."
            );
        }
        for day in [2, 3, 5, 6, 8] {
            assert!(
                !month_reminder_is_due_now(Berlin.with_ymd_and_hms(2026, 8, day, 8, 0, 0).unwrap()),
                "expected no reminder on the {day}."
            );
        }
        // Before the working day starts, nothing goes out.
        assert!(!month_reminder_is_due_now(
            Berlin.with_ymd_and_hms(2026, 8, 4, 7, 59, 0).unwrap()
        ));
    }

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
    fn scheduler_sleep_caps_long_deadline_waits() {
        let now = Berlin.with_ymd_and_hms(2026, 5, 6, 8, 0, 0).unwrap();
        let wait = duration_until_next_deadline(now, 15);
        assert_eq!(
            scheduler_sleep_duration(wait),
            Duration::from_secs(3600),
            "long waits are capped so settings are reloaded regularly"
        );
    }

    #[test]
    fn scheduler_sleep_keeps_imminent_deadline_waits() {
        let now = Berlin.with_ymd_and_hms(2026, 5, 15, 6, 30, 0).unwrap();
        let wait = duration_until_next_deadline(now, 15);
        assert_eq!(
            scheduler_sleep_duration(wait).as_secs(),
            30 * 60,
            "imminent deadline should not be delayed by polling"
        );
    }

    #[test]
    fn deadline_due_recheck_requires_current_day_and_hour() {
        assert!(deadline_is_due_now(
            Berlin.with_ymd_and_hms(2026, 5, 15, 7, 0, 0).unwrap(),
            15
        ));
        assert!(!deadline_is_due_now(
            Berlin.with_ymd_and_hms(2026, 5, 15, 6, 59, 0).unwrap(),
            15
        ));
        assert!(!deadline_is_due_now(
            Berlin.with_ymd_and_hms(2026, 5, 15, 7, 0, 0).unwrap(),
            25
        ));
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
