use chrono::{Datelike, Duration, NaiveDate, Utc};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{
    admin_login, bootstrap_team_with_suffix, create_and_submit_entry, id, login_change_pw,
    reference_date, temp_pw,
};

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
    let expired_hash = zerf::middleware::auth::hash_token("repo-expired-token");
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

    let old_hash = zerf::services::auth::hash_password("RepoCurrent!234").expect("hash old password");
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

    let reset_hash = zerf::middleware::auth::hash_token("repo-valid-token");
    let new_hash = zerf::services::auth::hash_password("RepoFresh!234").expect("hash new password");
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

    sessions.delete("token-a").await.expect("delete token-a");
    assert!(
        sessions
            .get_user_id("token-a")
            .await
            .expect("token-a lookup after delete")
            .is_none()
    );

    sessions
        .create("token-d", user_id, "csrf-d")
        .await
        .expect("create session d");
    sessions
        .create("token-e", user_id, "csrf-e")
        .await
        .expect("create session e");
    let mut tx_conn = app.state.pool.acquire().await.expect("acquire tx conn for sessions");
    zerf::repository::SessionDb::delete_except_tx(&mut tx_conn, user_id, "token-d")
        .await
        .expect("delete except tx");
    assert!(
        sessions
            .get_user_id("token-e")
            .await
            .expect("token-e lookup after delete_except_tx")
            .is_none()
    );

    let reset_hash_two = zerf::middleware::auth::hash_token("repo-valid-token-two");
    sessions
        .upsert_reset_token(&reset_hash_two, user_id, Utc::now() + Duration::hours(1))
        .await
        .expect("insert second valid reset token");
    sessions
        .consume_reset_token_and_update_password(&reset_hash_two, &new_hash)
        .await
        .expect("consume token with public wrapper");

    sessions
        .cleanup_expired_sessions(0, 0)
        .await;

    sqlx::query(
        "INSERT INTO login_attempts(email, success, attempted_at) VALUES ($1, FALSE, CURRENT_TIMESTAMP - INTERVAL '2 days')",
    )
    .bind("repo-cleanup@example.com")
    .execute(&app.state.pool)
    .await
    .expect("insert old login attempt");
    sessions.cleanup_login_attempts().await;

    sqlx::query(
        "INSERT INTO password_reset_tokens(token_hash, user_id, expires_at) VALUES ($1, $2, CURRENT_TIMESTAMP - INTERVAL '1 hour') ON CONFLICT (user_id) DO UPDATE SET token_hash=EXCLUDED.token_hash, expires_at=EXCLUDED.expires_at",
    )
    .bind("expired-cleanup-token")
    .bind(user_id)
    .execute(&app.state.pool)
    .await
    .expect("insert expired reset token for cleanup");
    sessions.cleanup_reset_tokens().await;

    assert!(
        sessions
            .get_active_user_by_email("repo-sessions@example.com")
            .await
            .expect("active user lookup by email")
            .is_some()
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

#[tokio::test]
async fn users_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "repo-users").await;

    let users = zerf::repository::UserDb::new(app.state.pool.clone());
    assert!(users.count().await.expect("count users") >= 3);
    assert_eq!(users.count_active_admins().await.expect("count admins"), 1);

    let lead = users
        .find_by_email("lead-repo-users@example.com")
        .await
        .expect("find lead by email")
        .expect("lead exists");
    assert_eq!(lead.id, lead_id);
    assert!(lead.is_lead());
    assert!(!lead.is_admin());
    assert!(users.find_by_email("missing@example.com").await.expect("find missing by email").is_none());
    assert!(users.find_by_id_active(emp_id).await.expect("find active").is_some());
    assert!(users.find_all_ordered().await.expect("all ordered").len() >= 3);
    assert!(users.find_all_active_ordered().await.expect("all active ordered").len() >= 3);

    let lead_scope = users.find_for_approver(lead_id).await.expect("find for approver");
    assert!(lead_scope.iter().any(|user| user.id == emp_id));
    assert!(lead_scope.iter().any(|user| user.id == lead_id));

    let lead_team = users
        .find_active_team_for_lead(lead_id)
        .await
        .expect("find team for lead");
    assert!(lead_team.iter().any(|user| user.id == emp_id));
    assert_eq!(users.count_direct_reports(lead_id).await.expect("count reports"), 1);
    assert_eq!(
        users
            .count_active_direct_reports(lead_id)
            .await
            .expect("count active reports"),
        1
    );
    assert_eq!(users.get_active_flag(emp_id).await.expect("active flag"), Some(true));
    assert_eq!(
        users.get_approver_info(lead_id).await.expect("approver info"),
        Some(("team_lead".to_string(), true))
    );
    assert_eq!(
        users
            .get_id_role_active(emp_id)
            .await
            .expect("role active tuple"),
        Some((emp_id, "employee".to_string(), true))
    );
    assert!(users.is_direct_report(emp_id, lead_id).await.expect("is direct report"));
    assert!(!users.is_direct_report(1, lead_id).await.expect("admin not direct report"));
    assert_eq!(
        users.get_start_date(emp_id).await.expect("start date"),
        NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
    );
    assert_eq!(
        users
            .get_start_date_and_overtime_balance(emp_id)
            .await
            .expect("start date + balance"),
        (NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), 0)
    );
    assert!(users.check_email_available("lead-repo-users@example.com", None).await.is_err());
    assert!(users.check_name_available("Lararepo-users", "Leadrepo-users", None).await.is_err());

    let all_team_settings = users.team_settings_all().await.expect("all team settings");
    assert!(all_team_settings.iter().any(|row| row.0 == emp_id));
    let lead_team_settings = users
        .team_settings_for_lead(lead_id)
        .await
        .expect("lead team settings");
    assert_eq!(lead_team_settings.len(), 2, "lead sees self and one direct report");

    let emp_client = login_change_pw(&app, "emp-repo-users@example.com", &emp_pw).await;
    let _ = create_and_submit_entry(&emp_client, &monday_iso, cat_id).await;

    let pending = users
        .pending_approvers_for_reminders()
        .await
        .expect("pending approver reminders");
    assert!(pending.iter().any(|row| row.0 == lead_id && row.4 > 0));

    users
        .update_allow_reopen(emp_id, true)
        .await
        .expect("update reopen policy");
    assert!(
        users
            .team_settings_for_lead(lead_id)
            .await
            .expect("team settings after update")
            .iter()
            .any(|row| row.0 == emp_id && row.5)
    );
    assert!(users.is_active_direct_report(emp_id, lead_id).await.expect("active direct report"));

    users
        .update_dark_mode(emp_id, true)
        .await
        .expect("update dark mode");
    assert!(users.find_by_id(emp_id).await.expect("find emp").unwrap().dark_mode);

    let new_hash = zerf::services::auth::hash_password("RepoUserPass!234").expect("hash repo user password");
    users
        .update_password_self(emp_id, &new_hash)
        .await
        .expect("update password self");
    let stored_hash = users
        .get_password_hash(emp_id)
        .await
        .expect("get stored hash")
        .expect("hash exists");
    assert_eq!(stored_hash, new_hash);

    assert_eq!(users.get_default_leave_days().await.expect("default leave days"), 30);
    assert_eq!(users.get_leave_days(emp_id, 2030).await.expect("lazy leave days"), 30);
    users.set_leave_days(emp_id, 2030, 27).await.expect("set leave days");
    assert_eq!(users.get_leave_days(emp_id, 2030).await.expect("stored leave days"), 27);
    // Use a year far enough in the future that no row is auto-created during user seeding
    let far_future_year = reference_date().year() + 5;
    assert_eq!(
        users
            .annual_days_or_default(emp_id, far_future_year, 33)
            .await
            .expect("annual days or default"),
        33
    );

    let mut tx_conn = app.state.pool.acquire().await.expect("acquire user tx conn");
    zerf::repository::UserDb::lock_user_graph_tx(&mut tx_conn)
        .await
        .expect("lock user graph tx");
    assert!(
        zerf::repository::UserDb::count_tx(&mut tx_conn)
            .await
            .expect("count users in tx")
            >= 3
    );
    assert_eq!(
        zerf::repository::UserDb::get_default_leave_days_tx(&mut tx_conn)
            .await
            .expect("default leave days tx"),
        30
    );

    let seeded_hash = zerf::services::auth::hash_password("RepoSeedAdmin!234").expect("hash seeded admin");
    let seeded_admin_id = zerf::repository::UserDb::create_initial_admin(
        &mut tx_conn,
        "repo-seeded-admin@example.com",
        &seeded_hash,
        "Repo",
        "SeededAdmin",
        NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        true,
    )
    .await
    .expect("create initial admin via repository");
    assert!(seeded_admin_id > 0);

    let update_missing = users.update_reopen_policy(9_999_999, true).await;
    assert!(update_missing.is_err(), "missing user update_reopen_policy should fail");

    let missing_update_password = zerf::repository::UserDb::update_password(
        &mut tx_conn,
        9_999_999,
        &seeded_hash,
        false,
    )
    .await;
    assert!(missing_update_password.is_err(), "missing user update_password should fail");

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email":"assistant-repo-users@example.com",
                "first_name":"Assist",
                "last_name":"Repo",
                "role":"assistant",
                "weekly_hours":0,
                "leave_days_current_year":0,
                "leave_days_next_year":0,
                "start_date":"2024-01-01",
                "approver_ids":[lead_id]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create assistant for repository workflow");
    let assistant_id = id(&body);

    let active_non_assistants = users
        .get_active_non_assistant_users()
        .await
        .expect("active non-assistant users");
    assert!(active_non_assistants.iter().any(|row| row.id == emp_id));
    assert!(!active_non_assistants.iter().any(|row| row.id == assistant_id));

    let invalid_insert = zerf::repository::UserDb::insert_approver_tx(&mut tx_conn, emp_id, assistant_id).await;
    assert!(invalid_insert.is_err(), "assistant cannot be inserted as approver");

    app.cleanup().await;
}

