use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn invalid_category_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, _) = admin
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": today(),
                "start_time": "08:00",
                "end_time": "10:00",
                "category_id": 999_999_i64,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "nonexistent category -> 400");

    app.cleanup().await;
}

#[tokio::test]
async fn reject_requires_reason_before_ownership_check() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, _emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let (st, body) = lead
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "lead work"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "lead creates own entry");
    let entry_id = id(&body);

    let (st, _) = lead
        .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
        .await;
    assert_eq!(st, StatusCode::OK, "lead submits own entry");

    let (st, _) = lead
        .post(
            &format!("/api/v1/time-entries/{}/reject", entry_id),
            &json!({"reason": "   "}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "blank reason wins over ownership check"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn blocks_time_entry_when_absence_cancellation_pending() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "vacation",
                "start_date": monday_iso,
                "end_date": monday_iso,
                "comment": "day off"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create requested absence");
    let absence_id = id(&body);

    let (st, _) = lead
        .post(&format!("/api/v1/absences/{absence_id}/approve"), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "approve absence");

    let (st, body) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
    assert_eq!(st, StatusCode::OK, "request cancellation");
    assert_eq!(body["pending"], true);

    let (st, _) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso,
                "start_time": "08:00",
                "end_time": "10:00",
                "category_id": cat_id,
                "comment": "should be blocked"
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "cancellation-pending absence must still block time logging"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn admin_can_batch_reject_own_submitted_entry() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let monday_iso = next_monday(-14).format("%Y-%m-%d").to_string();
    let (_, categories_body) = admin.get("/api/v1/categories").await;
    let category_id = categories_body.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let (st, body) = admin
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso,
                "start_time": "08:00",
                "end_time": "11:00",
                "category_id": category_id,
                "comment": "admin entry"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin creates own entry");
    let entry_id = id(&body);

    let (st, _) = admin
        .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit admin entry");

    let (st, body) = admin
        .post(
            "/api/v1/time-entries/batch-reject",
            &json!({"ids": [entry_id], "reason": "needs correction"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "batch reject succeeds");
    assert_eq!(body["count"], 1);

    let (_, entries) = admin.get("/api/v1/time-entries").await;
    let entry = find_by_id(&entries, entry_id).expect("entry exists");
    assert_eq!(entry["status"], "rejected");

    app.cleanup().await;
}
