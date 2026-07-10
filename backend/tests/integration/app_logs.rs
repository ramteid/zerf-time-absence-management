use chrono::Duration;
use reqwest::StatusCode;
use serde_json::json;
use sqlx::query_scalar;

use crate::common::TestApp;
use crate::helpers::{admin_login, login_change_pw, temp_pw};
use zerf::repository::AppLogDb;

#[tokio::test]
async fn logs_endpoint_is_forbidden_for_non_admin_users() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "logs-employee@example.com",
                "first_name": "Eva",
                "last_name": "Employee",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "annual_leave_days": 30,
                "start_date": "2024-01-01",
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let employee_pw = temp_pw(&body);

    let employee = login_change_pw(&app, "logs-employee@example.com", &employee_pw).await;
    let (st, _) = employee.get("/api/v1/logs").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "employee must not read app logs");

    app.cleanup().await;
}

#[tokio::test]
async fn logs_endpoint_paginates_newest_first() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let db = AppLogDb::new(app.state.pool.clone());
    let base = chrono::Utc::now();
    for i in 0_i64..120_i64 {
        db.insert(
            if i % 2 == 0 { "warn" } else { "error" },
            &format!("message {i}"),
            "zerf::test",
            Some(json!({"index": i.to_string()})),
            base + Duration::milliseconds(i),
        )
        .await
        .expect("insert app log row");
    }

    // First page: default limit is 100, newest entries first.
    let (st, body) = admin.get("/api/v1/logs").await;
    assert_eq!(st, StatusCode::OK, "first page query");
    assert_eq!(body["total"].as_i64(), Some(120));
    let rows = body["entries"]
        .as_array()
        .expect("logs response must contain an entries array");
    assert_eq!(rows.len(), 100, "default page size must be 100");
    assert_eq!(rows[0]["message"].as_str(), Some("message 119"));
    assert_eq!(rows[0]["level"].as_str(), Some("error"));
    assert_eq!(rows[0]["target"].as_str(), Some("zerf::test"));
    assert_eq!(rows[0]["fields"]["index"].as_str(), Some("119"));
    assert_eq!(rows[99]["message"].as_str(), Some("message 20"));

    // Second page: the remaining 20 rows.
    let (st, body) = admin.get("/api/v1/logs?offset=100").await;
    assert_eq!(st, StatusCode::OK, "second page query");
    assert_eq!(body["total"].as_i64(), Some(120));
    let rows = body["entries"]
        .as_array()
        .expect("logs response must contain an entries array");
    assert_eq!(rows.len(), 20, "second page holds the remainder");
    assert_eq!(rows[0]["message"].as_str(), Some("message 19"));
    assert_eq!(rows[19]["message"].as_str(), Some("message 0"));

    app.cleanup().await;
}

#[tokio::test]
async fn logs_endpoint_clamps_an_oversized_limit_to_500() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let db = AppLogDb::new(app.state.pool.clone());
    let base = chrono::Utc::now();
    for i in 0_i64..510_i64 {
        db.insert(
            "warn",
            &format!("message {i}"),
            "zerf::test",
            None,
            base + Duration::milliseconds(i),
        )
        .await
        .expect("insert app log row");
    }

    // A client-requested limit far above the 500-row ceiling must be clamped,
    // not honored verbatim.
    let (st, body) = admin.get("/api/v1/logs?limit=999999").await;
    assert_eq!(st, StatusCode::OK, "oversized limit query");
    let rows = body["entries"]
        .as_array()
        .expect("logs response must contain an entries array");
    assert_eq!(rows.len(), 500, "limit must be clamped to the 500-row ceiling");
    assert_eq!(body["total"].as_i64(), Some(510));

    app.cleanup().await;
}

#[tokio::test]
async fn app_log_prune_enforces_row_cap_and_age_expiry() {
    let app = TestApp::spawn().await;

    let db = AppLogDb::new(app.state.pool.clone());
    let now = chrono::Utc::now();

    // One row past the 365-day age limit, then 1010 fresh rows — 11 more than
    // the 1000-row cap allows in total.
    db.insert(
        "error",
        "expired entry",
        "zerf::test",
        None,
        now - Duration::days(366),
    )
    .await
    .expect("insert expired row");
    for i in 0_i64..1010_i64 {
        db.insert(
            "warn",
            &format!("fresh {i}"),
            "zerf::test",
            None,
            now + Duration::milliseconds(i),
        )
        .await
        .expect("insert fresh row");
    }

    db.prune().await.expect("prune");

    let count: i64 = query_scalar("SELECT COUNT(*) FROM app_logs")
        .fetch_one(&app.state.pool)
        .await
        .expect("count rows");
    assert_eq!(count, 1000, "prune must enforce the 1000-row cap");

    let expired: i64 =
        query_scalar("SELECT COUNT(*) FROM app_logs WHERE message = 'expired entry'")
            .fetch_one(&app.state.pool)
            .await
            .expect("count expired");
    assert_eq!(expired, 0, "rows older than 365 days must be deleted");

    // The oldest fresh rows beyond the cap are gone, the newest ones remain.
    let oldest_fresh: i64 = query_scalar("SELECT COUNT(*) FROM app_logs WHERE message = 'fresh 0'")
        .fetch_one(&app.state.pool)
        .await
        .expect("count oldest fresh");
    assert_eq!(oldest_fresh, 0, "cap must drop the oldest rows first");
    let newest_fresh: i64 =
        query_scalar("SELECT COUNT(*) FROM app_logs WHERE message = 'fresh 1009'")
            .fetch_one(&app.state.pool)
            .await
            .expect("count newest fresh");
    assert_eq!(newest_fresh, 1, "the newest row must survive pruning");

    app.cleanup().await;
}