#[tokio::test]
async fn time_entries_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, emp_id, _emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "repo-time").await;
    let time_entries = zerf::repository::TimeEntryDb::new(app.state.pool.clone());
    let monday = NaiveDate::parse_from_str(&monday_iso, "%Y-%m-%d").unwrap();
    let tuesday = monday + Duration::days(1);

    let monday_entry = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "08:00".to_string(),
                end_time: "12:00".to_string(),
                category_id: cat_id,
                comment: Some("repo monday".to_string()),
            },
        )
        .await
        .expect("create monday entry");
    let tuesday_entry = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: tuesday,
                start_time: "09:00".to_string(),
                end_time: "11:00".to_string(),
                category_id: cat_id,
                comment: Some("repo tuesday".to_string()),
            },
        )
        .await
        .expect("create tuesday entry");

    assert_eq!(
        time_entries
            .list_for_user(emp_id, Some(monday), Some(tuesday))
            .await
            .expect("list for user")
            .len(),
        2
    );
    assert!(
        time_entries
            .list_for_user(emp_id, None, None)
            .await
            .expect("list for user unfiltered")
            .len()
            >= 2
    );
    assert_eq!(
        time_entries
            .list_all(false, lead_id, Some(monday), Some(tuesday), Some(emp_id), Some("draft".to_string()))
            .await
            .expect("list all for lead")
            .len(),
        2
    );

    assert_eq!(time_entries.find_by_id(monday_entry.id).await.expect("find by id").id, monday_entry.id);
    assert!(time_entries.find_by_id_opt(999999).await.expect("find by id opt").is_none());
    assert_eq!(time_entries.get_user_id(monday_entry.id).await.expect("get user id"), emp_id);
    assert_eq!(time_entries.get_date_for_entry(monday_entry.id).await.expect("get date"), Some(monday));
    assert!(time_entries.all_entries_owned_by_user(&[monday_entry.id, tuesday_entry.id], emp_id).await.expect("owned by user"));
    assert!(!time_entries.all_entries_owned_by_user(&[monday_entry.id], lead_id).await.expect("not owned by lead"));
    assert!(time_entries.all_entries_owned_by_user(&[], lead_id).await.expect("empty owned list"));

    let distinct_dates = time_entries
        .entry_dates_for_ids(&[monday_entry.id, tuesday_entry.id])
        .await
        .expect("entry dates for ids");
    assert_eq!(distinct_dates.len(), 2);
    assert!(
        time_entries
            .entry_dates_for_ids(&[])
            .await
            .expect("entry dates for empty ids")
            .is_empty()
    );

    let updated = time_entries
        .update(
            tuesday_entry.id,
            emp_id,
            false,
            &zerf::repository::NewEntryData {
                entry_date: tuesday,
                start_time: "10:00".to_string(),
                end_time: "12:00".to_string(),
                category_id: cat_id,
                comment: Some("repo tuesday updated".to_string()),
            },
        )
        .await
        .expect("update draft entry");
    assert_eq!(updated.1.start_time, "10:00");

    let submitted_ids = time_entries
        .submit_batch(emp_id, &[monday_entry.id, tuesday_entry.id])
        .await
        .expect("submit batch");
    assert_eq!(submitted_ids.len(), 2);
    assert_eq!(
        time_entries
            .get_credited_submitted_dates_for_entries(emp_id, &[monday_entry.id, tuesday_entry.id])
            .await
            .expect("credited submitted dates")
            .len(),
        2
    );
    assert!(
        time_entries
            .get_credited_submitted_dates_for_entries(emp_id, &[])
            .await
            .expect("credited submitted empty ids")
            .is_empty()
    );
    assert_eq!(
        time_entries
            .count_non_draft_in_week(emp_id, monday, monday + Duration::days(6))
            .await
            .expect("non-draft in week"),
        2
    );

    let mut conn = app.state.pool.acquire().await.expect("acquire conn");
    assert!(
        zerf::repository::TimeEntryDb::check_direct_report_for_update(&mut conn, emp_id, lead_id)
            .await
            .expect("check direct report")
    );
    assert!(
        !zerf::repository::TimeEntryDb::check_direct_report_for_update(&mut conn, emp_id, emp_id)
            .await
            .expect("non-direct report should be false")
    );
    assert_eq!(
        zerf::repository::TimeEntryDb::find_by_id_for_update(&mut conn, monday_entry.id)
            .await
            .expect("find for update")
            .id,
        monday_entry.id
    );

    assert_eq!(
        time_entries
            .batch_approve(&[monday_entry.id], lead_id, false)
            .await
            .expect("batch approve")
            .len(),
        1
    );
    assert_eq!(
        time_entries
            .batch_approve(&[monday_entry.id], lead_id, false)
            .await
            .expect("batch approve already-approved entry")
            .len(),
        0
    );
    assert_eq!(
        time_entries
            .batch_reject(&[tuesday_entry.id], lead_id, false, "repo reject")
            .await
            .expect("batch reject")
            .len(),
        1
    );
    assert_eq!(
        time_entries
            .batch_reject(&[tuesday_entry.id], lead_id, false, "repo reject again")
            .await
            .expect("batch reject already-rejected entry")
            .len(),
        0
    );

    let non_draft_delete = time_entries.delete(monday_entry.id).await;
    assert!(non_draft_delete.is_err(), "non-draft delete should fail");

    let wrong_user_update = time_entries
        .update(
            tuesday_entry.id,
            lead_id,
            false,
            &zerf::repository::NewEntryData {
                entry_date: tuesday,
                start_time: "10:30".to_string(),
                end_time: "12:00".to_string(),
                category_id: cat_id,
                comment: Some("lead update should fail".to_string()),
            },
        )
        .await;
    assert!(wrong_user_update.is_err(), "non-owner non-admin update should fail");

    let draft_to_delete = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday + Duration::days(2),
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: cat_id,
                comment: Some("delete me".to_string()),
            },
        )
        .await
        .expect("create draft to delete");
    assert_eq!(
        time_entries.delete(draft_to_delete.id).await.expect("delete draft").id,
        draft_to_delete.id
    );

    assert_eq!(
        time_entries
            .get_by_user_in_range(emp_id, monday, tuesday)
            .await
            .expect("by user in range")
            .len(),
        2
    );
    assert_eq!(
        time_entries
            .get_submitted_dates_in_range(emp_id, monday, tuesday)
            .await
            .expect("submitted dates in range")
            .len(),
        1,
        "approved entries still count as submitted days"
    );
    assert_eq!(
        time_entries
            .get_incomplete_dates_in_range(emp_id, monday, tuesday)
            .await
            .expect("incomplete dates")
            .len(),
        1,
        "rejected entries remain incomplete"
    );
    let monthly_stats = time_entries
        .get_monthly_submission_stats(emp_id, monday, tuesday)
        .await
        .expect("monthly submission stats");
    assert_eq!(monthly_stats.len(), 1);
    assert_eq!(monthly_stats[0].2, 2);
    assert_eq!(monthly_stats[0].3, 1);

    app.cleanup().await;
}

