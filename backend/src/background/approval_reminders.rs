//! Background task: every Monday at 07:00 local time, notify approvers who have
//! any pending approval requests (submitted weeks, absences, reopen requests).

use crate::db::DatabasePool;
use crate::services::settings::{
    app_today, load_setting, APPROVAL_REMINDERS_ENABLED_KEY, DEFAULT_TIMEZONE, TIMEZONE_KEY,
};
use chrono::{Datelike, Duration, TimeZone, Timelike, Utc};
use std::time::Duration as StdDuration;

const SETTINGS_POLL_INTERVAL: StdDuration = StdDuration::from_secs(3600);

/// Returns the duration to wait until the next Monday at 07:00 in the
/// configured application timezone.
/// If today is Monday and it is not yet 07:00, targets today.
pub fn duration_until_next_monday_7am(now: chrono::DateTime<chrono_tz::Tz>) -> StdDuration {
    let weekday = now.weekday().num_days_from_monday();
    let days_ahead = if weekday == 0 && now.hour() < 7 {
        0
    } else {
        7 - weekday
    };
    let target_date = now.date_naive() + Duration::days(i64::from(days_ahead));
    let target_naive = match target_date.and_hms_opt(7, 0, 0) {
        Some(n) => n,
        None => return StdDuration::from_secs(3600),
    };
    let target = match now.timezone().from_local_datetime(&target_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(earliest, _) => earliest,
        chrono::LocalResult::None => {
            // Hour falls in DST gap; try later hours up to 23, like submission_reminders does.
            let mut resolved = None;
            for hour in 8..=23 {
                if let Some(naive) = target_date.and_hms_opt(hour, 0, 0) {
                    if let Some(dt) = now.timezone().from_local_datetime(&naive).earliest() {
                        resolved = Some(dt);
                        break;
                    }
                }
            }
            match resolved {
                Some(dt) => dt,
                None => return StdDuration::from_secs(3600),
            }
        }
    };
    (target - now)
        .to_std()
        .unwrap_or(StdDuration::from_secs(60))
}

pub fn scheduler_sleep_duration(deadline_wait: StdDuration) -> StdDuration {
    deadline_wait.min(SETTINGS_POLL_INTERVAL)
}

fn approval_reminder_is_due_now(now: chrono::DateTime<chrono_tz::Tz>) -> bool {
    now.weekday().num_days_from_monday() == 0 && now.hour() >= 7
}

/// Rows returned by the pending-approvals query: (approver_id, total_pending_count)
type PendingApproverRow = (i64, i64);

/// Query all active approvers who currently have at least one pending item.
/// Uses explicit approver assignments only.
async fn find_approvers_with_pending(pool: &DatabasePool) -> Vec<PendingApproverRow> {
    crate::repository::UserDb::new(pool.clone())
        .pending_approvers_for_reminders()
        .await
        .unwrap_or_default()
}

/// Run one check pass: notify every approver who has pending items.
pub async fn run_check(state: &crate::AppState) {
    let pool = &state.pool;

    let reminders_enabled = load_setting(pool, APPROVAL_REMINDERS_ENABLED_KEY, "true")
        .await
        .unwrap_or_else(|_| "true".to_string());
    if reminders_enabled == "false" {
        tracing::debug!(target:"zerf::approval_reminders", "Reminders are disabled, skipping check");
        return;
    }

    let language = match crate::i18n::load_ui_language(pool).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(target:"zerf::approval_reminders", "load language failed: {e}");
            crate::i18n::Language::default()
        }
    };

    let today_local = app_today(pool).await;

    let approvers = find_approvers_with_pending(pool).await;
    if approvers.is_empty() {
        tracing::debug!(target:"zerf::approval_reminders", "No pending approvals found, skipping");
        return;
    }

    for (approver_id, pending_count) in approvers {
        let count_str = pending_count.to_string();
        let text = crate::i18n::notification_event_text(
            &language,
            "approval_reminder",
            &[("count", count_str.clone())],
        );
        let email_body = crate::i18n::notification_email_body(
            &language,
            "approval_reminder",
            &[("count", count_str)],
        );

        // Idempotent per approver per local day. The uniqueness index is
        // (user_id, kind, dedupe_key), so the recipient is already part of the
        // key — and the count must stay out of it, or every approval during the
        // day would mint a new key and re-send the reminder.
        let dedupe_key = format!("approval_reminder:{}", today_local);
        crate::services::notifications::deliver(
            state,
            &crate::services::notifications::Outgoing::new(
                approver_id,
                &language,
                "approval_reminder",
                &text.title,
                &text.body,
            )
            .email_body(&email_body)
            .dedupe_key(&dedupe_key),
        )
        .await;
    }
}

