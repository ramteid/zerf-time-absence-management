use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn auto_approve_when_policy_set() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, true).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let _eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

    let (st, body) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "auto reopen");
    assert_eq!(body["status"], "auto_approved");
    assert_eq!(body["entries_reopened"], 1);

    let (st, body) = emp.get("/api/v1/time-entries").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body[0]["status"], "draft");

    let (_, body) = emp.get("/api/v1/notifications").await;
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "reopen_auto_approved"),
        "notification created: {body:?}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn pending_then_approve() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let _eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

    let (st, body) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["status"], "pending");
    let req_id = id(&body);

    // Duplicate pending rejected.
    let (st, _) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate rejected");

    // Lead sees it in their queue.
    let (_, body) = lead.get("/api/v1/reopen-requests/pending").await;
    assert!(has_id(&body, req_id), "lead sees request: {body:?}");

    let (st, body) = lead
        .post(
            &format!("/api/v1/reopen-requests/{}/approve", req_id),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve");
    assert_eq!(body["entries_reopened"], 1);

    let (_, body) = emp.get("/api/v1/time-entries").await;
    assert_eq!(body[0]["status"], "draft");

    let (_, body) = emp.get("/api/v1/notifications").await;
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["kind"] == "reopen_approved"));

    app.cleanup().await;
}

#[tokio::test]
async fn pending_then_reject() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

    let (_, body) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    let req_id = id(&body);

    // Reject without reason -> 400.
    let (st, _) = lead
        .post(
            &format!("/api/v1/reopen-requests/{}/reject", req_id),
            &json!({"reason": ""}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _) = lead
        .post(
            &format!("/api/v1/reopen-requests/{}/reject", req_id),
            &json!({"reason": "Not necessary"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);

    let (_, body) = emp.get("/api/v1/time-entries").await;
    assert_eq!(body[0]["status"], "submitted", "entry stays submitted");
    let _ = eid;

    let (_, body) = emp.get("/api/v1/notifications").await;
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["kind"] == "reopen_rejected"));

    app.cleanup().await;
}

#[tokio::test]
async fn empty_week_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, _cat_id) =
        bootstrap_team(&app, &admin, true).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let (st, body) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "empty week rejected");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("nothing"));
    app.cleanup().await;
}

#[tokio::test]
async fn not_monday_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team(&app, &admin, true).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let tuesday = (next_monday(-14) + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let (st, _) = emp
        .post("/api/v1/reopen-requests", &json!({"week_start": tuesday}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "tuesday rejected");
    app.cleanup().await;
}

#[tokio::test]
async fn cancels_open_change_requests() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, true).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

    let (st, body) = emp
        .post(
            "/api/v1/change-requests",
            &json!({"time_entry_id": eid, "reason": "fix typo", "new_comment": "edited"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let cr_id = id(&body);

    let (st, _) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);

    let (_, body) = emp.get("/api/v1/change-requests").await;
    let cr = find_by_id(&body, cr_id).expect("cr present");
    assert_eq!(cr["status"], "rejected");
    assert!(cr["rejection_reason"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("auto"));
    app.cleanup().await;
}

#[tokio::test]
async fn lead_self_service() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"solo@example.com","first_name":"Sol","last_name":"O",
                "role":"team_lead","weekly_hours":39,"annual_leave_days":30,
                "start_date":"2024-01-01"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let pw = temp_pw(&body);
    let lead = login_change_pw(&app, "solo@example.com", &pw).await;
    let (_, body) = admin.get("/api/v1/categories").await;
    let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let monday_iso = next_monday(-14).format("%Y-%m-%d").to_string();
    let _ = create_and_submit_entry(&lead, &monday_iso, cat_id).await;

    let (st, body) = lead
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "lead self-service auto");
    assert_eq!(body["status"], "auto_approved");

    app.cleanup().await;
}