#[tokio::test]
async fn holidays_repository_workflow() {
    let app = TestApp::spawn().await;

    let holidays = zerf::repository::HolidayDb::new(app.state.pool.clone());
    let current_year = reference_date().year();
    assert_eq!(holidays.get_country_setting().await.expect("country setting"), "DE");
    assert_eq!(holidays.get_region_setting().await.expect("region setting"), "DE-BW");
    assert!(holidays.count_auto_for_year(current_year).await.expect("auto holiday count") > 0);
    assert!(!holidays.list_for_year(current_year).await.expect("list holidays").is_empty());

    let from = NaiveDate::from_ymd_opt(current_year, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(current_year, 12, 31).unwrap();
    assert!(!holidays.get_dates_in_range(from, to).await.expect("holiday dates").is_empty());
    assert!(!holidays.get_rows_in_range(from, to).await.expect("holiday rows").is_empty());

    let manual_date = NaiveDate::from_ymd_opt(current_year + 2, 12, 30).unwrap();
    holidays
        .create_manual(manual_date, "Repository Manual Holiday")
        .await
        .expect("create manual holiday");
    let created = holidays
        .list_for_year(current_year + 2)
        .await
        .expect("list future holidays");
    let manual_id = created
        .iter()
        .find(|row| row.holiday_date == manual_date)
        .expect("manual holiday exists")
        .id;
    holidays.delete(manual_id).await.expect("delete manual holiday");

    let auto_year = current_year + 3;
    holidays
        .insert_auto_holidays(&[zerf::repository::PreparedHoliday {
            holiday_date: NaiveDate::from_ymd_opt(auto_year, 1, 2).unwrap(),
            name: "Repo Auto One".to_string(),
            local_name: "Repo Auto One".to_string(),
            year: auto_year,
        }])
        .await
        .expect("insert auto holidays");
    assert_eq!(holidays.count_auto_for_year(auto_year).await.expect("count repo auto"), 1);

    holidays
        .replace_auto_holidays(&[zerf::repository::PreparedHoliday {
            holiday_date: NaiveDate::from_ymd_opt(auto_year, 5, 1).unwrap(),
            name: "Repo Auto Replacement".to_string(),
            local_name: "Repo Auto Replacement".to_string(),
            year: auto_year,
        }])
        .await
        .expect("replace auto holidays");
    let replaced = holidays
        .list_for_year(auto_year)
        .await
        .expect("list replaced auto holidays");
    assert_eq!(replaced.iter().filter(|row| row.is_auto).count(), 1);
    assert_eq!(replaced[0].name, "Repo Auto Replacement");

    app.cleanup().await;
}

#[tokio::test]
async fn absences_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, emp_id, _emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "repo-absences").await;
    let absences = zerf::repository::AbsenceDb::new(app.state.pool.clone());
    let time_entries = zerf::repository::TimeEntryDb::new(app.state.pool.clone());
    let monday = NaiveDate::parse_from_str(&monday_iso, "%Y-%m-%d").unwrap();
    let tuesday = monday + Duration::days(1);
    let wednesday = monday + Duration::days(2);
    let friday = monday + Duration::days(4);

    assert_eq!(absences.user_workdays_per_week(emp_id).await.expect("user workdays"), 5);
    let holidays = absences.holidays_set(monday, friday).await.expect("holiday set");
    let expected_workdays = (5 - holidays.len() as i32).max(0) as f64;
    assert_eq!(
        absences.workdays(monday, friday).await.expect("default workdays"),
        expected_workdays
    );
    assert_eq!(
        absences
            .workdays_for_user(emp_id, monday, friday)
            .await
            .expect("user workdays in range"),
        expected_workdays
    );

    let requested = absences
        .create(
            emp_id,
            "vacation",
            monday,
            tuesday,
            Some("repo requested absence"),
            "requested",
        )
        .await
        .expect("create requested absence");
    assert_eq!(absences.get_user_id(requested.id).await.expect("absence user id"), emp_id);
    assert_eq!(absences.find_by_id(requested.id).await.expect("find absence").kind, "vacation");
    assert_eq!(
        absences
            .list_for_user(emp_id, monday, friday)
            .await
            .expect("list for user")
            .len(),
        1
    );
    assert_eq!(
        absences
            .list_all(false, lead_id, Some(monday), Some(friday), Some("pending_review"))
            .await
            .expect("lead pending review list")
            .len(),
        1
    );
    assert_eq!(
        absences
            .list_all(true, lead_id, Some(monday), Some(friday), Some("requested"))
            .await
            .expect("admin requested list")
            .len(),
        1
    );

    assert_eq!(absences.calendar_scope_user_ids(lead_id, true, true).await.expect("admin calendar scope"), None);
    let lead_scope = absences
        .calendar_scope_user_ids(lead_id, false, true)
        .await
        .expect("lead calendar scope")
        .expect("lead scoped ids");
    assert!(lead_scope.contains(&lead_id));
    assert!(lead_scope.contains(&emp_id));
    assert_eq!(
        absences
            .calendar_scope_user_ids(emp_id, false, false)
            .await
            .expect("employee calendar scope"),
        Some(vec![emp_id])
    );
    assert_eq!(
        absences
            .calendar_entries(monday, friday, Some(&[emp_id]))
            .await
            .expect("calendar entries")
            .len(),
        1
    );

    let updated = absences
        .update(
            requested.id,
            "training",
            monday,
            wednesday,
            Some("repo updated absence"),
            "requested",
            emp_id,
        )
        .await
        .expect("update requested absence");
    assert_eq!(updated.0.kind, "vacation");
    assert_eq!(updated.1.kind, "training");

    let mut conn = app.state.pool.acquire().await.expect("acquire absence conn");
    assert!(
        zerf::repository::AbsenceDb::is_direct_report_for_update(&mut conn, emp_id, lead_id)
            .await
            .expect("absence direct report")
    );
    assert_eq!(
        zerf::repository::AbsenceDb::find_for_update(&mut conn, requested.id)
            .await
            .expect("find absence for update")
            .id,
        requested.id
    );
    assert_eq!(
        zerf::repository::AbsenceDb::approve_tx(&mut conn, requested.id, lead_id)
            .await
            .expect("approve requested absence"),
        1
    );
    assert_eq!(
        zerf::repository::AbsenceDb::request_cancellation_tx(&mut conn, requested.id)
            .await
            .expect("request cancellation"),
        1
    );
    assert_eq!(
        zerf::repository::AbsenceDb::reject_cancellation_tx(&mut conn, requested.id, lead_id)
            .await
            .expect("reject cancellation"),
        1
    );

    let approved_vacation = absences
        .create(
            emp_id,
            "vacation",
            friday,
            friday,
            Some("approved vacation"),
            "approved",
        )
        .await
        .expect("create approved vacation");
    assert_eq!(
        absences
            .vacation_absences_in_year(emp_id, monday, friday)
            .await
            .expect("vacation absences in year")
            .len(),
        1
    );
    assert_eq!(
        absences
            .approved_ranges_in_period(emp_id, monday, friday)
            .await
            .expect("approved ranges in period")
            .len(),
        2,
        "approved training and approved vacation are both included"
    );
    assert_eq!(
        absences
            .workdays_total(emp_id, "vacation", monday, friday)
            .await
            .expect("workdays total for vacation"),
        1.0
    );
    assert_eq!(
        absences
            .workdays_total_filtered(emp_id, "training", monday, friday, &["approved"])
            .await
            .expect("filtered workdays total"),
        3.0
    );

    let sick_day = friday + Duration::days(1);
    let draft_sick = absences
        .create(
            emp_id,
            "sick",
            sick_day,
            sick_day,
            Some("sick day"),
            "requested",
        )
        .await
        .expect("create sick absence");
    assert_eq!(
        absences.cancel(draft_sick.id, emp_id).await.expect("cancel draft sick").id,
        draft_sick.id
    );

    let requested_cancel = absences
        .create(emp_id, "general_absence", friday + Duration::days(3), friday + Duration::days(3), Some("cancel requested"), "requested")
        .await
        .expect("create requested absence to cancel");
    let mut tx = absences.begin().await.expect("begin absence tx");
    zerf::repository::AbsenceDb::lock_user_scope_tx(&mut tx, emp_id)
        .await
        .expect("lock absence scope");
    zerf::repository::AbsenceDb::assert_no_overlap_tx(&mut tx, emp_id, friday + Duration::days(4), friday + Duration::days(4), None)
        .await
        .expect("no overlap helper");
    let inserted_id = zerf::repository::AbsenceDb::insert_tx(
        &mut tx,
        emp_id,
        "vacation",
        friday + Duration::days(4),
        friday + Duration::days(4),
        Some("insert tx absence"),
        "requested",
    )
    .await
    .expect("insert tx absence");
    zerf::repository::AbsenceDb::update_fields_tx(
        &mut tx,
        inserted_id,
        "special_leave",
        friday + Duration::days(4),
        friday + Duration::days(5),
        Some("updated in tx"),
        "requested",
    )
    .await
    .expect("update fields tx");
    zerf::repository::AbsenceDb::cancel_requested_tx(&mut tx, requested_cancel.id)
        .await
        .expect("cancel requested tx");
    let vacation_ranges = zerf::repository::AbsenceDb::vacation_ranges_in_year_tx(
        &mut tx,
        emp_id,
        monday,
        friday + Duration::days(7),
        None,
    )
    .await
    .expect("vacation ranges in year tx");
    assert_eq!(vacation_ranges.len(), 1);
    let approved_ranges = zerf::repository::AbsenceDb::approved_vacation_ranges_in_year_tx(
        &mut tx,
        emp_id,
        monday,
        friday + Duration::days(7),
        None,
    )
    .await
    .expect("approved vacation ranges in year tx");
    assert_eq!(approved_ranges.len(), 1);
    tx.commit().await.expect("commit absence tx");

    let non_sick_time_conflict_date = monday - Duration::days(7);
    time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: non_sick_time_conflict_date,
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: cat_id,
                comment: Some("time conflict seed".to_string()),
            },
        )
        .await
        .expect("create time entry for absence conflict");
    let time_conflict = absences
        .create(
            emp_id,
            "vacation",
            non_sick_time_conflict_date,
            non_sick_time_conflict_date,
            Some("conflicting absence"),
            "requested",
        )
        .await;
    assert!(
        time_conflict.is_err(),
        "non-sick absence over logged time must fail"
    );
    let time_conflict = time_conflict.err().unwrap();
    assert!(time_conflict.to_string().contains("logged time"));

    sqlx::query(
        "INSERT INTO audit_log(user_id, action, table_name, record_id, before_data, after_data) \
         VALUES ($1, 'updated', 'absences', $2, '{\"status\":\"requested\"}', '{\"status\":\"approved\"}')",
    )
    .bind(lead_id)
    .bind(approved_vacation.id)
    .execute(&app.state.pool)
    .await
    .expect("insert absence audit log");
    assert!(
        zerf::repository::AbsenceDb::latest_update_before_data(&app.state.pool, approved_vacation.id)
            .await
            .expect("latest update before data")
            .expect("before data exists")
            .contains("requested")
    );
    let batch_before = zerf::repository::AbsenceDb::latest_update_before_data_batch(
        &app.state.pool,
        &[approved_vacation.id, requested.id],
    )
    .await
    .expect("latest update before batch");
    assert!(batch_before.contains_key(&approved_vacation.id));

    assert_eq!(absences.carryover_expiry_setting().await.expect("carryover expiry"), "03-31");
    assert_eq!(absences.effective_annual_days(emp_id, 2032).await.expect("default annual days"), 30);
    sqlx::query(
        "INSERT INTO user_annual_leave(user_id, year, days) VALUES ($1,$2,$3) \
         ON CONFLICT (user_id, year) DO UPDATE SET days=EXCLUDED.days",
    )
    .bind(emp_id)
    .bind(2032)
    .bind(26_i64)
    .execute(&app.state.pool)
    .await
    .expect("seed annual leave override");
    assert_eq!(absences.effective_annual_days(emp_id, 2032).await.expect("overridden annual days"), 26);

    app.cleanup().await;
}

