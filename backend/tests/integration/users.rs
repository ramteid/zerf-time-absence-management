use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn non_admin_users_must_have_approver() {
    let app = TestApp::spawn().await;
    let admin = app.client();
    let (st, _) = admin.login("admin@example.com", &app.admin_password).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = admin
        .change_password(&app.admin_password, "AdminPass!234")
        .await;
    assert_eq!(st, StatusCode::OK);

    // Missing approver_id is rejected for employees.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"a@example.com","first_name":"A","last_name":"A",
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "missing approver rejected");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("approver"));

    // Missing approver_id is rejected for team leads.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"lead-missing@example.com","first_name":"Lead","last_name":"Missing",
                "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01"}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "missing team lead approver rejected"
    );
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
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_id": 1}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "with approver works");

    // Team leads may report to another explicit team lead.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"lead-approver@example.com","first_name":"Lead","last_name":"Approver",
                "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_id":1}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create team lead approver");
    let lead_approver_id = id(&body);

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"lead-report@example.com","first_name":"Lead","last_name":"Report",
                "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_id":lead_approver_id}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create team lead with lead approver");
    assert_eq!(body["user"]["approver_id"], lead_approver_id);
    let lead_report_id = id(&body);

    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{lead_report_id}"),
            &json!({"approver_id": null}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "clearing team lead approver is rejected"
    );
    assert!(body["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("approver"));

    // Approver pointing at non-existent user.
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"c@example.com","first_name":"C","last_name":"C",
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_id": 99999}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "missing approver row rejected");

    app.cleanup().await;
}

#[tokio::test]
async fn duplicate_user_identifiers_are_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "unique@example.com",
                "first_name": "Unique",
                "last_name": "Person",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_id": 1,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create baseline user");
    let baseline_id = id(&body);

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "unique@example.com",
                "first_name": "Different",
                "last_name": "Person",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_id": 1,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate email rejected");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Email already exists."));

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "same-name@example.com",
                "first_name": " Unique ",
                "last_name": " Person ",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_id": 1,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate full name rejected");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("First name and last name already exist."));

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "other@example.com",
                "first_name": "Other",
                "last_name": "Person",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_id": 1,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create second user");
    let second_id = id(&body);

    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{second_id}"),
            &json!({"first_name": "Unique", "last_name": "Person"}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::CONFLICT,
        "duplicate full name update rejected"
    );
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("First name and last name already exist."));

    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{baseline_id}"),
            &json!({"first_name": " Unique ", "last_name": " Person "}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "updating same user with trimmed name works"
    );
    assert_eq!(body["first_name"], "Unique");
    assert_eq!(body["last_name"], "Person");

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
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_id": 1,
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
                "leave_days_current_year": 30, "leave_days_next_year": 30,
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