/// Days between the month-boundary reminders: the 1st, the 4th, the 7th and so
/// on — the same rhythm the submission side uses.
const REMINDER_INTERVAL_DAYS: u32 = 3;

/// True on every third day from the 1st, from 08:00 local time.
///
/// It repeats for the same reason the submission side does: an undecided day
/// of the finished month holds the payroll report back, and a reminder that
/// fires once would leave the report blocked by something nobody is being
/// asked about any more.
fn month_end_reminder_is_due_now(now: chrono::DateTime<chrono_tz::Tz>) -> bool {
    now.hour() >= 8
        && (now.date_naive().day() - 1).is_multiple_of(REMINDER_INTERVAL_DAYS)
}

/// Ask approvers for the finished month specifically.
///
/// The weekly reminder above is not enough for a month boundary: it fires on
/// Mondays, and the days that decide whether the monthly exports can go out are
/// handed in on the 1st — which can be a Tuesday, leaving the decision sitting
/// until after the payroll report was due. This pass names the month, and
/// repeats every third day for as long as something in it is still undecided.
pub async fn run_month_end_check(state: &crate::AppState) {
    let pool = &state.pool;

    let reminders_enabled = load_setting(pool, APPROVAL_REMINDERS_ENABLED_KEY, "true")
        .await
        .unwrap_or_else(|_| "true".to_string());
    if reminders_enabled == "false" {
        return;
    }

    let language = crate::i18n::load_ui_language(pool)
        .await
        .unwrap_or_default();
    let today = app_today(pool).await;
    let period = crate::background::schedule::previous_period(today);
    let Ok((from, to)) = crate::background::schedule::period_bounds(&period) else {
        return;
    };
    let month_label = crate::i18n::format_month(&language, from.year(), from.month());

    let waiting_user_ids = match state
        .db
        .reports
        .user_ids_with_submitted_time_entries_in_range(from, to)
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(target:"zerf::approval_reminders", "month-end query failed: {e}");
            return;
        }
    };

    // An undecided absence request holds the payroll report back exactly like
    // an undecided week does, and the same person can settle both, so this pass
    // asks about both together.
    let mut waiting: std::collections::HashSet<i64> = waiting_user_ids.into_iter().collect();
    match state
        .db
        .reports
        .user_ids_with_requested_absences_in_range(from, to)
        .await
    {
        Ok(ids) => waiting.extend(ids),
        Err(e) => {
            tracing::warn!(target:"zerf::approval_reminders", "month-end absence query failed: {e}");
            return;
        }
    }

    // One reminder per approver, carrying how many of their people are waiting.
    let mut waiting_per_approver: std::collections::HashMap<i64, usize> =
        std::collections::HashMap::new();
    for user_id in waiting {
        for approver_id in crate::services::auth::user_approver_ids(pool, user_id).await {
            *waiting_per_approver.entry(approver_id).or_default() += 1;
        }
    }

    for (approver_id, waiting) in waiting_per_approver {
        let count = waiting.to_string();
        let params = [("month", month_label.clone()), ("count", count)];
        let text =
            crate::i18n::notification_event_text(&language, "month_end_approval_reminder", &params);
        let email_body =
            crate::i18n::notification_email_body(&language, "month_end_approval_reminder", &params);
        // Per day per approver: the pass repeats every third day while
        // decisions are outstanding, and the count must stay out of the key, or
        // approving one person would re-send the reminder for the rest.
        let dedupe_key = format!("month_end_approval_reminder:{today}");
        crate::services::notifications::deliver(
            state,
            &crate::services::notifications::Outgoing::new(
                approver_id,
                &language,
                "month_end_approval_reminder",
                &text.title,
                &text.body,
            )
            .email_body(&email_body)
            .dedupe_key(&dedupe_key),
        )
        .await;
    }
}