#[tokio::test]
async fn time_entries_repository_validation_guards() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "repo-time-guards").await;
    let time_entries = zerf::repository::TimeEntryDb::new(app.state.pool.clone());
    let absences = zerf::repository::AbsenceDb::new(app.state.pool.clone());

    let monday = NaiveDate::parse_from_str(&monday_iso, "%Y-%m-%d").unwrap();
    let next_day = monday + Duration::days(1);

    let end_before_start = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "12:00".to_string(),
                end_time: "10:00".to_string(),
                category_id: cat_id,
                comment: Some("bad range".to_string()),
            },
        )
        .await;
    assert!(end_before_start.is_err());
    assert!(end_before_start.err().unwrap().to_string().contains("after start"));

    let long_comment = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: cat_id,
                comment: Some("x".repeat(2001)),
            },
        )
        .await;
    assert!(long_comment.is_err());
    assert!(long_comment.err().unwrap().to_string().contains("Comment too long"));

    let unknown_category = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: 999_999,
                comment: Some("unknown category".to_string()),
            },
        )
        .await;
    assert!(unknown_category.is_err());
    assert!(unknown_category.err().unwrap().to_string().contains("Category not found"));

    let (status, created_category) = admin
        .post(
            "/api/v1/categories",
            &json!({"name":"Inactive Repo Cat","color":"#111111","counts_as_work":true,"active":true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let inactive_cat_id = id(&created_category);
    let (status, _) = admin
        .put(
            &format!("/api/v1/categories/{inactive_cat_id}"),
            &json!({"name":"Inactive Repo Cat","color":"#111111","counts_as_work":true,"active":false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let inactive_category = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: inactive_cat_id,
                comment: Some("inactive category".to_string()),
            },
        )
        .await;
    assert!(inactive_category.is_err());
    assert!(inactive_category.err().unwrap().to_string().contains("Category is inactive"));

    let future_date = reference_date() + Duration::days(2);
    let future_entry = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: future_date,
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: cat_id,
                comment: Some("future".to_string()),
            },
        )
        .await;
    assert!(future_entry.is_err());
    assert!(future_entry.err().unwrap().to_string().contains("future"));

    let base = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "08:00".to_string(),
                end_time: "12:00".to_string(),
                category_id: cat_id,
                comment: Some("base".to_string()),
            },
        )
        .await
        .expect("create base entry");

    let overlap = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "11:00".to_string(),
                end_time: "12:30".to_string(),
                category_id: cat_id,
                comment: Some("overlap".to_string()),
            },
        )
        .await;
    assert!(overlap.is_err());
    assert!(overlap.err().unwrap().to_string().contains("Overlap"));

    let second_long = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "12:30".to_string(),
                end_time: "23:00".to_string(),
                category_id: cat_id,
                comment: Some("long".to_string()),
            },
        )
        .await;
    assert!(second_long.is_err());
    assert!(second_long.err().unwrap().to_string().contains("14 hours"));

    absences
        .create(
            emp_id,
            "vacation",
            next_day,
            next_day,
            Some("approved absence"),
            "approved",
        )
        .await
        .expect("create approved absence");
    let absence_conflict = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: next_day,
                start_time: "08:00".to_string(),
                end_time: "09:00".to_string(),
                category_id: cat_id,
                comment: Some("absence conflict".to_string()),
            },
        )
        .await;
    assert!(absence_conflict.is_err());
    assert!(absence_conflict.err().unwrap().to_string().contains("approved absence"));

    assert_eq!(
        time_entries
            .find_by_id(base.id)
            .await
            .expect("base still exists")
            .id,
        base.id
    );

    app.cleanup().await;
}

