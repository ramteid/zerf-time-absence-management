use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, id, temp_pw};

#[tokio::test]
async fn sessions_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email":"repo-sessions@example.com",
                "first_name":"Repo",
                "last_name":"Sessions",
                "role":"employee",
                "weekly_hours":39,
                "leave_days_current_year":30,
                "leave_days_next_year":30,
                "start_date":"2024-01-01",
                "approver_ids":[1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let user_id = id(&body);
    let _temp = temp_pw(&body);

    let sessions = zerf::repository::SessionDb::new(app.state.pool.clone());
    let since = Utc::now() - Duration::hours(1);
    assert_eq!(
        sessions
            .count_recent_failures("repo-sessions@example.com", since)
            .await
            .expect("count failures"),
        0
    );

    sessions
        .record_attempt("repo-sessions@example.com", false)
        .await
        .expect("record failed attempt");
    sessions
        .record_attempt("repo-sessions@example.com", true)
        .await
        .expect("record successful attempt");
    assert_eq!(
        sessions
            .count_recent_failures("repo-sessions@example.com", since)
            .await
            .expect("count failures after writes"),
        1
    );

    sessions
        .create("token-a", user_id, "csrf-a")
        .await
        .expect("create session a");
    sessions
        .create("token-b", user_id, "csrf-b")
        .await
        .expect("create session b");

    assert_eq!(
        sessions
            .get_user_id("token-a")
            .await
            .expect("session user")
            .expect("user id"),
        user_id
    );
    assert_eq!(
        sessions
            .get_csrf_token("token-a")
            .await
            .expect("csrf token")
            .expect("csrf"),
        "csrf-a"
    );
    let session_info = sessions
        .get_session_info("token-a")
        .await
        .expect("session info")
        .expect("session info row");
    assert_eq!(session_info.user_id, user_id);
    assert_eq!(session_info.csrf_token, "csrf-a");
    assert!(session_info.last_active_at >= session_info.created_at);

    sessions.touch("token-a").await.expect("touch session");
    sessions
        .delete_except(user_id, "token-a")
        .await
        .expect("delete except token-a");
    assert!(
        sessions
            .get_user_id("token-b")
            .await
            .expect("token-b lookup")
            .is_none(),
        "token-b should be removed by delete_except"
    );

    // Create and consume expired token first to hit explicit expired branch.
    let expired_hash = zerf::auth::hash_token("repo-expired-token");
    sessions
        .upsert_reset_token(&expired_hash, user_id, Utc::now() - Duration::minutes(1))
        .await
        .expect("insert expired reset token");
    let expired_err = sessions
        .check_and_consume_expired_token(&expired_hash)
        .await
        .expect_err("expired token should error");
    assert!(
        expired_err.to_string().contains("reset_token_expired"),
        "expired branch should return reset_token_expired"
    );

    let old_hash = zerf::auth::hash_password("RepoCurrent!234").expect("hash old password");
    sqlx::query("UPDATE users SET password_hash=$1, must_change_password=TRUE WHERE id=$2")
        .bind(old_hash)
        .bind(user_id)
        .execute(&app.state.pool)
        .await
        .expect("seed user password state");

    sessions
        .create("token-c", user_id, "csrf-c")
        .await
        .expect("create session c");

    let reset_hash = zerf::auth::hash_token("repo-valid-token");
    let new_hash = zerf::auth::hash_password("RepoFresh!234").expect("hash new password");
    sessions
        .upsert_reset_token(&reset_hash, user_id, Utc::now() + Duration::hours(1))
        .await
        .expect("insert valid reset token");

    sessions
        .consume_reset_token_and_update_password_checked(&reset_hash, &new_hash, Some(&|_| false))
        .await
        .expect("consume valid token and update password");

    let must_change: bool = sqlx::query_scalar("SELECT must_change_password FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&app.state.pool)
        .await
        .expect("load must_change_password");
    assert!(!must_change, "reset flow clears must_change_password");
    assert!(
        sessions
            .get_user_id("token-c")
            .await
            .expect("session lookup after reset")
            .is_none(),
        "password reset must revoke existing sessions"
    );

    sessions
        .record_reset_attempt("repo-reset-key")
        .await;
    assert_eq!(
        sessions
            .count_reset_attempts("repo-reset-key", Utc::now() - Duration::hours(1))
            .await,
        1
    );

    app.cleanup().await;
}

#[tokio::test]
async fn settings_and_metadata_repository_workflow() {
    let app = TestApp::spawn().await;

    let settings = zerf::repository::SettingsDb::new(app.state.pool.clone());
    assert_eq!(
        settings
            .load_setting("non_existing_key", "fallback")
            .await
            .expect("load default"),
        "fallback"
    );

    assert_eq!(
        settings
            .save_setting("ui_language", "de")
            .await
            .expect("save language"),
        "de"
    );
    assert_eq!(settings.load_ui_language_code().await, "de");

    settings
        .save_setting("smtp_enabled", "true")
        .await
        .expect("save smtp_enabled");
    settings
        .save_setting("smtp_host", "smtp.example.com")
        .await
        .expect("save smtp_host");
    settings
        .save_setting("smtp_from", "noreply@example.com")
        .await
        .expect("save smtp_from");
    settings
        .save_setting("smtp_port", "invalid")
        .await
        .expect("save smtp_port");
    settings
        .save_setting("smtp_username", "mailer")
        .await
        .expect("save smtp_username");
    settings
        .save_setting("smtp_password", "secret")
        .await
        .expect("save smtp_password");
    settings
        .save_setting("smtp_encryption", "tls")
        .await
        .expect("save smtp_encryption");

    let smtp = settings.load_smtp_config().await.expect("smtp config");
    assert_eq!(smtp.host, "smtp.example.com");
    assert_eq!(smtp.from, "noreply@example.com");
    assert_eq!(smtp.port, 587, "invalid numeric port falls back to 587");
    assert_eq!(smtp.username.as_deref(), Some("mailer"));
    assert_eq!(smtp.password.as_deref(), Some("secret"));
    assert_eq!(smtp.encryption, "tls");

    let mut conn = app.state.pool.acquire().await.expect("acquire connection");
    let tx_saved = zerf::repository::SettingsDb::save_setting_tx(
        &mut conn,
        "organization_name",
        "Repository Integration Org",
    )
    .await
    .expect("save setting in tx-style call");
    assert_eq!(tx_saved, "Repository Integration Org");
    assert_eq!(
        settings
            .get_raw("organization_name")
            .await
            .expect("get raw org")
            .as_deref(),
        Some("Repository Integration Org")
    );

    let metadata = zerf::repository::SystemMetadataDb::new(app.state.pool.clone());
    let max_version = metadata
        .max_successful_migration_version()
        .await
        .expect("max migration version");
    assert!(max_version > 0, "migrations should be present");
    assert!(metadata.users_exist().await.expect("users_exist"));

    metadata
        .record_runtime_metadata("create-a", "100", "runtime-a", "101")
        .await
        .expect("record runtime metadata first time");
    metadata
        .record_runtime_metadata("create-b", "200", "runtime-b", "201")
        .await
        .expect("record runtime metadata second time");

    let created_git: String =
        sqlx::query_scalar("SELECT value FROM system_metadata WHERE key='database_created_git_commit'")
            .fetch_one(&app.state.pool)
            .await
            .expect("created git key");
    let runtime_git: String =
        sqlx::query_scalar("SELECT value FROM system_metadata WHERE key='runtime_git_commit'")
            .fetch_one(&app.state.pool)
            .await
            .expect("runtime git key");
    assert_eq!(created_git, "create-a", "created keys are insert-only");
    assert_eq!(runtime_git, "runtime-b", "runtime keys are upserted");

    // Re-run db init against the existing migrated DB to exercise startup metadata path.
    let cfg = zerf::config::Config {
        database_url: app.state.cfg.database_url.clone(),
        session_secret: app.state.cfg.session_secret.clone(),
        git_commit: " repo-init-commit ".to_string(),
        bind: app.state.cfg.bind.clone(),
        static_dir: app.state.cfg.static_dir.clone(),
        public_url: app.state.cfg.public_url.clone(),
        allowed_origins: app.state.cfg.allowed_origins.clone(),
        secure_cookies: app.state.cfg.secure_cookies,
        enforce_origin: app.state.cfg.enforce_origin,
        enforce_csrf: app.state.cfg.enforce_csrf,
        trust_proxy: app.state.cfg.trust_proxy,
    };
    let _pool = zerf::db::init(&cfg).await.expect("db init should succeed");

    app.cleanup().await;
}
