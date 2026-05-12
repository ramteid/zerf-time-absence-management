//! Background task: every Monday at 07:00 local time, notify approvers who have
//! any pending approval requests (change requests, absences, reopen requests).

use crate::db::DatabasePool;
use chrono::{Datelike, Duration, TimeZone, Timelike, Utc};
use std::time::Duration as StdDuration;

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
    (target - now).to_std().unwrap_or(StdDuration::from_secs(60))
}

/// Rows returned by the pending-approvals query:
/// (approver_id, approver_email, total_pending_count)
type PendingApproverRow = (i64, String, i64);

/// Query all active approvers who currently have at least one pending item.
/// Uses explicit approver assignments only.
async fn find_approvers_with_pending(pool: &DatabasePool) -> Vec<PendingApproverRow> {
    sqlx::query_as::<_, PendingApproverRow>(
        "WITH user_pending AS (
             SELECT user_id, COUNT(*)::bigint AS pending_count
             FROM (
                 -- Include all submitted entries: non-crediting entries also require
                 -- approval, so approvers must be reminded about them too.
                 SELECT user_id FROM time_entries
                 WHERE status = 'submitted'
                 UNION ALL
                 SELECT user_id FROM change_requests    WHERE status = 'open'
                 UNION ALL
                 SELECT user_id FROM absences           WHERE status IN ('requested','cancellation_pending')
                 UNION ALL
                 SELECT user_id FROM reopen_requests    WHERE status = 'pending'
             ) all_pending
             GROUP BY user_id
         ),
         -- Only count an assignment as active when the approver is active and
         -- role-eligible for the subject user.
         via_assignment AS (
             SELECT ua.approver_id, SUM(up.pending_count)::bigint AS pending_count
             FROM user_approvers ua
             JOIN user_pending up ON up.user_id = ua.user_id
             JOIN users subject   ON subject.id = ua.user_id
             JOIN users approver  ON approver.id = ua.approver_id
                                 AND approver.active = TRUE
             WHERE (
                 (subject.role = 'admin' AND approver.role = 'admin') OR
                 (subject.role != 'admin' AND approver.role IN ('team_lead', 'admin'))
             )
             GROUP BY ua.approver_id
         ),
         combined AS (
             SELECT approver_id, pending_count FROM via_assignment
         )
         SELECT c.approver_id, u.email, SUM(c.pending_count)::bigint AS total_pending
         FROM combined c
         JOIN users u ON u.id = c.approver_id AND u.active = TRUE
         GROUP BY c.approver_id, u.email
         HAVING SUM(c.pending_count) > 0",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Run one check pass: notify every approver who has pending items.
pub async fn run_check(state: &crate::AppState) {
    let pool = &state.pool;

    let reminders_enabled = crate::settings::load_setting(
        pool,
        crate::settings::APPROVAL_REMINDERS_ENABLED_KEY,
        "true",
    )
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

    let app_url = state
        .cfg
        .public_url
        .clone()
        .unwrap_or_else(|| "http://localhost".to_string());
    let timezone = crate::settings::load_setting(
        pool,
        crate::settings::TIMEZONE_KEY,
        crate::settings::DEFAULT_TIMEZONE,
    )
    .await
    .unwrap_or_else(|_| crate::settings::DEFAULT_TIMEZONE.to_string());
    let today_local = crate::settings::app_today(pool).await;

    let approvers = find_approvers_with_pending(pool).await;
    if approvers.is_empty() {
        tracing::debug!(target:"zerf::approval_reminders", "No pending approvals found, skipping");
        return;
    }

    let smtp = crate::settings::load_smtp_config(pool)
        .await
        .map(std::sync::Arc::new);

    for (approver_id, approver_email, pending_count) in approvers {
        let count_str = pending_count.to_string();
        let title = crate::i18n::translate(&language, "approval_reminder_title", &[]);
        let body = crate::i18n::translate(
            &language,
            "approval_reminder_body",
            &[("count", count_str.clone())],
        );
        let timestamp = crate::i18n::format_datetime_in_timezone(&language, chrono::Utc::now(), &timezone);
        let email_body = format!(
            "{}\n\n{}",
            crate::i18n::translate(
                &language,
                "approval_reminder_email_body",
                &[("count", count_str), ("app_url", app_url.clone())],
            ),
            timestamp,
        );

        let dedupe_key = format!("approval_reminder:{}", today_local);
        match state
            .db
            .notifications
            .insert_idempotent_with_dedupe_key(
                approver_id,
                "approval_reminder",
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
                    .send(crate::notifications::NotificationSignal { user_id: approver_id });
                crate::email::send_async(smtp.clone(), approver_email, title, email_body);
            }
            Ok(false) => {
                // Already reminded today (idempotency guard).
            }
            Err(e) => {
                tracing::warn!(
                    target:"zerf::approval_reminders",
                    "insert notification failed for approver {approver_id}: {e}"
                );
            }
        }
    }
}

/// Background loop: sleep until the next Monday at 07:00 local time, then run check.
pub async fn run_loop(state: crate::AppState) {
    loop {
        let timezone = crate::settings::load_setting(
            &state.pool,
            crate::settings::TIMEZONE_KEY,
            crate::settings::DEFAULT_TIMEZONE,
        )
        .await
        .unwrap_or_else(|_| crate::settings::DEFAULT_TIMEZONE.to_string());
        let tz = timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::Europe::Berlin);
        let wait = duration_until_next_monday_7am(Utc::now().with_timezone(&tz));
        tracing::info!(
            target:"zerf::approval_reminders",
            "Next approval reminder check scheduled in {:?}",
            wait
        );
        tokio::time::sleep(wait).await;
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
        assert!(secs >= 3500 && secs <= 3700, "expected ~1h, got {secs}s");
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