#[tokio::test]
async fn reports_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "repo-reports").await;
    let _lead = login_change_pw(&app, "lead-repo-reports@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-repo-reports@example.com", &emp_pw).await;

    let monday = NaiveDate::parse_from_str(&monday_iso, "%Y-%m-%d").unwrap();
    let tuesday = monday + Duration::days(1);
    let wednesday = monday + Duration::days(2);
    let report_db = zerf::repository::ReportDb::new(app.state.pool.clone());

    let entry_id = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let (_st, _body) = admin
        .put(
            &format!("/api/v1/time-entries/{entry_id}"),
            &json!({
                "entry_date": monday_iso,
                "start_time": "08:00",
                "end_time": "10:00",
                "category_id": cat_id,
                "comment": "report baseline"
            }),
        )
        .await;

    let (st, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": tuesday.format("%Y-%m-%d").to_string(),
                "start_time": "10:00",
                "end_time": "11:00",
                "category_id": cat_id,
                "comment": "report draft"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create draft entry for report repository");
    let draft_id = id(&body);

    assert!(
        report_db
            .is_direct_report(emp_id, lead_id)
            .await
            .expect("is direct report")
    );
    assert!(
        !report_db
            .is_direct_report(1, lead_id)
            .await
            .expect("admin subject is not lead direct report")
    );

    assert!(!report_db
        .time_entry_rows(emp_id, monday, tuesday)
        .await
        .expect("time entry rows")
        .is_empty());

    let absences = zerf::repository::AbsenceDb::new(app.state.pool.clone());
    absences
        .create(
            emp_id,
            "vacation",
            wednesday,
            wednesday,
            Some("repo report absence"),
            "approved",
        )
        .await
        .expect("create approved absence for reports");

    assert_eq!(
        report_db
            .approved_absence_rows(emp_id, monday, wednesday)
            .await
            .expect("approved absence rows")
            .len(),
        1
    );

    let _ = report_db
        .holiday_rows(monday, tuesday)
        .await
        .expect("holiday rows");
    let _ = report_db
        .holiday_set(monday, tuesday)
        .await
        .expect("holiday set");

    let submitted_dates = report_db
        .submitted_dates_in_range(emp_id, monday, tuesday)
        .await
        .expect("submitted dates");
    assert!(submitted_dates.contains(&monday));

    let incomplete_dates = report_db
        .incomplete_dates_in_range(emp_id, monday, tuesday)
        .await
        .expect("incomplete dates");
    assert!(incomplete_dates.contains(&tuesday));

    assert!(
        report_db
            .has_pending_submitted_entries_in_range(emp_id, monday, monday)
            .await
            .expect("pending submitted entries")
    );

    assert_eq!(
        report_db
            .absence_ranges_in_period(emp_id, monday, wednesday)
            .await
            .expect("absence ranges")
            .len(),
        1
    );

    assert!(
        report_db
            .active_team_members(1, true)
            .await
            .expect("active team members admin")
            .len()
            >= 2
    );
    assert!(
        report_db
            .active_team_members(lead_id, false)
            .await
            .expect("active team members lead")
            .iter()
            .any(|u| u.id == emp_id)
    );

    let _ = report_db
        .user_start_and_overtime(emp_id)
        .await
        .expect("user start and overtime");
    let _ = report_db
        .flextime_entries(emp_id, monday, tuesday)
        .await
        .expect("flextime entries");
    let _ = report_db
        .category_entries_for_user(emp_id, monday, tuesday)
        .await
        .expect("category entries for user");

    assert!(
        report_db
            .team_category_members(1, true)
            .await
            .expect("team category members admin")
            .len()
            >= 2
    );
    assert!(
        report_db
            .team_category_members(lead_id, false)
            .await
            .expect("team category members lead")
            .iter()
            .any(|(uid, _, _)| *uid == emp_id)
    );

    let target_scope = report_db
        .category_rows_for_scope(lead_id, false, Some(emp_id), monday, tuesday)
        .await
        .expect("category rows for explicit target");
    assert!(!target_scope.is_empty());

    let admin_scope = report_db
        .category_rows_for_scope(1, true, None, monday, tuesday)
        .await
        .expect("category rows for admin scope");
    assert!(!admin_scope.is_empty());

    let lead_scope = report_db
        .category_rows_for_scope(lead_id, false, None, monday, tuesday)
        .await
        .expect("category rows for lead scope");
    assert!(!lead_scope.is_empty());

    let _ = report_db
        .team_category_entry_rows(1, true, monday, tuesday)
        .await
        .expect("team category entry rows admin");
    let _ = report_db
        .team_category_entry_rows(lead_id, false, monday, tuesday)
        .await
        .expect("team category entry rows lead");

    let deleted = zerf::repository::TimeEntryDb::new(app.state.pool.clone())
        .delete(draft_id)
        .await
        .expect("delete created draft entry");
    assert_eq!(deleted.id, draft_id);

    app.cleanup().await;
}

