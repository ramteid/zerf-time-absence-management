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
