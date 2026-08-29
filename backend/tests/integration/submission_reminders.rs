//! End-to-end submission reminder tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use chrono::{Datelike, TimeZone};
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

        zerf::background::submission_reminders::run_check(&app.state).await;

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
        let ref_date = reference_date();
        let last_week_monday =
            ref_date - chrono::Duration::days(ref_date.weekday().num_days_from_monday() as i64 + 7);
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

        zerf::background::submission_reminders::run_check(&app.state).await;

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

        zerf::background::submission_reminders::run_check(&app.state).await;
        zerf::background::submission_reminders::run_check(&app.state).await;

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

        let assistant =
            login_change_pw(&app, "assistant-reminder@example.com", &assistant_pw).await;

        let (st, _) = assistant.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::background::submission_reminders::run_check(&app.state).await;

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

        zerf::background::submission_reminders::run_check(&app.state).await;

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

    // -- Reminder skips admins even when they have weekly hours and incomplete weeks --
    {
        let ref_date = reference_date();
        let last_month_start = if ref_date.month() == 1 {
            chrono::NaiveDate::from_ymd_opt(ref_date.year() - 1, 12, 1).unwrap()
        } else {
            chrono::NaiveDate::from_ymd_opt(ref_date.year(), ref_date.month() - 1, 1).unwrap()
        };
        let start_date = last_month_start.format("%Y-%m-%d").to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "admin-reminder@example.com",
                    "first_name": "Admin",
                    "last_name": "Reminder",
                    "role": "admin",
                    "tracks_time": false,
                    "weekly_hours": 20,
                    "start_date": start_date
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin user");
        let admin_pw = temp_pw(&body);

        let admin_user = login_change_pw(&app, "admin-reminder@example.com", &admin_pw).await;

        let (st, _) = admin_user.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);

        zerf::background::submission_reminders::run_check(&app.state).await;

        let (st, body) = admin_user.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let reminders: Vec<_> = body
            .as_array()
            .expect("notifications array")
            .iter()
            .filter(|n| n["kind"] == "submission_reminder")
            .collect();
        assert_eq!(reminders.len(), 0, "admins are excluded from reminders");
    }

    // -- Reminder still warns when the only submitted entry does not count as work --
    {
        let ref_date = reference_date();
        let last_month_start = if ref_date.month() == 1 {
            chrono::NaiveDate::from_ymd_opt(ref_date.year() - 1, 12, 1).unwrap()
        } else {
            chrono::NaiveDate::from_ymd_opt(ref_date.year(), ref_date.month() - 1, 1).unwrap()
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

        zerf::background::submission_reminders::run_check(&app.state).await;

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

#[tokio::test]
async fn submission_reminders_respects_enabled_toggle() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "toggle").await;
    let emp = login_change_pw(&app, "emp-toggle@example.com", &emp_pw).await;

    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    let (st, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(st, StatusCode::OK);

    let (st, _) = admin
        .put(
            "/api/v1/settings/smtp",
            &json!({
                "smtp_enabled": settings["smtp_enabled"],
                "smtp_host": settings["smtp_host"],
                "smtp_port": settings["smtp_port"],
                "smtp_username": settings["smtp_username"],
                "smtp_from": settings["smtp_from"],
                "smtp_encryption": settings["smtp_encryption"],
                "submission_reminders_enabled": false,
                "approval_reminders_enabled": settings["approval_reminders_enabled"]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "disable reminders");

    zerf::background::submission_reminders::run_check(&app.state).await;

    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders_disabled: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|n| n["kind"] == "submission_reminder")
        .collect();
    assert_eq!(
        reminders_disabled.len(),
        0,
        "disabled toggle suppresses reminders"
    );

    let (st, _) = admin
        .put(
            "/api/v1/settings/smtp",
            &json!({
                "smtp_enabled": settings["smtp_enabled"],
                "smtp_host": settings["smtp_host"],
                "smtp_port": settings["smtp_port"],
                "smtp_username": settings["smtp_username"],
                "smtp_from": settings["smtp_from"],
                "smtp_encryption": settings["smtp_encryption"],
                "submission_reminders_enabled": true,
                "approval_reminders_enabled": settings["approval_reminders_enabled"]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "enable reminders");

    zerf::background::submission_reminders::run_check(&app.state).await;

    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders_enabled: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|n| n["kind"] == "submission_reminder")
        .collect();
    assert_eq!(
        reminders_enabled.len(),
        1,
        "enabled toggle allows reminders"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn submission_reminders_treat_approved_absence_as_covered_week() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let ref_date = reference_date();
    let last_week_monday =
        ref_date - chrono::Duration::days(ref_date.weekday().num_days_from_monday() as i64 + 7);
    let week_start = last_week_monday.format("%Y-%m-%d").to_string();
    let week_end = (last_week_monday + chrono::Duration::days(4))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "absence-reminder@example.com",
                "first_name": "Absence",
                "last_name": "Reminder",
                "role": "employee",
                "weekly_hours": 20,
                "start_date": week_start,
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let emp_pw = temp_pw(&body);

    let emp = login_change_pw(&app, "absence-reminder@example.com", &emp_pw).await;

    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": week_start,"end_date": week_end}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create requested absence");
    let absence_id = id(&body);

    let (st, _) = admin
        .post(
            &format!("/api/v1/absences/{absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve absence");

    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    zerf::background::submission_reminders::run_check(&app.state).await;

    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|n| n["kind"] == "submission_reminder")
        .collect();
    assert_eq!(
        reminders.len(),
        0,
        "approved absence should suppress reminder for that week"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn submission_reminders_treat_requested_absence_as_covered_week() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let ref_date = reference_date();
    let last_week_monday =
        ref_date - chrono::Duration::days(ref_date.weekday().num_days_from_monday() as i64 + 7);
    let week_start = last_week_monday.format("%Y-%m-%d").to_string();
    let week_end = (last_week_monday + chrono::Duration::days(4))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "requested-absence-reminder@example.com",
                "first_name": "Requested",
                "last_name": "Reminder",
                "role": "employee",
                "weekly_hours": 20,
                "start_date": week_start,
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let emp_pw = temp_pw(&body);

    let emp = login_change_pw(&app, "requested-absence-reminder@example.com", &emp_pw).await;

    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": week_start,"end_date": week_end}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create requested absence");
    assert_eq!(body["status"], "requested");

    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    zerf::background::submission_reminders::run_check(&app.state).await;

    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|n| n["kind"] == "submission_reminder")
        .collect();
    assert_eq!(
        reminders.len(),
        0,
        "requested absence should suppress reminder for that week while approval is pending"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn submission_reminders_treat_cancellation_pending_absence_as_covered_week() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let ref_date = reference_date();
    let last_week_monday =
        ref_date - chrono::Duration::days(ref_date.weekday().num_days_from_monday() as i64 + 7);
    let week_start = last_week_monday.format("%Y-%m-%d").to_string();
    let week_end = (last_week_monday + chrono::Duration::days(4))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "cancel-pending-reminder@example.com",
                "first_name": "Cancel",
                "last_name": "Pending",
                "role": "employee",
                "weekly_hours": 20,
                "start_date": week_start,
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let emp_pw = temp_pw(&body);

    let emp = login_change_pw(&app, "cancel-pending-reminder@example.com", &emp_pw).await;

    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": week_start,"end_date": week_end}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create requested absence");
    let absence_id = id(&body);

    let (st, _) = admin
        .post(
            &format!("/api/v1/absences/{absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve absence");

    let (st, body) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
    assert_eq!(st, StatusCode::OK, "request cancellation");
    assert_eq!(
        body["pending"],
        serde_json::Value::Bool(true),
        "approved absence cancellation should become pending"
    );

    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    zerf::background::submission_reminders::run_check(&app.state).await;

    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|n| n["kind"] == "submission_reminder")
        .collect();
    assert_eq!(
        reminders.len(),
        0,
        "cancellation_pending absence should still suppress reminder for that week"
    );

    app.cleanup().await;
}


/// Both month-boundary passes only fire on something genuinely missing, and
/// they divide the audience between them: an assistant is asked about the
/// bookings they never handed in, because that is the only evidence the app
/// has for them, while everybody with a fixed contract gets the missing-week
/// list instead. Nobody gets both.
#[tokio::test]
async fn month_end_reminders_split_assistants_from_contract_employees() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, _emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "month-end").await;
    let emp = login_change_pw(&app, "emp-month-end@example.com", &emp_pw).await;

    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "aushilfe-month-end@example.com",
                "first_name": "Alex",
                "last_name": "AssistMonthEnd",
                "role": "assistant",
                "weekly_hours": 0,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create assistant: {body}");
    let assistant = login_change_pw(
        &app,
        "aushilfe-month-end@example.com",
        &temp_pw(&body),
    )
    .await;

    // The reminder names a deadline, and takes it from the payroll send day.
    for (key, value) in [
        (zerf::services::settings::PAYROLL_REPORT_ENABLED_KEY, "true"),
        (
            zerf::services::settings::PAYROLL_REPORT_DAY_OF_MONTH_KEY,
            "5",
        ),
    ] {
        app.state
            .db
            .settings
            .save_setting(key, value)
            .await
            .expect("configure the deadline");
    }

    // A day in the middle of the finished month, left as a draft by both.
    let ref_date = reference_date();
    let first_of_month = ref_date.with_day(1).expect("first of month");
    let in_previous_month = (first_of_month - chrono::Duration::days(1))
        .with_day(15)
        .expect("15th of the previous month")
        .format("%Y-%m-%d")
        .to_string();
    for client in [&emp, &assistant] {
        let (status, _) = client
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": in_previous_month,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "create draft in the finished month");
        let (status, _) = client.delete("/api/v1/notifications").await;
        assert_eq!(status, StatusCode::OK);
    }

    let first_of_month = chrono_tz::Europe::Berlin
        .with_ymd_and_hms(ref_date.year(), ref_date.month(), 1, 8, 0, 0)
        .single()
        .expect("local time");
    zerf::background::submission_reminders::run_month_end_check(&app.state, first_of_month).await;
    zerf::background::submission_reminders::run_month_weeks_reminder(&app.state, first_of_month)
        .await;

    let kinds_of = |body: &serde_json::Value| -> Vec<String> {
        body.as_array()
            .expect("notifications array")
            .iter()
            .filter_map(|item| item["kind"].as_str().map(str::to_string))
            .collect()
    };

    // The assistant is asked directly: a booking they never handed in is the
    // only evidence the app has that they owe anything.
    let (status, body) = assistant.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kinds_of(&body)
            .iter()
            .filter(|kind| *kind == "month_end_submission_reminder")
            .count(),
        1,
        "the assistant must be reminded exactly once: {body}"
    );

    // The employee is served by the missing-week list instead, which says the
    // same thing with more detail — so they must not get both.
    let (status, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK);
    let employee_kinds = kinds_of(&body);
    assert_eq!(
        employee_kinds
            .iter()
            .filter(|kind| *kind == "month_weeks_reminder")
            .count(),
        1,
        "the employee must get the missing-week list: {body}"
    );
    assert!(
        !employee_kinds
            .iter()
            .any(|kind| kind == "month_end_submission_reminder"),
        "and must not also get the assistants' message: {body}"
    );

    // A second pass on the same day must not nag again — somebody working
    // through their backlog would otherwise be reminded on every wake-up.
    zerf::background::submission_reminders::run_month_end_check(&app.state, first_of_month).await;
    zerf::background::submission_reminders::run_month_weeks_reminder(&app.state, first_of_month)
        .await;
    let (status, body) = assistant.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kinds_of(&body)
            .iter()
            .filter(|kind| *kind == "month_end_submission_reminder")
            .count(),
        1,
        "the month-end reminder must be sent once per month"
    );

    app.cleanup().await;
}