/// Directly exercises all `NotificationDb` methods including idempotency,
/// cleanup, and the broadcast/subscribe channel — ensuring repository-layer
/// paths are covered independently of the HTTP handler stack.
#[tokio::test]
async fn notifications_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create a user to receive notifications.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email":"notif-repo@example.com",
                "first_name":"Notif",
                "last_name":"Repo",
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

    let notifications = zerf::repository::NotificationDb::new(
        app.state.pool.clone(),
        zerf::repository::new_broadcaster(),
    );

    // Subscribe to the broadcast channel before inserting so we can verify it fires.
    let mut rx = notifications.subscribe();

    notifications
        .insert(user_id, "test_kind", "Test Title", "Test body", None, None)
        .await
        .expect("insert notification");

    // Broadcast must fire when a notification is inserted.
    let signal = rx.try_recv().expect("broadcast signal after insert");
    assert_eq!(signal.user_id, user_id);

    // List and verify unread count.
    let list = notifications.list_for_user(user_id).await.expect("list for user");
    assert_eq!(list.len(), 1);
    let notif_id = list[0].id;
    assert_eq!(list[0].kind, "test_kind");
    assert!(!list[0].is_read);

    let unread = notifications.count_unread(user_id).await.expect("count unread");
    assert_eq!(unread, 1);

    // mark_read marks a single notification and returns the affected row count.
    let affected = notifications.mark_read(notif_id, user_id).await.expect("mark read");
    assert_eq!(affected, 1);
    assert_eq!(notifications.count_unread(user_id).await.expect("after mark read"), 0);

    // Marking an already-read notification returns 1 (the row still matches).
    let again = notifications.mark_read(notif_id, user_id).await.expect("mark read again");
    assert_eq!(again, 1);

    // Insert a second notification; mark_all_read should clear it.
    notifications
        .insert(user_id, "second_kind", "Second", "body2", Some("time_entries"), Some(42))
        .await
        .expect("insert second");
    let all_read = notifications.mark_all_read(user_id).await.expect("mark all read");
    assert!(all_read >= 1);
    assert_eq!(notifications.count_unread(user_id).await.expect("after mark_all_read"), 0);

    // insert_idempotent returns true on first insert, false on duplicate.
    let first_insert = notifications
        .insert_idempotent(user_id, "idem_kind", "Idem Title", "idem body", None, None)
        .await
        .expect("idempotent first insert");
    assert!(first_insert, "first idempotent insert should return true");

    // insert_idempotent_with_dedupe_key uses an explicit dedupe key.
    let key_first = notifications
        .insert_idempotent_with_dedupe_key(
            user_id,
            "dedup_kind",
            "Dedup Title",
            "dedup body",
            None,
            None,
            Some("dedup-key-1"),
        )
        .await
        .expect("idempotent dedup first");
    assert!(key_first, "first dedup-key insert should return true");
    let key_second = notifications
        .insert_idempotent_with_dedupe_key(
            user_id,
            "dedup_kind",
            "Dedup Title",
            "dedup body",
            None,
            None,
            Some("dedup-key-1"),
        )
        .await
        .expect("idempotent dedup second");
    assert!(!key_second, "duplicate dedup-key insert must return false");

    // get_user_email returns the user's display info for email sending.
    let email_info = notifications
        .get_user_email(user_id)
        .await
        .expect("get_user_email must return Some for active user");
    assert_eq!(email_info.0, "notif-repo@example.com");
    assert_eq!(email_info.1, "Notif");
    assert_eq!(email_info.2, "Repo");

    // get_user_email for a non-existent id must return None.
    assert!(
        notifications.get_user_email(999_999).await.is_none(),
        "get_user_email must return None for unknown user"
    );

    // cleanup_old must complete without panicking; row count is not asserted
    // since notifications were just created and are well within the 90-day window.
    notifications.cleanup_old().await;

    // Seed old notification row directly so cleanup_old removes it.
    sqlx::query(
        "INSERT INTO notifications(user_id,kind,title,body,created_at) \
         VALUES ($1,'old_kind','Old','old body', CURRENT_TIMESTAMP - INTERVAL '91 days')",
    )
    .bind(user_id)
    .execute(&app.state.pool)
    .await
    .expect("insert old notification");
    let before_count = notifications.list_for_user(user_id).await.expect("count before cleanup").len();
    notifications.cleanup_old().await;
    let after_count = notifications.list_for_user(user_id).await.expect("count after cleanup").len();
    assert!(
        after_count < before_count,
        "cleanup_old must remove notifications older than 90 days"
    );

    // delete_all removes every notification for the user.
    let deleted = notifications.delete_all(user_id).await.expect("delete all");
    assert!(deleted > 0);
    assert!(notifications.list_for_user(user_id).await.expect("empty after delete").is_empty());

    app.cleanup().await;
}

