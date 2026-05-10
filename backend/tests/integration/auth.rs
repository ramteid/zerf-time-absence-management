//! Authentication workflow tests running in a single container for efficiency.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{temp_pw, today};
use zerf::auth::{hash_password, hash_token};

#[tokio::test]
async fn auth_full_workflow() {
    let app = TestApp::spawn().await;

    // -- Me payload provides role-shaped view data --
    {
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
            "can_view_reports",
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
                    "role":"employee","weekly_hours":39.0,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date": today(), "approver_ids": [1]
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
        assert_eq!(eme["home"], "/dashboard");
        assert_eq!(eme["permissions"]["is_admin"], false);
        assert_eq!(eme["permissions"]["is_lead"], false);
        assert_eq!(eme["permissions"]["can_view_dashboard"], true);
        assert_eq!(eme["permissions"]["can_view_reports"], true);
        assert_eq!(eme["permissions"]["can_approve"], false);
        assert_eq!(eme["permissions"]["can_view_team_reports"], false);

        let nav: Vec<&str> = eme["nav"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["href"].as_str().unwrap())
            .collect();
        assert!(!nav.contains(&"/admin/users"));
        assert!(nav.contains(&"/dashboard"));
        assert!(nav.contains(&"/reports"));
        assert!(nav.contains(&"/time"));
        assert!(nav.contains(&"/account"));
    }

    // -- Public settings are anonymously readable --
    {
        let anon = app.client();
        let (st, body) = anon.get("/api/v1/settings/public").await;
        assert_eq!(st, StatusCode::OK);
        assert!(body["ui_language"].is_string());
    }

    // -- Notification stream requires authentication --
    {
        let anon = app.client();
        let (st, _) = anon.get("/api/v1/notifications/stream").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    // -- Password reset token is single use and rejects current password --
    {
        let current_password = "CurrentPass!234";
        let new_password = "FreshPass!234";
        let replay_password = "ReplayPass!234";
        let token = "reset-token-one";
        let user_id = create_password_reset_user(
            &app,
            "reset-one@example.com",
            "Reset",
            "One",
            current_password,
            true,
        )
        .await;
        insert_reset_token(&app, user_id, token, "1 hour").await;

        let active_session = app.client();
        let (st, _) = active_session
            .login("reset-one@example.com", current_password)
            .await;
        assert_eq!(st, StatusCode::OK, "login before reset");

        let anon = app.client();
        let (st, body) = anon
            .post(
                "/api/v1/auth/reset-password",
                &json!({"token": token, "password": "short"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "weak password rejected");
        assert_eq!(
            body["error"], "Password must be at least 12 characters.",
            "weak-password reset error"
        );

        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=$1")
                .bind(user_id)
                .fetch_one(&app.state.pool)
                .await
                .expect("count reset tokens after weak password");
        assert_eq!(token_count, 1, "token remains usable after weak password");

        let (st, body) = anon
            .post(
                "/api/v1/auth/reset-password",
                &json!({"token": token, "password": current_password}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "current password rejected");
        assert_eq!(
            body["error"], "New password must differ from the current one.",
            "current-password reset error"
        );

        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=$1")
                .bind(user_id)
                .fetch_one(&app.state.pool)
                .await
                .expect("count reset tokens");
        assert_eq!(
            token_count, 1,
            "token remains usable after validation error"
        );

        let (st, body) = anon
            .post(
                "/api/v1/auth/reset-password",
                &json!({"token": token, "password": new_password}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "reset with new password");
        assert_eq!(body["ok"], true, "reset ok payload");

        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=$1")
                .bind(user_id)
                .fetch_one(&app.state.pool)
                .await
                .expect("count reset tokens after success");
        assert_eq!(token_count, 0, "successful reset consumes token");

        let (st, body) = anon
            .post(
                "/api/v1/auth/reset-password",
                &json!({"token": token, "password": replay_password}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "replay rejected");
        assert_eq!(body["error"], "reset_token_invalid", "replay error");

        let (st, _) = active_session.get("/api/v1/auth/me").await;
        assert_eq!(
            st,
            StatusCode::UNAUTHORIZED,
            "existing sessions are revoked"
        );

        let old_login = app.client();
        let (st, _) = old_login
            .login("reset-one@example.com", current_password)
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "old password rejected");

        let new_login = app.client();
        let (st, _) = new_login.login("reset-one@example.com", new_password).await;
        assert_eq!(st, StatusCode::OK, "new password accepted");
    }

    // -- Password reset rejects inactive user and consumes token --
    {
        let token = "reset-token-inactive";
        let user_id = create_password_reset_user(
            &app,
            "reset-inactive@example.com",
            "Reset",
            "Inactive",
            "CurrentPass!234",
            false,
        )
        .await;
        insert_reset_token(&app, user_id, token, "1 hour").await;

        let anon = app.client();
        let (st, body) = anon
            .post(
                "/api/v1/auth/reset-password",
                &json!({"token": token, "password": "FreshPass!234"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "inactive user rejected");
        assert_eq!(body["error"], "reset_token_invalid", "inactive reset error");

        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=$1")
                .bind(user_id)
                .fetch_one(&app.state.pool)
                .await
                .expect("count inactive reset tokens");
        assert_eq!(token_count, 0, "inactive-user token is consumed");
    }

    // -- Expired password reset token is consumed --
    {
        let token = "reset-token-expired";
        let user_id = create_password_reset_user(
            &app,
            "reset-expired@example.com",
            "Reset",
            "Expired",
            "CurrentPass!234",
            true,
        )
        .await;
        insert_reset_token(&app, user_id, token, "-1 hour").await;

        let anon = app.client();
        let (st, body) = anon
            .post(
                "/api/v1/auth/reset-password",
                &json!({"token": token, "password": "short"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "expired token rejected before password validation"
        );
        assert_eq!(body["error"], "reset_token_expired", "expired reset error");

        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=$1")
                .bind(user_id)
                .fetch_one(&app.state.pool)
                .await
                .expect("count expired reset tokens");
        assert_eq!(token_count, 0, "expired token is consumed");
    }

    // -- Password reset tokens are unique per user --
    {
        let user_id = create_password_reset_user(
            &app,
            "reset-unique@example.com",
            "Reset",
            "Unique",
            "CurrentPass!234",
            true,
        )
        .await;
        insert_reset_token(&app, user_id, "reset-token-original", "1 hour").await;

        let duplicate_result = sqlx::query(
            "INSERT INTO password_reset_tokens(token_hash, user_id, expires_at) \
             VALUES ($1, $2, CURRENT_TIMESTAMP + INTERVAL '1 hour')",
        )
        .bind(hash_token("reset-token-duplicate"))
        .bind(user_id)
        .execute(&app.state.pool)
        .await;
        assert!(
            duplicate_result.is_err(),
            "second reset token for one user is rejected"
        );

        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=$1")
                .bind(user_id)
                .fetch_one(&app.state.pool)
                .await
                .expect("count unique reset tokens");
        assert_eq!(token_count, 1, "only one reset token remains");
    }

    // -- Forgot password requires public url when SMTP is enabled --
    {
        sqlx::query(
            "INSERT INTO app_settings(key, value) VALUES \
             ('smtp_enabled', 'true'), \
             ('smtp_host', 'localhost'), \
             ('smtp_from', 'noreply@example.com') \
             ON CONFLICT (key) DO UPDATE SET value=EXCLUDED.value",
        )
        .execute(&app.state.pool)
        .await
        .expect("seed smtp settings");

        let anon = app.client();
        let (st, body) = anon
            .post(
                "/api/v1/auth/forgot-password",
                &json!({"email": "admin@example.com"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "error when public_url is missing");
        assert_eq!(body["error"], "password_reset_unavailable", "generic error code");
    }

    app.cleanup().await;
}


async fn create_password_reset_user(
    app: &TestApp,
    email: &str,
    first_name: &str,
    last_name: &str,
    password: &str,
    active: bool,
) -> i64 {
    let password_hash = hash_password(password).expect("hash reset test password");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users(email, password_hash, first_name, last_name, role, weekly_hours, \
         start_date, active, must_change_password, overtime_start_balance_min) \
         VALUES ($1, $2, $3, $4, 'employee', 39.0, CURRENT_DATE, $5, FALSE, 0) \
         RETURNING id",
    )
    .bind(email)
    .bind(password_hash)
    .bind(first_name)
    .bind(last_name)
    .bind(active)
    .fetch_one(&app.state.pool)
    .await
    .expect("create reset test user");
    sqlx::query("INSERT INTO user_approvers(user_id, approver_id) VALUES ($1, 1)")
        .bind(user_id)
        .execute(&app.state.pool)
        .await
        .expect("link reset test user to admin approver");

    sqlx::query(
        "INSERT INTO user_annual_leave(user_id, year, days) VALUES \
         ($1, EXTRACT(YEAR FROM CURRENT_DATE)::INTEGER, 30), \
         ($1, EXTRACT(YEAR FROM CURRENT_DATE)::INTEGER + 1, 30) \
         ON CONFLICT (user_id, year) DO UPDATE SET days=EXCLUDED.days",
    )
    .bind(user_id)
    .execute(&app.state.pool)
    .await
    .expect("seed reset test user leave days");

    user_id
}

async fn insert_reset_token(app: &TestApp, user_id: i64, token: &str, interval: &str) {
    sqlx::query(
        "INSERT INTO password_reset_tokens(token_hash, user_id, expires_at) \
         VALUES ($1, $2, CURRENT_TIMESTAMP + $3::INTERVAL)",
    )
    .bind(hash_token(token))
    .bind(user_id)
    .bind(interval)
    .execute(&app.state.pool)
    .await
    .expect("insert reset token");
}