/// December 2029 ends on a Monday, so its last day sits in the week
/// 31.12.–06.01. Asking for that week on the 1st would mean asking somebody to
/// hand in a week they are still working; from its Friday the ask is fair, and
/// only the days that belong to December are what December still needs.
#[tokio::test]
async fn the_straddling_week_is_only_asked_for_from_its_friday() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "straddle").await;
    let emp = login_change_pw(&app, "emp-straddle@example.com", &emp_pw).await;

    let december = (
        chrono::NaiveDate::from_ymd_opt(2029, 12, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2029, 12, 31).unwrap(),
    );
    let straddling_monday = chrono::NaiveDate::from_ymd_opt(2029, 12, 31).unwrap();
    let start_date = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let pool = app.state.pool.clone();
    let missing_on = move |today: chrono::NaiveDate| {
        let pool = pool.clone();
        async move {
            zerf::services::reports::unsubmitted_weeks_in_month(
                &pool,
                emp_id,
                december.0,
                december.1,
                start_date,
                5,
                today,
            )
            .await
            .expect("missing weeks")
        }
    };

    let on_the_first = missing_on(chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap()).await;
    assert!(
        !on_the_first.is_empty(),
        "December's own weeks are missing and must be named"
    );
    assert!(
        !on_the_first.contains(&straddling_monday),
        "the week the employee is still working must not be asked for yet: {on_the_first:?}"
    );

    let on_friday = missing_on(chrono::NaiveDate::from_ymd_opt(2030, 1, 4).unwrap()).await;
    assert!(
        on_friday.contains(&straddling_monday),
        "from its Friday the straddling week is due: {on_friday:?}"
    );

    // Handing in just the December day of that week settles it — the drafts
    // that follow in January belong to January's month, not December's.
    let (status, _) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": "2029-12-31",
                "start_time": "08:00",
                "end_time": "16:00",
                "category_id": _cat_id,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "book the month's last day");
    let (status, body) = emp.get("/api/v1/time-entries?from=2029-12-31&to=2029-12-31").await;
    assert_eq!(status, StatusCode::OK);
    let entry_ids: Vec<i64> = body
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["id"].as_i64().expect("id"))
        .collect();
    let (status, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": entry_ids}))
        .await;
    assert_eq!(status, StatusCode::OK, "submit it");

    let after_submitting = missing_on(chrono::NaiveDate::from_ymd_opt(2030, 1, 4).unwrap()).await;
    assert!(
        !after_submitting.contains(&straddling_monday),
        "the December part is handed in, so the week is settled: {after_submitting:?}"
    );

    app.cleanup().await;
}


