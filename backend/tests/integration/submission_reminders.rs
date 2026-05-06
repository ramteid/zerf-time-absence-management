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
async fn reminder_creates_notification_for_unsubmitted_months() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create employee with start_date in the past (2024-01-01 via bootstrap_team)
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // Clear any existing notifications
    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    // Run submission reminder check directly
    zerf::submission_reminders::run_check(&app.state).await;

    // Employee should have received a submission_reminder notification
    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let notifications = body.as_array().expect("notifications array");
    let reminder = notifications
        .iter()
        .find(|n| n["kind"] == "submission_reminder");
    assert!(
        reminder.is_some(),
        "employee should receive submission_reminder notification"
    );

    // Body should mention months
    let reminder = reminder.unwrap();
    assert!(
        !reminder["body"].as_str().unwrap_or("").is_empty(),
        "notification body should not be empty"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn reminder_skips_user_with_all_submitted() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create employee with recent start_date so only last month matters
    let today = chrono::Local::now().date_naive();
    let last_month_start = if today.month() == 1 {
        chrono::NaiveDate::from_ymd_opt(today.year() - 1, 12, 1).unwrap()
    } else {
        chrono::NaiveDate::from_ymd_opt(today.year(), today.month() - 1, 1).unwrap()
    };
    let start_date = last_month_start.format("%Y-%m-%d").to_string();

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
                "annual_leave_days": 10,
                "start_date": start_date,
                "approver_id": 1
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create recent employee");
    let emp_pw = temp_pw(&body);

    let emp = login_change_pw(&app, "recent@example.com", &emp_pw).await;

    // Create an entry in last month and submit it
    let entry_date = last_month_start.format("%Y-%m-%d").to_string();
    let eid = create_draft_entry(&emp, &entry_date, cat_id).await;

    // Submit the entry
    let (st, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": [eid]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit entry");

    // Clear notifications
    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    // Run check
    zerf::submission_reminders::run_check(&app.state).await;

    // Should have no submission_reminder notifications
    let (st, body) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let notifications = body.as_array().expect("notifications array");
    let reminder = notifications
        .iter()
        .find(|n| n["kind"] == "submission_reminder");
    assert!(
        reminder.is_none(),
        "fully submitted user should not receive reminder, got: {:?}",
        notifications
    );

    app.cleanup().await;
}

#[tokio::test]
async fn reminder_deduplicates_on_same_day() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // Clear notifications
    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    // Run check twice
    zerf::submission_reminders::run_check(&app.state).await;
    zerf::submission_reminders::run_check(&app.state).await;

    // Should only have one submission_reminder notification
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
        1,
        "should have exactly 1 reminder after 2 runs, got {}",
        reminders.len()
    );

    app.cleanup().await;
}

#[tokio::test]
async fn reminder_skips_zero_hours_user() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create user with weekly_hours = 0
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "zerohrs@example.com",
                "first_name": "Zero",
                "last_name": "Hours",
                "role": "employee",
                "weekly_hours": 0,
                "annual_leave_days": 0,
                "start_date": "2024-01-01",
                "approver_id": 1
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create zero-hours user");
    let emp_pw = temp_pw(&body);

    let emp = login_change_pw(&app, "zerohrs@example.com", &emp_pw).await;

    // Clear notifications
    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    // Run check
    zerf::submission_reminders::run_check(&app.state).await;

    // Should have no submission_reminder
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
        "zero-hours user should not receive reminder"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn submission_deadline_day_setting_validation() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Get current settings to have all required fields
    let (st, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(st, StatusCode::OK);

    // Valid: day 15
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
    assert_eq!(st, StatusCode::OK, "valid deadline day 15");

    // Verify it was saved
    let (st, updated) = admin.get("/api/v1/settings").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(updated["submission_deadline_day"], 15);

    // Invalid: day 0
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
    assert_eq!(st, StatusCode::BAD_REQUEST, "day 0 should be rejected");

    // Invalid: day 29
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
    assert_eq!(st, StatusCode::BAD_REQUEST, "day 29 should be rejected");

    // Clear: null
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
    assert_eq!(st, StatusCode::OK, "null should clear the setting");

    let (st, cleared) = admin.get("/api/v1/settings").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        cleared["submission_deadline_day"].is_null(),
        "deadline day should be cleared"
    );

    app.cleanup().await;
}
