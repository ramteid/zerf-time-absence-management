use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn invalid_category_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, monday, cat) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let eid = create_and_submit_entry(&emp, &monday, cat).await;

    let (st, _) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid,
                "new_category_id": 999_999_i64,
                "reason": "wrong category",
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "change request with nonexistent category -> 400"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn noop_change_request_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, _lead_pw, _emp_id, emp_pw, monday, cat) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let eid = create_and_submit_entry(&emp, &monday, cat).await;

    let (st, _) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid,
                "reason": "please fix"
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "change request without any effective edit -> 400"
    );

    let (st, _) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid,
                "new_date": monday,
                "new_start_time": "08:00",
                "new_end_time": "12:00",
                "new_category_id": cat,
                "new_comment": "work",
                "reason": "same values"
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "equivalent change request -> 400"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn approval_overlap_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday, cat) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // Entry A: 08:00-12:00 -- submitted and approved.
    let eid_a = create_and_submit_entry(&emp, &monday, cat).await;
    let (st, _) = lead
        .post(
            &format!("/api/v1/time-entries/{}/approve", eid_a),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve entry A");

    // Entry B: 13:00-17:00 -- submitted and approved.
    let (st, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday,
                "start_time": "13:00",
                "end_time": "17:00",
                "category_id": cat,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create entry B");
    let eid_b = id(&body);
    let (st, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": [eid_b]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit entry B");
    let (st, _) = lead
        .post(
            &format!("/api/v1/time-entries/{}/approve", eid_b),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve entry B");

    // Change request for B: shift start to 09:00, overlapping with A (08:00-12:00).
    let (st, cr_body) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid_b,
                "new_start_time": "09:00",
                "reason": "came in early",
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create overlapping change request");
    let cr_id = id(&cr_body);

    let (st, _) = lead
        .post(
            &format!("/api/v1/change-requests/{}/approve", cr_id),
            &json!({}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "approving overlapping change request -> 400"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn approval_rejects_request_after_entry_is_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday, cat) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let eid = create_and_submit_entry(&emp, &monday, cat).await;
    let (st, body) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid,
                "new_start_time": "09:00",
                "reason": "schedule changed"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create change request");
    let cr_id = id(&body);

    let (st, _) = lead
        .post(
            &format!("/api/v1/time-entries/{}/reject", eid),
            &json!({"reason": "incorrect"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject target entry");

    let (st, _) = lead
        .post(
            &format!("/api/v1/change-requests/{}/approve", cr_id),
            &json!({}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "stale change request for rejected entry -> 400"
    );

    let (st, body) = emp
        .get(&format!(
            "/api/v1/time-entries?from={}&to={}",
            monday, monday
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "load rejected entry");
    let entry = body
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == eid)
        .unwrap();
    assert_eq!(entry["status"], "rejected");
    assert_eq!(entry["start_time"], "08:00");

    app.cleanup().await;
}

#[tokio::test]
async fn requested_changes_are_applied_on_approval() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday, cat) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let eid = create_and_submit_entry(&emp, &monday, cat).await;
    let (st, _) = lead
        .post(&format!("/api/v1/time-entries/{}/approve", eid), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "approve original entry");

    let (st, body) = emp
        .post(
            "/api/v1/change-requests",
            &json!({
                "time_entry_id": eid,
                "new_start_time": "09:00",
                "new_end_time": "13:00",
                "new_comment": "shifted work",
                "reason": "schedule changed"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create effective change request");
    let cr_id = id(&body);

    let (st, _) = lead
        .post(
            &format!("/api/v1/change-requests/{}/approve", cr_id),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve change request");

    let (st, body) = emp
        .get(&format!(
            "/api/v1/time-entries?from={}&to={}",
            monday, monday
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "load updated entry");
    let entry = body
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == eid)
        .unwrap();
    assert_eq!(entry["start_time"], "09:00");
    assert_eq!(entry["end_time"], "13:00");
    assert_eq!(entry["comment"], "shifted work");
    assert_eq!(entry["status"], "approved");

    app.cleanup().await;
}