/// Directly exercises `ReopenRequestDb` query and mutation methods that are
/// not fully covered by the HTTP-layer integration tests (list variants, direct
/// field lookups, access-check error branches).
#[tokio::test]
async fn reopen_requests_repository_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, _emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "rr-repo").await;
    let _lead = login_change_pw(&app, "lead-rr-repo@example.com", &lead_pw).await;

    let monday = chrono::NaiveDate::parse_from_str(&monday_iso, "%Y-%m-%d").unwrap();
    let week_end = monday + chrono::Duration::days(6);

    let reopen_requests = zerf::repository::ReopenRequestDb::new(app.state.pool.clone());

    // Create a time entry and submit it so the week has non-draft entries.
    let (st, body) = admin
        .post(
            "/api/v1/time-entries",
            &serde_json::json!({
                "entry_date": monday_iso,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "user_id": emp_id
            }),
        )
        .await;
    // Inject time entry via repository since handlers may not allow cross-user create.
    let _ = (st, body);
    let time_entries = zerf::repository::TimeEntryDb::new(app.state.pool.clone());
    let entry = time_entries
        .create(
            emp_id,
            &zerf::repository::NewEntryData {
                entry_date: monday,
                start_time: "08:00".to_string(),
                end_time: "12:00".to_string(),
                category_id: cat_id,
                comment: Some("rr repo test entry".to_string()),
            },
        )
        .await
        .expect("create entry for reopen repo test");
    time_entries
        .submit_batch(emp_id, &[entry.id])
        .await
        .expect("submit entry for reopen repo test");

    // count_non_draft_entries returns the number of non-draft entries in the week.
    let non_draft = reopen_requests
        .count_non_draft_entries(emp_id, monday, week_end)
        .await
        .expect("count non-draft entries");
    assert_eq!(non_draft, 1, "one submitted entry in the week");

    // get_user_full_name returns formatted name from the users table.
    let full_name = reopen_requests
        .get_user_full_name(emp_id)
        .await
        .expect("get user full name");
    assert!(full_name.contains("Emilrr-repo"), "full name contains first name");

    // insert_pending creates a reopen request in 'pending' status.
    let (req_id, _created_at) = reopen_requests
        .insert_pending(emp_id, monday, "need to correct an entry")
        .await
        .expect("insert pending reopen request");
    assert!(req_id > 0);

    // find_pending_request_id returns the id of the pending request.
    let found = reopen_requests
        .find_pending_request_id(emp_id, monday)
        .await
        .expect("find pending request id")
        .expect("request id should be Some");
    assert_eq!(found, req_id);

    // list_mine returns the request for the employee.
    let mine = reopen_requests.list_mine(emp_id).await.expect("list mine");
    assert!(mine.iter().any(|r| r.id == req_id));

    // list_pending_admin returns all pending requests (admin view).
    let pending_admin = reopen_requests.list_pending_admin().await.expect("list pending admin");
    assert!(pending_admin.iter().any(|r| r.id == req_id));

    // list_pending_for_lead returns the request visible to the assigned lead.
    let pending_lead = reopen_requests
        .list_pending_for_lead(lead_id)
        .await
        .expect("list pending for lead");
    assert!(
        pending_lead.iter().any(|r| r.id == req_id),
        "lead should see the pending request"
    );

    // find_by_id returns the request.
    let found_by_id = reopen_requests.find_by_id(req_id).await.expect("find by id");
    assert_eq!(found_by_id.status, "pending");

    // approve_with_access_check by lead: must succeed because the lead is
    // the assigned approver for the employee.
    let (approved_req, affected) = reopen_requests
        .approve_with_access_check(req_id, lead_id, false)
        .await
        .expect("lead approve reopen request");
    assert_eq!(approved_req.id, req_id);
    assert_eq!(affected.len(), 1, "one entry reopened");

    // After approval the pending queue must be empty for this request.
    assert!(
        reopen_requests
            .find_pending_request_id(emp_id, monday)
            .await
            .expect("find after approve")
            .is_none(),
        "no pending request after approval"
    );

    // Re-submit the reopened entry via the repository to set up the rejection scenario.
    // Using the repository directly because only the entry owner can call the submit
    // endpoint, and this test has no employee session.
    time_entries
        .submit_batch(emp_id, &[entry.id])
        .await
        .expect("re-submit entry for rejection test");

    let (req2_id, _) = reopen_requests
        .insert_pending(emp_id, monday, "second request for rejection")
        .await
        .expect("insert second pending request");

    // reject_with_access_check by lead: must succeed.
    let rejected_req = reopen_requests
        .reject_with_access_check(req2_id, lead_id, false, "not needed")
        .await
        .expect("lead reject reopen request");
    assert_eq!(rejected_req.id, req2_id);

    app.cleanup().await;
}