/// Background loop: sleep until the next Monday at 07:00 local time, then run check.
/// Fixed due-first so restart after 07:00 on Monday still fires.
pub async fn run_loop(state: crate::AppState) {
    loop {
        let timezone = load_setting(&state.pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
            .await
            .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
        let tz = timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::Europe::Berlin);
        let now_local = Utc::now().with_timezone(&tz);
        if approval_reminder_is_due_now(now_local) {
            tracing::info!(target:"zerf::approval_reminders", "Running approval reminder check");
            run_check(&state).await;
        }
        // The month-end pass rides the same hourly wake-up (the sleep below is
        // capped at one hour), so it fires on its own day regardless of which
        // weekday that is. Repeats within the day are collapsed by the
        // per-period dedupe key.
        if month_end_reminder_is_due_now(now_local) {
            tracing::info!(target:"zerf::approval_reminders", "Running month-end approval reminder check");
            run_month_end_check(&state).await;
        }
        let wait = duration_until_next_monday_7am(Utc::now().with_timezone(&tz));
        let sleep_for = scheduler_sleep_duration(wait);
        tracing::info!(
            target:"zerf::approval_reminders",
            "Next approval reminder check scheduled in {:?}",
            wait
        );
        tokio::time::sleep(sleep_for).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Europe::Berlin;

    /// Approvers are asked on the same rhythm as the people who hand things in:
    /// every third day from the 1st, from 08:00, on whatever weekday that is.
    #[test]
    fn month_end_reminder_repeats_every_third_day() {
        for day in [1, 4, 7, 10] {
            assert!(
                month_end_reminder_is_due_now(
                    Berlin.with_ymd_and_hms(2026, 9, day, 8, 0, 0).unwrap()
                ),
                "expected a reminder on the {day}."
            );
        }
        for day in [2, 3, 5] {
            assert!(
                !month_end_reminder_is_due_now(
                    Berlin.with_ymd_and_hms(2026, 9, day, 8, 0, 0).unwrap()
                ),
                "expected no reminder on the {day}."
            );
        }
        assert!(!month_end_reminder_is_due_now(
            Berlin.with_ymd_and_hms(2026, 9, 4, 7, 0, 0).unwrap()
        ));
    }

    #[test]
    fn monday_before_7am_targets_today() {
        // Monday 2026-05-04 06:00 → should target the same day at 07:00
        let now = Berlin.with_ymd_and_hms(2026, 5, 4, 6, 0, 0).unwrap();
        let wait = duration_until_next_monday_7am(now);
        let secs = wait.as_secs();
        assert!((3500..=3700).contains(&secs), "expected ~1h, got {secs}s");
    }

    #[test]
    fn scheduler_sleep_caps_long_approval_waits() {
        let now = Berlin.with_ymd_and_hms(2026, 5, 6, 12, 0, 0).unwrap();
        let wait = duration_until_next_monday_7am(now);
        assert_eq!(
            scheduler_sleep_duration(wait),
            StdDuration::from_secs(3600),
            "long waits are capped so timezone settings are reloaded regularly"
        );
    }

    #[test]
    fn scheduler_sleep_keeps_imminent_approval_waits() {
        let now = Berlin.with_ymd_and_hms(2026, 5, 4, 6, 30, 0).unwrap();
        let wait = duration_until_next_monday_7am(now);
        assert_eq!(scheduler_sleep_duration(wait).as_secs(), 30 * 60);
    }

    #[test]
    fn approval_due_recheck_requires_monday_after_7am() {
        assert!(approval_reminder_is_due_now(
            Berlin.with_ymd_and_hms(2026, 5, 4, 7, 0, 0).unwrap()
        ));
        assert!(!approval_reminder_is_due_now(
            Berlin.with_ymd_and_hms(2026, 5, 4, 6, 59, 0).unwrap()
        ));
        assert!(!approval_reminder_is_due_now(
            Berlin.with_ymd_and_hms(2026, 5, 5, 7, 0, 0).unwrap()
        ));
    }

    #[test]
    fn monday_after_7am_schedules_next_week() {
        // Monday 2026-05-04 08:00 → next Monday 2026-05-11 07:00 = ~6 days 23 h
        let now = Berlin.with_ymd_and_hms(2026, 5, 4, 8, 0, 0).unwrap();
        let wait = duration_until_next_monday_7am(now);
        let secs = wait.as_secs();
        assert!(secs > 6 * 86400, "should be >6 days, got {secs}s");
        assert!(secs < 7 * 86400, "should be <7 days, got {secs}s");
    }

    #[test]
    fn mid_week_schedules_next_monday() {
        // Wednesday 2026-05-06 12:00 → next Monday 2026-05-11 07:00 = ~4 days 19 h
        let now = Berlin.with_ymd_and_hms(2026, 5, 6, 12, 0, 0).unwrap();
        let wait = duration_until_next_monday_7am(now);
        let secs = wait.as_secs();
        assert!(secs > 4 * 86400, "should be >4 days, got {secs}s");
        assert!(secs < 5 * 86400, "should be <5 days, got {secs}s");
    }

    #[test]
    fn sunday_schedules_next_monday() {
        // Sunday 2026-05-10 20:00 → next Monday 2026-05-11 07:00 = ~11 h
        let now = Berlin.with_ymd_and_hms(2026, 5, 10, 20, 0, 0).unwrap();
        let wait = duration_until_next_monday_7am(now);
        let secs = wait.as_secs();
        assert!(secs > 10 * 3600, "should be >10h, got {secs}s");
        assert!(secs < 12 * 3600, "should be <12h, got {secs}s");
    }
}
