//! Background task: check on the configured deadline day of each month
//! whether users have submitted all past months' time entries.
//! Users with weekly_hours = 0 are skipped (non-booking users).

use crate::db::DatabasePool;
use crate::i18n::{Language, TextKey};
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
    let target = candidate
        .and_hms_opt(7, 0, 0)
        .and_then(|dt| chrono::Local.from_local_datetime(&dt).single())
        .expect("valid datetime");

    if target > now {
        return (target - now).to_std().unwrap_or(Duration::from_secs(60));
    }

    // Already past – schedule next month
    let next = advance_one_month(today, day);
    let t2 = next
        .and_hms_opt(7, 0, 0)
        .and_then(|dt| chrono::Local.from_local_datetime(&dt).single())
        .expect("valid datetime");
    (t2 - now).to_std().unwrap_or(Duration::from_secs(60))
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

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next_month.unwrap().pred_opt().unwrap().day()
}

fn format_month(language: Language, year: i32, month: u32) -> String {
    match language {
        Language::De => {
            let name = match month {
                1 => "Januar", 2 => "Februar", 3 => "März", 4 => "April",
                5 => "Mai", 6 => "Juni", 7 => "Juli", 8 => "August",
                9 => "September", 10 => "Oktober", 11 => "November", 12 => "Dezember",
                _ => "?",
            };
            format!("{name} {year}")
        }
        Language::En => {
            let name = match month {
                1 => "January", 2 => "February", 3 => "March", 4 => "April",
                5 => "May", 6 => "June", 7 => "July", 8 => "August",
                9 => "September", 10 => "October", 11 => "November", 12 => "December",
                _ => "?",
            };
            format!("{name} {year}")
        }
    }
}

/// Run one check pass for all active users with weekly_hours > 0.
pub async fn run_check(state: &crate::AppState) {
    let pool = &state.pool;

    let language = match crate::i18n::load_ui_language(pool).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(target:"zerf::submission_reminders", "load language failed: {e}");
            Language::default()
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

    for (user_id, user_email, user_start) in rows {
        let mut missing_months: Vec<String> = Vec::new();

        let mut y = user_start.year();
        let mut m = user_start.month();

        loop {
            if y > last_year || (y == last_year && m > last_month) {
                break;
            }

            let month_start = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
            let month_end = NaiveDate::from_ymd_opt(y, m, last_day_of_month(y, m)).unwrap();

            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM time_entries WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3",
            )
            .bind(user_id)
            .bind(month_start)
            .bind(month_end)
            .fetch_one(pool)
            .await
            .unwrap_or(0);

            let draft: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM time_entries WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 AND status = 'draft'",
            )
            .bind(user_id)
            .bind(month_start)
            .bind(month_end)
            .fetch_one(pool)
            .await
            .unwrap_or(0);

            // Not submitted: either all entries are draft, or there are no entries at all
            if total == 0 || draft > 0 {
                missing_months.push(format_month(language, y, m));
            }

            if m == 12 { m = 1; y += 1; } else { m += 1; }
        }

        if missing_months.is_empty() {
            continue;
        }

        let months_str = missing_months.join(", ");
        let title = crate::i18n::translate(language, TextKey::SubmissionReminderTitle, &[]);
        let body = crate::i18n::translate(
            language,
            TextKey::SubmissionReminderBody,
            &[("months", months_str.clone())],
        );
        let email_body = crate::i18n::translate(
            language,
            TextKey::SubmissionReminderEmailBody,
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
                let _ = state.notifications.send(
                    crate::notifications::NotificationSignal { user_id },
                );
            }
            Err(e) => {
                tracing::warn!(
                    target:"zerf::submission_reminders",
                    "insert notification failed for user {user_id}: {e}"
                );
            }
        }

        // Send email best-effort
        let smtp = crate::settings::load_smtp_config(pool)
            .await
            .map(std::sync::Arc::new);
        crate::email::send_async(smtp, user_email, title, email_body);
    }
}

/// Background loop: sleep until the next deadline day at 07:00 then run check.
pub async fn run_loop(pool: DatabasePool, state: crate::AppState) {
    loop {
        let day_str = load_setting(&pool, SUBMISSION_DEADLINE_DAY_KEY, "")
            .await
            .unwrap_or_default();
        let day: Option<u8> = day_str.parse().ok().filter(|&d: &u8| d >= 1 && d <= 28);

        if let Some(d) = day {
            let now = Local::now();
            let wait = duration_until_next_deadline(now, d);
            tracing::info!(
                target:"zerf::submission_reminders",
                "Next submission reminder check scheduled in {:?}",
                wait
            );
            tokio::time::sleep(wait).await;

            // Re-read to confirm setting still active
            let day_str2 = load_setting(&pool, SUBMISSION_DEADLINE_DAY_KEY, "")
                .await
                .unwrap_or_default();
            if day_str2.parse::<u8>().ok().filter(|&d2| d2 >= 1 && d2 <= 28).is_some() {
                tracing::info!(target:"zerf::submission_reminders", "Running submission reminder check");
                run_check(&state).await;
            }
        } else {
            // No deadline configured – poll every hour
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
}
