//! Background task: check on the configured deadline day of each month
//! whether users have submitted all past months' time entries.
//! Users with weekly_hours = 0 are skipped (non-booking users).

use crate::db::DatabasePool;

use crate::settings::load_setting;
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use std::time::Duration;

const SUBMISSION_DEADLINE_DAY_KEY: &str = "submission_deadline_day";

/// Returns the duration to wait until the next occurrence of `day_of_month` at 07:00 local time.
pub fn duration_until_next_deadline(
    now: chrono::DateTime<chrono::Local>,
    day_of_month: u8,
) -> Duration {
    let day = day_of_month as u32;
    let today = now.date_naive();

    // Try this month's deadline day
    let candidate_day = day.min(last_day_of_month(today.year(), today.month()));
    let candidate = NaiveDate::from_ymd_opt(today.year(), today.month(), candidate_day).unwrap();

    if let Some(target) = resolve_local_datetime(candidate, 7) {
        if target > now {
            return (target - now).to_std().unwrap_or(Duration::from_secs(60));
        }
    }

    // Already past or ambiguous – schedule next month
    let next = advance_one_month(today, day);
    let t2 = resolve_local_datetime(next, 7).expect("next month datetime must resolve");
    (t2 - now).to_std().unwrap_or(Duration::from_secs(60))
}

/// Resolve a naive date + hour to a local datetime, handling DST gaps/ambiguities.
fn resolve_local_datetime(date: NaiveDate, hour: u32) -> Option<chrono::DateTime<chrono::Local>> {
    let naive = date.and_hms_opt(hour, 0, 0)?;
    match chrono::Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt),
        chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest),
        chrono::LocalResult::None => {
            // Hour falls in a DST gap; try one hour later
            let fallback = date.and_hms_opt(hour + 1, 0, 0)?;
            chrono::Local.from_local_datetime(&fallback).earliest()
        }
    }
}

fn advance_one_month(d: NaiveDate, desired_day: u32) -> NaiveDate {
    let (year, month) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    let actual_day = desired_day.min(last_day_of_month(year, month));
    NaiveDate::from_ymd_opt(year, month, actual_day).unwrap()
}

pub fn last_day_of_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next_month.unwrap().pred_opt().unwrap().day()
}

/// Collect (year, month) pairs where the user has unsubmitted time entries,
/// from their start_date through the last fully completed month.
async fn find_unsubmitted_months(
    pool: &DatabasePool,
    user_id: i64,
    user_start: NaiveDate,
    last_year: i32,
    last_month: u32,
) -> Vec<(i32, u32)> {
    // Single query: for each month with any entries, check if any are still draft.
    // Months with zero entries are also "unsubmitted" and handled separately.
    let rows: Vec<(i32, i32, i64, i64)> = sqlx::query_as(
        "SELECT \
             EXTRACT(YEAR FROM entry_date)::int AS y, \
             EXTRACT(MONTH FROM entry_date)::int AS m, \
             COUNT(*) AS total, \
             COUNT(*) FILTER (WHERE status = 'draft') AS drafts \
         FROM time_entries \
         WHERE user_id = $1 \
           AND entry_date >= $2 \
           AND entry_date < $3 \
         GROUP BY y, m",
    )
    .bind(user_id)
    .bind(user_start)
    .bind(
        NaiveDate::from_ymd_opt(
            if last_month == 12 {
                last_year + 1
            } else {
                last_year
            },
            if last_month == 12 { 1 } else { last_month + 1 },
            1,
        )
        .unwrap(),
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Build a set of months that have all entries submitted (total > 0 && drafts == 0)
    let submitted: std::collections::HashSet<(i32, u32)> = rows
        .into_iter()
        .filter(|(_, _, total, drafts)| *total > 0 && *drafts == 0)
        .map(|(y, m, _, _)| (y, m as u32))
        .collect();

    // Iterate all months from start to last completed month
    let mut missing = Vec::new();
    let mut y = user_start.year();
    let mut m = user_start.month();
    while y < last_year || (y == last_year && m <= last_month) {
        if !submitted.contains(&(y, m)) {
            missing.push((y, m));
        }
        if m == 12 {
            m = 1;
            y += 1;
        } else {
            m += 1;
        }
    }

    missing
}

/// Run one check pass for all active users with weekly_hours > 0.
pub async fn run_check(state: &crate::AppState) {
    let pool = &state.pool;

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

    let today = Local::now().date_naive();
    // Last fully completed month
    let (last_year, last_month) = if today.month() == 1 {
        (today.year() - 1, 12u32)
    } else {
        (today.year(), today.month() - 1)
    };

    let rows: Vec<(i64, String, NaiveDate)> = match sqlx::query_as::<_, (i64, String, NaiveDate)>(
        "SELECT id, email, start_date FROM users WHERE active = TRUE AND weekly_hours > 0",
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target:"zerf::submission_reminders", "fetch users failed: {e}");
            return;
        }
    };

    // Load SMTP config once for all users
    let smtp = crate::settings::load_smtp_config(pool)
        .await
        .map(std::sync::Arc::new);

    for (user_id, user_email, user_start) in rows {
        let missing =
            find_unsubmitted_months(pool, user_id, user_start, last_year, last_month).await;

        if missing.is_empty() {
            continue;
        }

        // Check for duplicate: skip if a submission_reminder was already sent today
        let already_sent: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM notifications \
                 WHERE user_id = $1 AND kind = 'submission_reminder' \
                   AND created_at::date = CURRENT_DATE\
             )",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(false);

        if already_sent {
            continue;
        }

        let missing_months: Vec<String> = missing
            .iter()
            .map(|(y, m)| crate::i18n::format_month(&language, *y, *m))
            .collect();

        let months_str = missing_months.join(", ");
        let title = crate::i18n::translate(&language, "submission_reminder_title", &[]);
        let body = crate::i18n::translate(
            &language,
            "submission_reminder_body",
            &[("months", months_str.clone())],
        );
        let email_body = crate::i18n::translate(
            &language,
            "submission_reminder_email_body",
            &[
                ("months", missing_months.join("\n")),
                ("app_url", app_url.clone()),
            ],
        );

        // Insert in-app notification
        match sqlx::query(
            "INSERT INTO notifications(user_id,kind,title,body,reference_type,reference_id) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(user_id)
        .bind("submission_reminder")
        .bind(&title)
        .bind(&body)
        .bind(Option::<String>::None)
        .bind(Option::<i64>::None)
        .execute(pool)
        .await
        {
            Ok(_) => {
                let _ = state
                    .notifications
                    .send(crate::notifications::NotificationSignal { user_id });
            }
            Err(e) => {
                tracing::warn!(
                    target:"zerf::submission_reminders",
                    "insert notification failed for user {user_id}: {e}"
                );
            }
        }

        // Send email best-effort
        crate::email::send_async(smtp.clone(), user_email, title, email_body);
    }
}

