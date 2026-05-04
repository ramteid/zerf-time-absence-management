use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn employee_must_have_approver() {
    let app = TestApp::spawn().await;
    let admin = app.client();
    let (st, _) = admin.login("admin@example.com", &app.admin_password).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = admin
        .change_password(&app.admin_password, "AdminPass!234")
        .await;
    assert_eq!(st, StatusCode::OK);

    // Missing approver_id is rejected.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"a@example.com","first_name":"A","last_name":"A",
                "role":"employee","weekly_hours":39,"annual_leave_days":30,
                "start_date":"2024-01-01"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "missing approver rejected");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("approver"));

    // Approver = admin works.
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"b@example.com","first_name":"B","last_name":"B",
                "role":"employee","weekly_hours":39,"annual_leave_days":30,
                "start_date":"2024-01-01","approver_id": 1}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "with approver works");

    // Approver pointing at non-existent user.
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"c@example.com","first_name":"C","last_name":"C",
                "role":"employee","weekly_hours":39,"annual_leave_days":30,
                "start_date":"2024-01-01","approver_id": 99999}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "missing approver row rejected");

    app.cleanup().await;
}

#[tokio::test]
async fn creation_password_modes_set_must_change_correctly() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let manual_password = "ManualPass!234";
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "manual@example.com",
                "first_name": "Manual",
                "last_name": "User",
                "role": "team_lead",
                "weekly_hours": 39,
                "annual_leave_days": 30,
                "start_date": "2024-01-01",
                "password": manual_password,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user with manual password");
    assert_eq!(body["temporary_password"], manual_password);
    assert_eq!(body["user"]["must_change_password"], true);

    let manual = app.client();
    let (st, _) = manual.login("manual@example.com", manual_password).await;
    assert_eq!(st, StatusCode::OK, "manual password login");
    let (st, body) = manual.get("/api/v1/auth/me").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["must_change_password"], true);

    let generated_password = "GeneratedPass!234";
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "generated@example.com",
                "first_name": "Generated",
                "last_name": "User",
                "role": "employee",
                "weekly_hours": 39,
                "annual_leave_days": 30,
                "start_date": "2024-01-01",
                "approver_id": 1,
                "password": generated_password,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "create user with explicit password always requires change"
    );
    assert_eq!(body["temporary_password"], generated_password);
    assert_eq!(body["user"]["must_change_password"], true);

    let generated = app.client();
    let (st, _) = generated
        .login("generated@example.com", generated_password)
        .await;
    assert_eq!(st, StatusCode::OK, "generated password login");
    let (st, body) = generated.get("/api/v1/auth/me").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["must_change_password"], true);

    app.cleanup().await;
}
