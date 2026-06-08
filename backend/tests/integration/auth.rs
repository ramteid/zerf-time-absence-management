//! Authentication workflow tests running in a single container for efficiency.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, temp_pw, today};
use zerf::middleware::auth::hash_token;
use zerf::services::auth::hash_password;

#[tokio::test]
async fn auth_full_workflow() {
    let app = TestApp::spawn().await;

    // -- Me payload provides role-shaped view data --
    {
        let admin = admin_login(&app).await;

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
        assert!(nav.contains(&"/admin/settings"));
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
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "error when public_url is missing"
        );
        assert_eq!(
            body["error"], "password_reset_unavailable",
            "generic error code"
        );
    }

    // -- Preferences persist and logout revokes all sessions for the same user --
    {
        // Password was already changed to "AdminPass!234" by the first block.
        let admin = app.client();
        let (st, _) = admin.login("admin@example.com", "AdminPass!234").await;
        assert_eq!(st, StatusCode::OK, "admin login for preferences test");
        let second_session = app.client();
        let (st, _) = second_session
            .login("admin@example.com", "AdminPass!234")
            .await;
        assert_eq!(st, StatusCode::OK, "second admin session login");

        let (st, body) = admin
            .put("/api/v1/auth/preferences", &json!({"dark_mode": true}))
            .await;
        assert_eq!(st, StatusCode::OK, "update preferences");
        assert_eq!(body["ok"], true);

        let (st, me) = admin.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK, "me after preferences update");
        assert_eq!(me["dark_mode"], true, "dark mode persisted");

        let (st, body) = admin.post("/api/v1/auth/logout", &json!({})).await;
        assert_eq!(st, StatusCode::OK, "logout succeeds");
        assert_eq!(body["ok"], true);

        let (st, _) = admin.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "current session revoked");

        let (st, _) = second_session.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "other sessions revoked too");
    }

    app.cleanup().await;
}

