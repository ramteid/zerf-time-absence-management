use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, date_offset, today};

/// Admin's start_date is set to today during seed. Verify that creating a time
/// entry before that date is rejected.
#[tokio::test]
async fn time_entry_before_start_date_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, cats) = admin.get("/api/v1/categories").await;
    let cat_id = cats.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let yesterday = date_offset(-1);
    let (st, body) = admin
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": yesterday,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "entry before start_date: {body}"
    );
    assert!(
        body.to_string().contains("before user start date"),
        "error message: {body}"
    );
}

/// Time entry on the start_date itself must succeed.
#[tokio::test]
async fn time_entry_on_start_date_accepted() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, cats) = admin.get("/api/v1/categories").await;
    let cat_id = cats.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let (st, _) = admin
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": today(),
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "entry on start_date should succeed");
}

/// Absence that starts before the user's start_date must be rejected.
#[tokio::test]
async fn absence_before_start_date_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let yesterday = date_offset(-1);
    let (st, body) = admin
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "vacation",
                "start_date": yesterday,
                "end_date": yesterday,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "absence before start_date: {body}"
    );
    assert!(
        body.to_string().contains("before user start date"),
        "error message: {body}"
    );
}

/// Absence on or after start_date should be accepted.
#[tokio::test]
async fn absence_on_start_date_accepted() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, _) = admin
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "sick",
                "start_date": today(),
                "end_date": today(),
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "absence on start_date should succeed");
}

/// Overtime report for a user whose start_date is today should show only the
/// current month (if any), not months before it, and the cumulative balance
/// must be non-positive only by at most one day's target.
#[tokio::test]
async fn overtime_no_months_before_start_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let year = chrono::Local::now().format("%Y").to_string();
    let (st, body) = admin
        .get(&format!("/api/v1/reports/overtime?year={year}"))
        .await;
    assert_eq!(st, StatusCode::OK);

    let rows = body.as_array().expect("overtime should be array");
    // Admin was seeded with today's date. Only the current month (or none) should appear.
    let current_month = chrono::Local::now().format("%Y-%m").to_string();
    for row in rows {
        let month = row["month"].as_str().unwrap();
        assert!(
            month >= current_month.as_str(),
            "month {month} is before start month {current_month}"
        );
    }
    // The cumulative balance must not be wildly negative (max 1 day deficit).
    if let Some(last) = rows.last() {
        let cum = last["cumulative_min"].as_i64().unwrap();
        // 39h/week => 468 min/day max deficit
        assert!(
            cum >= -468,
            "cumulative overtime {cum} min is too negative for a fresh user"
        );
    }
}

#[tokio::test]
async fn overtime_start_balance_carries_into_later_years() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let current_year: i32 = chrono::Local::now()
        .format("%Y")
        .to_string()
        .parse()
        .unwrap();
    let start_date = chrono::NaiveDate::from_ymd_opt(current_year - 1, 1, 1)
        .unwrap()
        .to_string();
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "carry@example.com",
                "first_name": "Carry",
                "last_name": "Balance",
                "role": "admin",
                "weekly_hours": 0,
                "annual_leave_days": 0,
                "start_date": start_date,
                "overtime_start_balance_min": 120
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create carry-balance user: {body}");
    let uid = body["id"].as_i64().unwrap();

    let (st, body) = admin
        .get(&format!(
            "/api/v1/reports/overtime?user_id={uid}&year={current_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK);
    let rows = body.as_array().expect("overtime should be array");
    assert!(!rows.is_empty(), "current year should have overtime rows");
    assert_eq!(
        rows[0]["cumulative_min"].as_i64(),
        Some(120),
        "start balance should carry into the next year"
    );
    assert_eq!(
        rows.last().unwrap()["cumulative_min"].as_i64(),
        Some(120),
        "zero-hour user should keep the carried balance"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn flextime_start_balance_begins_on_start_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "flex-carry@example.com",
                "first_name": "Flex",
                "last_name": "Carry",
                "role": "admin",
                "weekly_hours": 0,
                "annual_leave_days": 0,
                "start_date": today(),
                "overtime_start_balance_min": 120
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create flex carry user: {body}");
    let uid = body["id"].as_i64().unwrap();

    let from = date_offset(-1);
    let to = today();
    let (st, body) = admin
        .get(&format!(
            "/api/v1/reports/flextime?user_id={uid}&from={from}&to={to}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime report");
    let rows = body.as_array().expect("flextime should be array");
    assert_eq!(
        rows.first().unwrap()["cumulative_min"].as_i64(),
        Some(0),
        "balance should not apply before the user's start date"
    );
    assert_eq!(
        rows.last().unwrap()["cumulative_min"].as_i64(),
        Some(120),
        "balance should apply on the user's start date"
    );

    app.cleanup().await;
}

/// A newly created user with a future-ish start_date should not be able to
/// create entries before that date.
#[tokio::test]
async fn new_user_start_date_enforced() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create a user with start_date = today
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "new@example.com",
                "first_name": "New",
                "last_name": "User",
                "role": "admin",
                "weekly_hours": 39,
                "annual_leave_days": 30,
                "start_date": today(),
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let pw = body["temporary_password"].as_str().unwrap().to_string();

    // Login as the new user
    let new_client = app.client();
    let (st, _) = new_client.login("new@example.com", &pw).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = new_client.change_password(&pw, "NewPass!2345").await;
    assert_eq!(st, StatusCode::OK);

    let (_, cats) = new_client.get("/api/v1/categories").await;
    let cat_id = cats.as_array().unwrap()[0]["id"].as_i64().unwrap();

    // Entry yesterday (before start_date) should fail
    let yesterday = date_offset(-1);
    let (st, _) = new_client
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": yesterday,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "entry before start_date for new user"
    );

    // Entry today should succeed
    let (st, _) = new_client
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": today(),
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "entry on start_date for new user");
}
