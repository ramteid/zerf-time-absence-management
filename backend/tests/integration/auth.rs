use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{temp_pw, today};

#[tokio::test]
async fn me_payload_provides_role_shaped_view_data() {
    let app = TestApp::spawn().await;
    let admin = app.client();
    let (st, _) = admin.login("admin@example.com", &app.admin_password).await;
    assert_eq!(st, StatusCode::OK, "admin login");

    let (st, me) = admin.get("/api/v1/auth/me").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["role"], "admin");
    assert_eq!(me["home"], "/dashboard");

    let perms = &me["permissions"];
    for key in [
        "is_admin",
        "is_lead",
        "can_manage_users",
        "can_manage_categories",
        "can_manage_holidays",
        "can_view_audit_log",
        "can_manage_settings",
        "can_approve",
        "can_view_team_reports",
        "can_view_dashboard",
    ] {
        assert_eq!(perms[key], serde_json::Value::Bool(true), "{key} for admin");
    }

    let nav: Vec<&str> = me["nav"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["href"].as_str().unwrap())
        .collect();
    assert!(nav.contains(&"/admin/users"));
    assert!(nav.contains(&"/dashboard"));
    assert!(nav.contains(&"/reports"));

    // Employee gets a reduced payload.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email":"emp-me@example.com","first_name":"E","last_name":"M",
                "role":"employee","weekly_hours":39.0,"annual_leave_days":30,
                "start_date": today(), "approver_id": 1
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let pw = temp_pw(&body);

    let emp = app.client();
    let (st, _) = emp.login("emp-me@example.com", &pw).await;
    assert_eq!(st, StatusCode::OK);
    let (_, eme) = emp.get("/api/v1/auth/me").await;
    assert_eq!(eme["role"], "employee");
    assert_eq!(eme["home"], "/time");
    assert_eq!(eme["permissions"]["is_admin"], false);
    assert_eq!(eme["permissions"]["is_lead"], false);
    assert_eq!(eme["permissions"]["can_view_dashboard"], false);

    let nav: Vec<&str> = eme["nav"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["href"].as_str().unwrap())
        .collect();
    assert!(!nav.contains(&"/admin/users"));
    assert!(!nav.contains(&"/dashboard"));
    assert!(nav.contains(&"/time"));
    assert!(nav.contains(&"/account"));

    app.cleanup().await;
}

#[tokio::test]
async fn public_settings_are_anonymously_readable() {
    let app = TestApp::spawn().await;
    let anon = app.client();
    let (st, body) = anon.get("/api/v1/settings/public").await;
    assert_eq!(st, StatusCode::OK);
    assert!(body["ui_language"].is_string());
    app.cleanup().await;
}

#[tokio::test]
async fn notification_stream_requires_authentication() {
    let app = TestApp::spawn().await;
    let anon = app.client();
    let (st, _) = anon.get("/api/v1/notifications/stream").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    app.cleanup().await;
}