/// Background loop: sleep until the next deadline day at 07:00 then run check.
pub async fn run_loop(pool: DatabasePool, state: crate::AppState) {
    loop {
        let day_str = load_setting(&pool, SUBMISSION_DEADLINE_DAY_KEY, "").await.unwrap_or_default();
        let day: Option<u8> = day_str.parse().ok().filter(|&d: &u8| (1..=28).contains(&d));

        if let Some(d) = day {
            let wait = duration_until_next_deadline(Local::now(), d);
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

    #[test]
    fn deadline_in_future_same_month() {
        // 2026-05-06 08:00 local, deadline day 15 -> should wait until 15th at 07:00
        let now = chrono::Local.with_ymd_and_hms(2026, 5, 6, 8, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 15);
        // Should be ~8 days 23 hours = 8*86400 + 23*3600 = 774000 seconds
        let secs = dur.as_secs();
        assert!(secs > 7 * 86400, "should be more than 7 days, got {secs}");
        assert!(secs < 10 * 86400, "should be less than 10 days, got {secs}");
    }

    #[test]
    fn deadline_today_but_not_yet() {
        // 2026-05-15 06:00 local, deadline day 15 -> should wait ~1 hour
        let now = chrono::Local
            .with_ymd_and_hms(2026, 5, 15, 6, 0, 0)
            .unwrap();
        let dur = duration_until_next_deadline(now, 15);
        let secs = dur.as_secs();
        assert!(secs >= 3500, "should be about 1 hour, got {secs}");
        assert!(secs <= 3700, "should be about 1 hour, got {secs}");
    }

    #[test]
    fn deadline_already_passed_schedules_next_month() {
        // 2026-05-15 08:00 local, deadline day 10 -> next: June 10 at 07:00
        let now = chrono::Local
            .with_ymd_and_hms(2026, 5, 15, 8, 0, 0)
            .unwrap();
        let dur = duration_until_next_deadline(now, 10);
        let secs = dur.as_secs();
        // ~25.96 days
        assert!(secs > 24 * 86400, "should be >24 days, got {secs}");
        assert!(secs < 27 * 86400, "should be <27 days, got {secs}");
    }

    #[test]
    fn deadline_day_clamped_to_month_end() {
        // Feb 2026: 28 days. Deadline day 28 on Feb 1 -> should target Feb 28
        let now = chrono::Local.with_ymd_and_hms(2026, 2, 1, 6, 0, 0).unwrap();
        let dur = duration_until_next_deadline(now, 28);
        let secs = dur.as_secs();
        // ~27 days + 1 hour
        assert!(secs > 26 * 86400, "should be >26 days, got {secs}");
        assert!(secs < 28 * 86400, "should be <28 days, got {secs}");
    }

    #[test]
    fn deadline_december_wraps_to_january() {
        // 2026-12-20 08:00, deadline day 5 -> next: Jan 5, 2027 at 07:00
        let now = chrono::Local
            .with_ymd_and_hms(2026, 12, 20, 8, 0, 0)
            .unwrap();
        let dur = duration_until_next_deadline(now, 5);
        let secs = dur.as_secs();
        // ~15.96 days
        assert!(secs > 14 * 86400, "should be >14 days, got {secs}");
        assert!(secs < 17 * 86400, "should be <17 days, got {secs}");
    }

    #[test]
    fn last_day_of_month_february_leap_year() {
        assert_eq!(last_day_of_month(2024, 2), 29);
        assert_eq!(last_day_of_month(2025, 2), 28);
    }

    #[test]
    fn last_day_of_month_various() {
        assert_eq!(last_day_of_month(2026, 1), 31);
        assert_eq!(last_day_of_month(2026, 4), 30);
        assert_eq!(last_day_of_month(2026, 12), 31);
    }

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
}
