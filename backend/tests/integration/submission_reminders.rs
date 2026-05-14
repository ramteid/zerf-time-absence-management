//! End-to-end submission reminder tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use chrono::Datelike;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

/// Helper: create a time entry for a past date (draft status by default).
async fn create_draft_entry(client: &crate::common::TestClient, date: &str, cat_id: i64) -> i64 {
    let (st, body) = client
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": date,
                "start_time": "08:00",
                "end_time": "16:30",
                "category_id": cat_id,
                "comment": ""
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create draft entry for {date}");
    id(&body)
}

#[tokio::test]
async fn submission_reminders_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Reminder creates notification for unsubmitted months --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "1").await;
        let emp = login_change_pw(&app, "emp-1@example.com", &emp_pw).await;

        let (st, _) = emp.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::submission_reminders::run_check(&app.state).await;

        let (st, body) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let notifications = body.as_array().expect("notifications array");
        let reminder = notifications
            .iter()
            .find(|n| n["kind"] == "submission_reminder");
        assert!(reminder.is_some(), "should receive submission_reminder");

        let reminder = reminder.unwrap();
        assert!(!reminder["body"].as_str().unwrap_or("").is_empty());
    }

    // -- Reminder skips user with all submitted --
    //
    // Create a user whose start date is last week's Monday so there is exactly
    // one fully elapsed past week.  Submit entries for all 5 contract workdays
    // of that week so the reminder check finds nothing incomplete.
    {
        let today = chrono::Local::now().date_naive();
        let last_week_monday =
            today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64 + 7);
        let start_date = last_week_monday.format("%Y-%m-%d").to_string();

        let (_, body) = admin.get("/api/v1/categories").await;
        let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "recent@example.com",
                    "first_name": "Recent",
                    "last_name": "User",
                    "role": "employee",
                    "weekly_hours": 20,
                    "leave_days_current_year": 10,
                    "leave_days_next_year": 10,
                    "start_date": start_date,
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        let emp_pw = temp_pw(&body);

        let emp = login_change_pw(&app, "recent@example.com", &emp_pw).await;

        // Submit entries for all 5 workdays (Mon-Fri) of last week.
        let mut entry_ids = Vec::new();
        for day_offset in 0..5 {
            let day = (last_week_monday + chrono::Duration::days(day_offset))
                .format("%Y-%m-%d")
                .to_string();
            let eid = create_draft_entry(&emp, &day, cat_id).await;
            entry_ids.push(eid);
        }
        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": entry_ids}))
            .await;
        assert_eq!(st, StatusCode::OK);

        let (st, _) = emp.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::submission_reminders::run_check(&app.state).await;

        let (st, body) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let notifications = body.as_array().expect("notifications array");
        let reminder = notifications
            .iter()
            .find(|n| n["kind"] == "submission_reminder");
        assert!(reminder.is_none(), "no reminder for submitted user");
    }

    // -- Reminder deduplicates on same day --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;
        let emp = login_change_pw(&app, "emp-2@example.com", &emp_pw).await;

        let (st, _) = emp.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::submission_reminders::run_check(&app.state).await;
        zerf::submission_reminders::run_check(&app.state).await;

        let (st, body) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let reminders: Vec<_> = body
            .as_array()
            .expect("notifications array")
            .iter()
            .filter(|n| n["kind"] == "submission_reminder")
            .collect();
        assert_eq!(reminders.len(), 1, "should deduplicate");
    }

    // -- Reminder skips assistants even if legacy data contains non-zero weekly hours --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "assistant-reminder@example.com",
                    "first_name": "Assistant",
                    "last_name": "Reminder",
                    "role": "assistant",
                    "weekly_hours": 0,
                    "leave_days_current_year": 0,
                    "leave_days_next_year": 0,
                    "start_date": "2024-01-01",
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        let assistant_id = id(&body);
        let assistant_pw = temp_pw(&body);

        sqlx::query("UPDATE users SET weekly_hours = 39 WHERE id = $1")
            .bind(assistant_id)
            .execute(&app.state.pool)
            .await
            .unwrap();

        let assistant = login_change_pw(&app, "assistant-reminder@example.com", &assistant_pw).await;

        let (st, _) = assistant.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::submission_reminders::run_check(&app.state).await;

        let (st, body) = assistant.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let reminders: Vec<_> = body
            .as_array()
            .expect("notifications array")
            .iter()
            .filter(|n| n["kind"] == "submission_reminder")
            .collect();
        assert_eq!(reminders.len(), 0, "assistant is skipped by role policy");
    }

    // -- Reminder still skips legacy zero-hours employees because reminder policy is independent of assistant role --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "zerohrs@example.com",
                    "first_name": "Zero",
                    "last_name": "Hours",
                    "role": "employee",
                    "weekly_hours": 0,
                    "leave_days_current_year": 0,
                    "leave_days_next_year": 0,
                    "start_date": "2024-01-01",
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        let emp_pw = temp_pw(&body);

        let emp = login_change_pw(&app, "zerohrs@example.com", &emp_pw).await;

        let (st, _) = emp.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::submission_reminders::run_check(&app.state).await;

        let (st, body) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let reminders: Vec<_> = body
            .as_array()
            .expect("notifications array")
            .iter()
            .filter(|n| n["kind"] == "submission_reminder")
            .collect();
        assert_eq!(reminders.len(), 0, "zero-hours user skipped");
    }

    // -- Reminder still warns when the only submitted entry does not count as work --
    {
        let today = chrono::Local::now().date_naive();
        let last_month_start = if today.month() == 1 {
            chrono::NaiveDate::from_ymd_opt(today.year() - 1, 12, 1).unwrap()
        } else {
            chrono::NaiveDate::from_ymd_opt(today.year(), today.month() - 1, 1).unwrap()
        };
        let start_date = last_month_start.format("%Y-%m-%d").to_string();

        let (_, categories_body) = admin.get("/api/v1/categories").await;
        let flextime_category_id = category_id_by_name(&categories_body, "Flextime Reduction")
            .expect("seeded flextime reduction category");

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "flextime-reminder@example.com",
                    "first_name": "Flextime",
                    "last_name": "Reminder",
                    "role": "employee",
                    "weekly_hours": 20,
                    "leave_days_current_year": 10,
                    "leave_days_next_year": 10,
                    "start_date": start_date,
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        let emp_pw = temp_pw(&body);

        let emp = login_change_pw(&app, "flextime-reminder@example.com", &emp_pw).await;

        let entry_date = last_month_start.format("%Y-%m-%d").to_string();
        let eid = create_draft_entry(&emp, &entry_date, flextime_category_id).await;

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [eid]}))
            .await;
        assert_eq!(st, StatusCode::OK);

        let (st, _) = emp.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::submission_reminders::run_check(&app.state).await;

        let (st, body) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let notifications = body.as_array().expect("notifications array");
        let reminder = notifications
            .iter()
            .find(|n| n["kind"] == "submission_reminder");
        assert!(
            reminder.is_some(),
            "non-crediting entries must not suppress the reminder"
        );
    }

    // -- Submission deadline day setting validation --
    {
        let (st, settings) = admin.get("/api/v1/settings").await;
        assert_eq!(st, StatusCode::OK);

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": settings["ui_language"],
                    "time_format": settings["time_format"],
                    "country": settings["country"],
                    "region": settings["region"],
                    "default_weekly_hours": settings["default_weekly_hours"],
                    "default_annual_leave_days": settings["default_annual_leave_days"],
                    "carryover_expiry_date": settings["carryover_expiry_date"],
                    "submission_deadline_day": 15
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "valid: day 15");

        let (st, updated) = admin.get("/api/v1/settings").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(updated["submission_deadline_day"], 15);

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": settings["ui_language"],
                    "time_format": settings["time_format"],
                    "country": settings["country"],
                    "region": settings["region"],
                    "default_weekly_hours": settings["default_weekly_hours"],
                    "default_annual_leave_days": settings["default_annual_leave_days"],
                    "carryover_expiry_date": settings["carryover_expiry_date"],
                    "submission_deadline_day": 0
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid: day 0");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": settings["ui_language"],
                    "time_format": settings["time_format"],
                    "country": settings["country"],
                    "region": settings["region"],
                    "default_weekly_hours": settings["default_weekly_hours"],
                    "default_annual_leave_days": settings["default_annual_leave_days"],
                    "carryover_expiry_date": settings["carryover_expiry_date"],
                    "submission_deadline_day": 29
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid: day 29");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": settings["ui_language"],
                    "time_format": settings["time_format"],
                    "country": settings["country"],
                    "region": settings["region"],
                    "default_weekly_hours": settings["default_weekly_hours"],
                    "default_annual_leave_days": settings["default_annual_leave_days"],
                    "carryover_expiry_date": settings["carryover_expiry_date"],
                    "submission_deadline_day": null
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK);

        let (st, cleared) = admin.get("/api/v1/settings").await;
        assert_eq!(st, StatusCode::OK);
        assert!(cleared["submission_deadline_day"].is_null());
    }

    app.cleanup().await;
}
