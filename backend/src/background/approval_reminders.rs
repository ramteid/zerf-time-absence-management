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
            // Hour falls in DST gap; try one hour later
            let fallback = target_date.and_hms_opt(8, 0, 0).unwrap();
            match now.timezone().from_local_datetime(&fallback).earliest() {
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

/// Rows returned by the pending-approvals query:
/// (approver_id, approver_email, first_name, last_name, total_pending_count)
type PendingApproverRow = (i64, String, String, String, i64);

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

    for (approver_id, _email, _first, _last, pending_count) in approvers {
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

        // Idempotent per approver per local day. `deliver` owns the in-app row,
        // the SSE broadcast, the email, and its shared footer.
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

/// Background loop: sleep until the next Monday at 07:00 local time, then run check.
pub async fn run_loop(state: crate::AppState) {
    loop {
        let timezone = load_setting(&state.pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
            .await
            .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
        let tz = timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::Europe::Berlin);
        let wait = duration_until_next_monday_7am(Utc::now().with_timezone(&tz));
        let sleep_for = scheduler_sleep_duration(wait);
        tracing::info!(
            target:"zerf::approval_reminders",
            "Next approval reminder check scheduled in {:?}",
            wait
        );
        tokio::time::sleep(sleep_for).await;
        if wait > SETTINGS_POLL_INTERVAL {
            continue;
        }
        let current_timezone = load_setting(&state.pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
            .await
            .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
        let current_tz = current_timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::Europe::Berlin);
        if !approval_reminder_is_due_now(Utc::now().with_timezone(&current_tz)) {
            continue;
        }
        tracing::info!(target:"zerf::approval_reminders", "Running approval reminder check");
        run_check(&state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Europe::Berlin;

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