/// Three rules in one run, on the calendar that actually causes the trouble:
/// December 2029 ends on a Monday, so its last day sits in the week
/// 31.12.–06.01.
///
///  * that week is not asked for on the 1st — it is still being worked;
///  * from its Friday it is, and the reminder names the deadline from the
///    general settings;
///  * once handed in it produces nothing further, even though nobody has
///    approved it yet — that is no longer the employee's move.
#[tokio::test]
async fn reminders_only_chase_what_is_genuinely_missing() {
    use chrono::TimeZone;

    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "genuine").await;
    let _lead = login_change_pw(&app, "lead-genuine@example.com", &lead_pw).await;

    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::SUBMISSION_DEADLINE_DAY_KEY, "10")
        .await
        .expect("configure the deadline day");

    // Starting on the month's last day leaves exactly one December week to
    // judge: the one reaching into January.
    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "boundary-genuine@example.com",
                "first_name": "Bo",
                "last_name": "Boundary",
                "role": "employee",
                "weekly_hours": 39,
                "start_date": "2029-12-31",
                "approver_ids": [lead_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create boundary employee: {body}");
    let boundary = login_change_pw(&app, "boundary-genuine@example.com", &temp_pw(&body)).await;

    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "aushilfe-genuine@example.com",
                "first_name": "Alex",
                "last_name": "Assist",
                "role": "assistant",
                "weekly_hours": 0,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create assistant: {body}");
    let assistant = login_change_pw(&app, "aushilfe-genuine@example.com", &temp_pw(&body)).await;

    for (client, day) in [(&boundary, "2029-12-31"), (&assistant, "2029-12-10")] {
        let (status, _) = client
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "book {day}");
        let (status, _) = client.delete("/api/v1/notifications").await;
        assert_eq!(status, StatusCode::OK);
    }

    let at = |day: u32| {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(2030, 1, day, 8, 0, 0)
            .single()
            .expect("local time")
    };
    let kinds = |body: &serde_json::Value| -> Vec<String> {
        body.as_array()
            .expect("notifications array")
            .iter()
            .filter_map(|item| item["kind"].as_str().map(str::to_string))
            .collect()
    };
    let state = app.state.clone();
    let run = move |now: chrono::DateTime<chrono_tz::Tz>| {
        let state = state.clone();
        async move {
            zerf::background::submission_reminders::run_month_end_check(&state, now).await;
            zerf::background::submission_reminders::run_month_weeks_reminder(&state, now).await;
        }
    };

    // The 1st: the assistant is asked about the day they never handed in. The
    // employee is not — their only December week is still being worked.
    run(at(1)).await;
    let (_, body) = assistant.get("/api/v1/notifications").await;
    assert!(
        kinds(&body).contains(&"month_end_submission_reminder".to_string()),
        "the assistant holds an unsubmitted booking: {body}"
    );
    let (_, body) = boundary.get("/api/v1/notifications").await;
    assert!(
        !kinds(&body).contains(&"month_weeks_reminder".to_string()),
        "the week reaching into January is not due on the 1st: {body}"
    );

    // The 4th is that week's Friday: now it is fair to ask, and the message
    // names the deadline from the general settings.
    run(at(4)).await;
    let (_, body) = boundary.get("/api/v1/notifications").await;
    let reminder = body
        .as_array()
        .expect("notifications array")
        .iter()
        .find(|item| item["kind"] == "month_weeks_reminder")
        .unwrap_or_else(|| panic!("no week reminder on the Friday: {body}"));
    let deadline = zerf::i18n::format_date(
        &zerf::i18n::Language::default(),
        chrono::NaiveDate::from_ymd_opt(2030, 1, 10).unwrap(),
    );
    assert!(
        reminder["body"].as_str().unwrap_or_default().contains(&deadline),
        "the reminder names the configured deadline {deadline}: {reminder}"
    );

    // Both hand their days in. Nobody has approved them, and that is nobody's
    // move but the approver's — so the next pass has nothing left to say.
    for client in [&boundary, &assistant] {
        let (status, body) = client
            .get("/api/v1/time-entries?from=2029-12-01&to=2029-12-31")
            .await;
        assert_eq!(status, StatusCode::OK);
        let ids: Vec<i64> = body
            .as_array()
            .expect("entries")
            .iter()
            .map(|entry| entry["id"].as_i64().expect("id"))
            .collect();
        let (status, _) = client
            .post("/api/v1/time-entries/submit", &json!({"ids": ids}))
            .await;
        assert_eq!(status, StatusCode::OK, "submit December");
        let (status, _) = client.delete("/api/v1/notifications").await;
        assert_eq!(status, StatusCode::OK);
    }

    run(at(7)).await;
    for (client, who) in [(&boundary, "employee"), (&assistant, "assistant")] {
        let (_, body) = client.get("/api/v1/notifications").await;
        let remaining = kinds(&body);
        assert!(
            !remaining.contains(&"month_weeks_reminder".to_string())
                && !remaining.contains(&"month_end_submission_reminder".to_string()),
            "{who} handed everything in; waiting for approval is not their move: {body}"
        );
    }

    app.cleanup().await;
}
