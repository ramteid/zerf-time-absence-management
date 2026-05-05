use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn notifications_crud() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, true).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let _ = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    emp.post(
        "/api/v1/reopen-requests",
        &json!({"week_start": monday_iso}),
    )
    .await;

    let (st, body) = emp.get("/api/v1/notifications/unread-count").await;
    assert_eq!(st, StatusCode::OK);
    assert!(body["count"].as_i64().unwrap() >= 1);

    let (st, list) = emp.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let nid = list[0]["id"].as_i64().unwrap();

    let (st, _) = emp
        .post(&format!("/api/v1/notifications/{}/read", nid), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _) = emp.post("/api/v1/notifications/read-all", &json!({})).await;
    assert_eq!(st, StatusCode::OK);

    let (st, body) = emp.get("/api/v1/notifications/unread-count").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["count"], 0);

    let (st, _) = emp.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let (_, list) = emp.get("/api/v1/notifications").await;
    assert_eq!(list.as_array().unwrap().len(), 0);

    app.cleanup().await;
}

#[tokio::test]
async fn absence_request_notifies_approver() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, _cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "vacation",
                "start_date": monday_iso,
                "end_date": monday_iso,
                "comment": "need a day off"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create absence request");
    assert_eq!(body["status"], "requested");

    let (st, body) = lead.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK, "lead notifications");
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "absence_requested"),
        "lead received absence request notification"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn change_request_creation_notifies_approver() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let (st, body) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid,
                "new_start_time": "09:00",
                "reason": "came in later"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create change request");
    assert_eq!(body["status"], "open");

    let (st, body) = lead.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK, "lead notifications");
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "change_request_created"),
        "lead received change request notification"
    );

    app.cleanup().await;
}