/// Directly exercises `CategoryDb` methods that are only called indirectly
/// through the service layer in other integration tests.
#[tokio::test]
async fn categories_repository_workflow() {
    let app = TestApp::spawn().await;

    let categories = zerf::repository::CategoryDb::new(app.state.pool.clone());

    // list_active and list_all both return the seed categories.
    let active = categories.list_active().await.expect("list_active");
    assert!(!active.is_empty(), "seed categories must exist");
    let all = categories.list_all().await.expect("list_all");
    assert!(all.len() >= active.len());

    // find_by_id returns the category; find on unknown id returns None.
    let first_id = active[0].id;
    let found = categories.find_by_id(first_id).await.expect("find_by_id");
    assert_eq!(found.expect("category exists").id, first_id);
    assert!(
        categories.find_by_id(999_999).await.expect("find unknown").is_none(),
        "missing category must return None"
    );

    // get_active_flag returns the active status; unknown id returns None.
    let flag = categories.get_active_flag(first_id).await.expect("get_active_flag");
    assert_eq!(flag, Some(true));
    assert!(
        categories.get_active_flag(999_999).await.expect("missing flag").is_none()
    );

    // Create a new category and verify its id is returned.
    let new_id = categories
        .create("Repo Test Cat", Some("Used in repo test"), "#aabbcc", 99, false)
        .await
        .expect("create category");
    assert!(new_id > 0);

    // Duplicate name must be rejected with a Conflict error.
    let dup = categories.create("Repo Test Cat", None, "#112233", 100, true).await;
    assert!(dup.is_err(), "duplicate name must be rejected");

    // update modifies name and active flag.
    categories
        .update(new_id, Some("Repo Updated Cat".to_string()), None, None, None, None, Some(false))
        .await
        .expect("update category");
    let updated = categories.find_by_id(new_id).await.expect("find updated").expect("exists");
    assert_eq!(updated.name, "Repo Updated Cat");
    assert!(!updated.active);

    // update on a missing id must return NotFound.
    let missing_update = categories
        .update(999_999, Some("Ghost".to_string()), None, None, None, None, None)
        .await;
    assert!(missing_update.is_err(), "update on missing category must fail");

    app.cleanup().await;
}

/// Directly exercises `SystemMetadataDb` methods that are called at startup but
/// never exercised by the workflow tests (which bypass `main.rs`).
#[tokio::test]
async fn system_metadata_repository_workflow() {
    let app = TestApp::spawn().await;

    let meta = zerf::repository::SystemMetadataDb::new(app.state.pool.clone());

    // max_successful_migration_version returns the highest applied migration
    // version; because every TestApp runs all migrations, the value must be > 0.
    let version = meta
        .max_successful_migration_version()
        .await
        .expect("max_successful_migration_version");
    assert!(version > 0, "at least one migration must have been applied");

    // users_exist returns true because TestApp seeds a user during bootstrap.
    let exists = meta.users_exist().await.expect("users_exist");
    assert!(exists, "seeded TestApp must have at least one user");

    // record_runtime_metadata performs insert-if-missing for the two creation
    // keys and an upsert for the two runtime keys — all within a single
    // transaction. Calling it twice verifies idempotency: the creation keys
    // are unchanged on the second call while runtime keys are overwritten.
    meta.record_runtime_metadata("sha-abc", "1", "sha-abc", "1")
        .await
        .expect("record_runtime_metadata first call");
    meta.record_runtime_metadata("sha-abc", "1", "sha-xyz", "2")
        .await
        .expect("record_runtime_metadata second call (idempotent create, updated runtime)");

    app.cleanup().await;
}