/// Expired sessions must be rejected by the auth middleware with 401.
/// This covers the timeout-check branch in `auth_middleware` (lines 245-250) that
/// deletes the session row and returns Unauthorized.
#[tokio::test]
async fn expired_session_returns_unauthorized() {
    let app = TestApp::spawn().await;
    let admin = app.client();
    let (st, _) = admin.login("admin@example.com", &app.admin_password).await;
    assert_eq!(st, StatusCode::OK, "admin login");

    // The admin is authenticated — verify a protected endpoint works.
    let (st, _) = admin.get("/api/v1/auth/me").await;
    assert_eq!(st, StatusCode::OK, "me before session expiry");

    // Manually expire the session by back-dating both timestamps past their
    // respective timeouts in the database.  This simulates what happens after
    // `ABSOLUTE_TIMEOUT_HOURS` (168 h) without a refresh.
    sqlx::query(
        "UPDATE sessions SET \
         created_at = CURRENT_TIMESTAMP - INTERVAL '200 hours', \
         last_active_at = CURRENT_TIMESTAMP - INTERVAL '200 hours'",
    )
    .execute(&app.state.pool)
    .await
    .expect("expire all sessions");

    // The middleware must detect the expired session, delete it, and return 401.
    let (st, _) = admin.get("/api/v1/auth/me").await;
    assert_eq!(
        st,
        StatusCode::UNAUTHORIZED,
        "expired session must be rejected"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn forgot_password_rate_limit_and_generic_responses() {
    let app = TestApp::spawn_with_public_url("https://zerf.example.test").await;

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
    for attempt in 1..=4 {
        let (st, body) = anon
            .post(
                "/api/v1/auth/forgot-password",
                &json!({"email": "admin@example.com"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "forgot-password attempt {attempt}");
        assert_eq!(body["ok"], true, "generic success response");
    }

    let reset_attempts = zerf::repository::SessionDb::new(app.state.pool.clone())
        .count_reset_attempts(
            "reset:admin@example.com",
            chrono::Utc::now() - chrono::Duration::minutes(15),
        )
        .await;
    assert_eq!(reset_attempts, 3, "rate limiter stores at most 3 attempts");

    let token_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id=1")
            .fetch_one(&app.state.pool)
            .await
            .expect("count admin reset tokens");
    assert_eq!(token_count, 1, "tokens are upserted, not multiplied");

    let (st, body) = anon
        .post(
            "/api/v1/auth/forgot-password",
            &json!({"email": "unknown@example.com"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "unknown email response stays generic");
    assert_eq!(body["ok"], true);

    let (st, body) = anon
        .post(
            "/api/v1/auth/forgot-password",
            &json!({"email": "x".repeat(300)}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "oversized email response stays generic");
    assert_eq!(body["ok"], true);

    app.cleanup().await;
}

/// Covers the `create_initial_admin` success path on a fresh database that has
/// no users yet.  Uses `TestApp::spawn_unseeded` to get a completely empty DB so
/// the setup endpoint can actually create the first admin user.
#[tokio::test]
async fn setup_success_creates_admin_on_empty_database() {
    let (app, _) = TestApp::spawn_unseeded().await;
    let anon = app.client();

    // -- setup-status confirms no users exist --
    {
        let (st, body) = anon.get("/api/v1/auth/setup-status").await;
        assert_eq!(st, StatusCode::OK, "setup-status on fresh db");
        assert_eq!(body["needs_setup"], true, "fresh DB needs setup: {body}");
    }

    // -- Valid setup call creates the first admin --
    {
        let (st, body) = anon
            .post(
                "/api/v1/auth/setup",
                &json!({
                    "email": "root@example.com",
                    "password": "SecureAdmin!234",
                    "first_name": "Root",
                    "last_name": "Admin",
                    "tracks_time": false
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "setup succeeds on fresh db: {body}");
        assert_eq!(body["ok"], true, "setup returns ok");
    }

    // -- setup-status now reports setup is done --
    {
        let (st, body) = anon.get("/api/v1/auth/setup-status").await;
        assert_eq!(st, StatusCode::OK, "setup-status after setup");
        assert_eq!(body["needs_setup"], false, "setup is done: {body}");
    }

    // -- The created admin can log in --
    {
        let (st, _) = anon.login("root@example.com", "SecureAdmin!234").await;
        assert_eq!(st, StatusCode::OK, "initial admin can log in");
    }

    app.cleanup().await;
}

/// Exercises the input-validation branches of `services::auth::create_initial_admin`.
///
/// The validation runs BEFORE the DB is consulted for all but the last case, so
/// even a fully-seeded TestApp (where setup is already "done") reaches every
/// early-return branch.
#[tokio::test]
async fn setup_validation_rejects_bad_inputs_and_duplicate_call() {
    let app = TestApp::spawn().await;
    let anon = app.client();

    // -- Invalid email (no @) --
    {
        let (st, body) = anon
            .post(
                "/api/v1/auth/setup",
                &json!({
                    "email": "not-an-email",
                    "password": "ValidPass!234",
                    "first_name": "Alice",
                    "last_name": "Admin",
                    "tracks_time": true
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid email: {body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains("email"),
            "error mentions email: {body}"
        );
    }

    // -- Empty first name --
    {
        let (st, body) = anon
            .post(
                "/api/v1/auth/setup",
                &json!({
                    "email": "admin@setup.example.com",
                    "password": "ValidPass!234",
                    "first_name": "",
                    "last_name": "Admin",
                    "tracks_time": true
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "empty first_name: {body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("name"),
            "error mentions name: {body}"
        );
    }

    // -- Name too long (> 200 chars) --
    {
        let long_name = "x".repeat(201);
        let (st, body) = anon
            .post(
                "/api/v1/auth/setup",
                &json!({
                    "email": "admin@setup.example.com",
                    "password": "ValidPass!234",
                    "first_name": long_name,
                    "last_name": "Admin",
                    "tracks_time": true
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "name too long: {body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("name"),
            "error mentions name: {body}"
        );
    }

    // -- Setup already completed (admin was seeded by TestApp::spawn) --
    // Valid input but the DB already has a user, so the "already done" branch
    // inside the transaction is hit.
    {
        let (st, body) = anon
            .post(
                "/api/v1/auth/setup",
                &json!({
                    "email": "admin2@setup.example.com",
                    "password": "ValidPass!234",
                    "first_name": "Alice",
                    "last_name": "Admin",
                    "tracks_time": true
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "already setup: {body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("setup"),
            "error mentions setup: {body}"
        );
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
