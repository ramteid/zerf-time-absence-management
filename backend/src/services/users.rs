use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::repository::{SessionDb, UserDb};
use crate::roles::{
    is_admin_role, is_assistant_role, is_team_lead_role, normalize_role, ROLE_ASSISTANT,
};
use crate::AppState;
use std::collections::HashSet;

pub struct NewUser {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub weekly_hours: f64,
    pub workdays_per_week: Option<i16>,
    pub leave_days_current_year: i64,
    pub leave_days_next_year: i64,
    pub start_date: chrono::NaiveDate,
    pub hire_date: Option<chrono::NaiveDate>,
    pub overtime_start_balance_min: Option<i64>,
    pub password: Option<String>,
    pub approver_ids: Vec<i64>,
    pub tracks_time: bool,
}

pub struct CreateResponse {
    pub id: i64,
    pub user: User,
    pub temporary_password: String,
}

pub fn repo_user_to_auth_user(u: crate::repository::User) -> User {
    User {
        id: u.id,
        email: u.email,
        password_hash: u.password_hash,
        first_name: u.first_name,
        last_name: u.last_name,
        role: u.role,
        weekly_hours: u.weekly_hours,
        workdays_per_week: u.workdays_per_week,
        start_date: u.start_date,
        hire_date: u.hire_date,
        active: u.active,
        must_change_password: u.must_change_password,
        created_at: u.created_at,
        allow_reopen_without_approval: u.allow_reopen_without_approval,
        allow_submission_without_approval: u.allow_submission_without_approval,
        dark_mode: u.dark_mode,
        overtime_start_balance_min: u.overtime_start_balance_min,
        tracks_time: u.tracks_time,
    }
}

/// Return `Forbidden` when the requesting user has time tracking disabled.
/// This is the canonical implementation — service-level copies delegate here.
pub fn require_tracks_time(user: &User) -> AppResult<()> {
    if !user.tracks_time {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn assert_can_access_user(
    app_state: &AppState,
    requester: &User,
    target_id: i64,
) -> AppResult<()> {
    if requester.is_admin() || requester.id == target_id {
        return Ok(());
    }
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let is_report = app_state
        .db
        .users
        .is_direct_report(target_id, requester.id)
        .await?;
    if !is_report {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Validate that each approver_id refers to an active lead/admin and is not the user themselves.
/// Also enforces the rule that non-admin users must have at least one approver.
pub async fn validate_approver_ids(
    app_state: &AppState,
    role: &str,
    user_self_id: Option<i64>,
    approver_ids: &[i64],
) -> AppResult<()> {
    let mut seen = HashSet::new();
    for approver_id in approver_ids {
        if !seen.insert(*approver_id) {
            return Err(AppError::BadRequest(
                "Approver list contains duplicates.".into(),
            ));
        }
    }
    if !is_admin_role(role) && approver_ids.is_empty() {
        return Err(AppError::BadRequest(
            "An approver is required for non-admin users.".into(),
        ));
    }
    for aid in approver_ids {
        if Some(*aid) == user_self_id {
            return Err(AppError::BadRequest(
                "Approver cannot be the user themselves.".into(),
            ));
        }
        let approver_row = app_state.db.users.get_approver_info(*aid).await?;
        match approver_row {
            None => return Err(AppError::BadRequest("Approver not found.".into())),
            Some((approver_role, true))
                if is_admin_role(&approver_role)
                    || (!is_admin_role(role) && is_team_lead_role(&approver_role)) => {}
            Some(_) => {
                return Err(AppError::BadRequest(if is_admin_role(role) {
                    "Admins may only report to an active Admin.".into()
                } else {
                    "Approver must be an active Team lead or Admin.".into()
                }))
            }
        }
    }
    Ok(())
}

pub fn normalize_user_name(first_name: &str, last_name: &str) -> AppResult<(String, String)> {
    let first_name = first_name.trim().to_string();
    let last_name = last_name.trim().to_string();
    if first_name.is_empty()
        || last_name.is_empty()
        || first_name.len() > 200
        || last_name.len() > 200
    {
        return Err(AppError::BadRequest("Invalid name.".into()));
    }
    Ok((first_name, last_name))
}

pub fn normalize_optional_user_name(name: Option<&String>) -> AppResult<Option<String>> {
    let Some(value) = name else { return Ok(None) };
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return Err(AppError::BadRequest("Invalid name.".into()));
    }
    Ok(Some(trimmed))
}

pub async fn ensure_email_available(
    app_state: &AppState,
    email: &str,
    excluded_user_id: Option<i64>,
) -> AppResult<()> {
    app_state
        .db
        .users
        .check_email_available(email, excluded_user_id)
        .await
}

pub async fn ensure_user_name_available(
    app_state: &AppState,
    first_name: &str,
    last_name: &str,
    excluded_user_id: Option<i64>,
) -> AppResult<()> {
    app_state
        .db
        .users
        .check_name_available(first_name, last_name, excluded_user_id)
        .await
}

pub fn user_unique_conflict(error: &crate::db::SqlxError) -> Option<AppError> {
    let crate::db::SqlxError::Database(db_error) = error else {
        return None;
    };
    match db_error.constraint() {
        Some("users_email_key") => Some(AppError::Conflict("Email already exists.".into())),
        Some("idx_users_first_last_name_unique") => Some(AppError::Conflict(
            "First name and last name already exist.".into(),
        )),
        _ if db_error.code().as_deref() == Some("23505") && db_error.table() == Some("users") => {
            Some(AppError::Conflict("User already exists.".into()))
        }
        _ => None,
    }
}

/// Get the leave days for `user_id` in `year`.
/// If no row exists yet, one is created lazily using the global default.
pub async fn get_leave_days(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    year: i32,
) -> AppResult<i64> {
    let db = UserDb::new(pool.clone());
    db.get_leave_days(user_id, year).await
}

pub async fn fetch_for_update(tx: &mut crate::db::PgConnection, user_id: i64) -> AppResult<User> {
    UserDb::fetch_for_update(tx, user_id)
        .await
        .map(repo_user_to_auth_user)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_repo_user(
    tx: &mut crate::db::PgConnection,
    email: &str,
    password_hash: &str,
    first_name: &str,
    last_name: &str,
    role: &str,
    weekly_hours: f64,
    workdays_per_week: i16,
    start_date: chrono::NaiveDate,
    hire_date: Option<chrono::NaiveDate>,
    overtime_start_balance_min: i64,
    tracks_time: bool,
) -> Result<i64, crate::db::SqlxError> {
    UserDb::create(
        tx,
        email,
        password_hash,
        first_name,
        last_name,
        role,
        weekly_hours,
        workdays_per_week,
        start_date,
        hire_date,
        true,
        overtime_start_balance_min,
        tracks_time,
    )
    .await
}

pub async fn insert_approver_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    approver_id: i64,
) -> AppResult<()> {
    UserDb::insert_approver_tx(tx, user_id, approver_id).await
}

pub async fn set_leave_days_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    year: i32,
    days: i64,
) -> AppResult<()> {
    UserDb::set_leave_days_tx(tx, user_id, year, days).await
}

pub async fn get_approver_ids_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
) -> AppResult<Vec<i64>> {
    UserDb::get_approver_ids_tx(tx, user_id).await
}

pub async fn count_active_admins_tx(tx: &mut crate::db::PgConnection) -> AppResult<i64> {
    UserDb::count_active_admins_tx(tx).await
}

pub async fn count_active_direct_reports_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
) -> AppResult<i64> {
    UserDb::count_active_direct_reports_tx(tx, user_id).await
}

pub async fn delete_time_data_for_user_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
) -> AppResult<()> {
    UserDb::delete_time_data_for_user_tx(tx, user_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn update_basic_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    email: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    role: Option<String>,
    weekly_hours: Option<f64>,
    workdays_per_week: Option<i16>,
    start_date: Option<chrono::NaiveDate>,
    hire_date: Option<Option<chrono::NaiveDate>>,
    active: Option<bool>,
    allow_reopen_without_approval: Option<bool>,
    allow_submission_without_approval: Option<bool>,
    overtime_start_balance_min: Option<i64>,
    tracks_time: Option<bool>,
) -> Result<(), crate::db::SqlxError> {
    UserDb::update_basic(
        tx,
        user_id,
        email,
        first_name,
        last_name,
        role,
        weekly_hours,
        workdays_per_week,
        start_date,
        hire_date,
        active,
        allow_reopen_without_approval,
        allow_submission_without_approval,
        overtime_start_balance_min,
        tracks_time,
    )
    .await
}

pub async fn set_approvers_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    approver_ids: &[i64],
) -> AppResult<()> {
    UserDb::set_approvers_tx(tx, user_id, approver_ids).await
}

pub async fn delete_sessions_for_user_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
) -> AppResult<()> {
    SessionDb::delete_for_user_tx(tx, user_id).await
}

pub async fn deactivate_tx(tx: &mut crate::db::PgConnection, user_id: i64) -> AppResult<()> {
    UserDb::deactivate_tx(tx, user_id).await
}

pub async fn delete_tx(tx: &mut crate::db::PgConnection, user_id: i64) -> AppResult<()> {
    UserDb::delete_tx(tx, user_id).await
}

pub async fn update_password_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    password_hash: &str,
    must_change_password: bool,
) -> AppResult<()> {
    UserDb::update_password(tx, user_id, password_hash, must_change_password).await
}

/// Generate a 16-char temporary password with at least one of each class
/// (lower / upper / digit / symbol) so it satisfies the strength policy.
/// Uses the OS CSPRNG (`SysRng`) — never the thread RNG — for security.
/// Uses rejection sampling to avoid modulo bias.
pub fn generate_password() -> String {
    use rand::rand_core::{Rng, UnwrapErr};
    use rand::rngs::SysRng;
    use rand::seq::SliceRandom;
    let lower_chars: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    let upper_chars: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let digit_chars: &[u8] = b"23456789";
    // Avoid characters that may confuse shells / JSON / URLs when copy-pasted:
    // backslash, quotes, $, &, ?, =, %, /
    let symbol_chars: &[u8] = b"!@#*-_+";
    let character_pools = [lower_chars, upper_chars, digit_chars, symbol_chars];
    let mut rng = UnwrapErr(SysRng);

    // Pick one character from a pool using rejection sampling to avoid modulo bias.
    let pick_from = |rng: &mut UnwrapErr<SysRng>, pool: &[u8]| -> u8 {
        let len = pool.len();
        let limit = 256 - (256 % len);
        loop {
            let mut buf = [0u8; 1];
            rng.fill_bytes(&mut buf);
            let value = buf[0] as usize;
            if value < limit {
                return pool[value % len];
            }
        }
    };

    let mut password_bytes: Vec<u8> = character_pools
        .iter()
        .map(|pool| pick_from(&mut rng, pool))
        .collect();
    let all_chars: Vec<u8> = character_pools
        .iter()
        .flat_map(|pool| pool.iter().copied())
        .collect();
    while password_bytes.len() < 16 {
        password_bytes.push(pick_from(&mut rng, &all_chars));
    }
    password_bytes.shuffle(&mut rng);
    String::from_utf8(password_bytes).unwrap()
}

pub async fn team_settings_update(
    app_state: &AppState,
    requester: &User,
    target_id: i64,
    allow_reopen_without_approval: bool,
    allow_submission_without_approval: bool,
) -> AppResult<()> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() && target_id == requester.id {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() {
        let is_report = app_state
            .db
            .users
            .is_direct_report(target_id, requester.id)
            .await?;
        if !is_report {
            return Err(AppError::Forbidden);
        }
    }
    let mut tx = app_state.db.users.begin().await?;
    let previous_user = UserDb::fetch_for_update(&mut tx, target_id).await?;
    if !previous_user.active {
        return Err(AppError::BadRequest("User not found or inactive.".into()));
    }
    UserDb::update_basic(
        &mut tx,
        target_id,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(allow_reopen_without_approval),
        Some(allow_submission_without_approval),
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "team_settings_updated",
        "users",
        target_id,
        Some(serde_json::json!({
            "allow_reopen_without_approval": previous_user.allow_reopen_without_approval,
            "allow_submission_without_approval": previous_user.allow_submission_without_approval,
        })),
        Some(serde_json::json!({
            "allow_reopen_without_approval": allow_reopen_without_approval,
            "allow_submission_without_approval": allow_submission_without_approval,
        })),
    )
    .await;
    Ok(())
}

pub async fn create(
    app_state: &AppState,
    requester: &User,
    mut body: NewUser,
) -> AppResult<CreateResponse> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    body.role = normalize_role(&body.role);
    if !["employee", "team_lead", "admin", ROLE_ASSISTANT].contains(&body.role.as_str()) {
        return Err(AppError::BadRequest("Invalid role".into()));
    }
    let normalized_email = body.email.trim().to_lowercase();
    if normalized_email.is_empty()
        || normalized_email.len() > 254
        || !normalized_email.contains('@')
    {
        return Err(AppError::BadRequest("Invalid email.".into()));
    }
    let (first_name, last_name) = normalize_user_name(&body.first_name, &body.last_name)?;
    if !(0.0..=168.0).contains(&body.weekly_hours) {
        return Err(AppError::BadRequest("Invalid weekly_hours.".into()));
    }
    if !(0..=366).contains(&body.leave_days_current_year)
        || !(0..=366).contains(&body.leave_days_next_year)
    {
        return Err(AppError::BadRequest("Invalid leave_days.".into()));
    }
    let effective_workdays: i16 = if is_assistant_role(&body.role) {
        if body.weekly_hours != 0.0 {
            return Err(AppError::BadRequest(
                "Assistants must have weekly_hours set to 0.".into(),
            ));
        }
        if body.overtime_start_balance_min.unwrap_or(0) != 0 {
            return Err(AppError::BadRequest(
                "Assistants cannot have an overtime start balance.".into(),
            ));
        }
        if body.workdays_per_week.is_some() {
            return Err(AppError::BadRequest(
                "Assistants cannot have fixed working days per week.".into(),
            ));
        }
        7
    } else {
        let wdpw = body.workdays_per_week.unwrap_or(5);
        if !(1..=5).contains(&wdpw) {
            return Err(AppError::BadRequest("Invalid workdays_per_week.".into()));
        }
        wdpw
    };
    ensure_email_available(app_state, &normalized_email, None).await?;
    ensure_user_name_available(app_state, &first_name, &last_name, None).await?;
    if !is_admin_role(&body.role) && !body.tracks_time {
        return Err(AppError::BadRequest(
            "tracks_time can only be disabled for admin users.".into(),
        ));
    }
    let temporary_password = match body.password {
        Some(provided) if !provided.is_empty() => {
            crate::services::auth::validate_password_strength(&provided)?;
            provided
        }
        _ => generate_password(),
    };
    let password_hash =
        crate::services::auth::hash_password_async(temporary_password.clone()).await?;
    let overtime_balance = body.overtime_start_balance_min.unwrap_or(0);
    if !(-525_600..=525_600).contains(&overtime_balance) {
        return Err(AppError::BadRequest(
            "Invalid overtime_start_balance_min.".into(),
        ));
    }
    let mut transaction = app_state.db.users.begin().await?;
    crate::services::auth::lock_user_graph(&mut transaction).await?;
    validate_approver_ids(app_state, &body.role, None, &body.approver_ids).await?;
    let new_user_id = UserDb::create(
        &mut transaction,
        &normalized_email,
        &password_hash,
        &first_name,
        &last_name,
        &body.role,
        body.weekly_hours,
        effective_workdays,
        body.start_date,
        body.hire_date,
        true,
        overtime_balance,
        body.tracks_time,
    )
    .await
    .map_err(|e| {
        tracing::warn!(target:"zerf::users", "create user insert failed: {e}");
        user_unique_conflict(&e)
            .unwrap_or_else(|| AppError::Conflict("Could not create user.".into()))
    })?;
    for approver_id in &body.approver_ids {
        UserDb::insert_approver_tx(&mut transaction, new_user_id, *approver_id).await?;
    }
    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
    UserDb::set_leave_days_tx(
        &mut transaction,
        new_user_id,
        current_year,
        body.leave_days_current_year,
    )
    .await?;
    UserDb::set_leave_days_tx(
        &mut transaction,
        new_user_id,
        current_year + 1,
        body.leave_days_next_year,
    )
    .await?;
    transaction.commit().await?;
    let created_user = app_state
        .db
        .users
        .find_by_id(new_user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let created_auth_user = repo_user_to_auth_user(created_user);
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "users",
        new_user_id,
        None,
        serde_json::to_value(&created_auth_user).ok(),
    )
    .await;
    let smtp = crate::services::settings::load_smtp_config(&app_state.pool)
        .await
        .map(std::sync::Arc::new);
    let login_line = match app_state.cfg.public_url.as_deref() {
        Some(url) => format!("\nURL:      {}\n", url.trim_end_matches('/')),
        None => String::new(),
    };
    let language = i18n::load_ui_language(&app_state.pool)
        .await
        .unwrap_or_default();
    let org_name_raw =
        crate::services::settings::load_setting(&app_state.pool, "organization_name", "")
            .await
            .unwrap_or_default();
    let org_name = if org_name_raw.trim().is_empty() {
        "Zerf".to_string()
    } else {
        org_name_raw
    };
    let subject = i18n::translate(
        &language,
        "account_created_subject",
        &[("org_name", org_name)],
    );
    let body_text = i18n::translate(
        &language,
        "account_created_body",
        &[
            ("first_name", first_name.clone()),
            ("last_name", last_name.clone()),
            ("email", normalized_email.clone()),
            ("password", temporary_password.clone()),
            ("login_line", login_line),
        ],
    );
    crate::email::send_async(
        smtp,
        normalized_email,
        format!("{} {}", first_name, last_name),
        subject,
        body_text,
    );
    Ok(CreateResponse {
        id: new_user_id,
        user: created_auth_user,
        temporary_password,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // Helper: construct a repository User with minimal fields.
    fn repo_user(id: i64, email: &str, role: &str) -> crate::repository::User {
        crate::repository::User {
            id,
            email: email.to_string(),
            password_hash: "hash".to_string(),
            first_name: "Alice".to_string(),
            last_name: "Smith".to_string(),
            role: role.to_string(),
            weekly_hours: 39.0,
            workdays_per_week: 5,
            start_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            hire_date: None,
            active: true,
            must_change_password: false,
            created_at: Utc::now(),
            allow_reopen_without_approval: false,
            allow_submission_without_approval: false,
            dark_mode: false,
            overtime_start_balance_min: 0,
            tracks_time: true,
        }
    }

    /// `repo_user_to_auth_user` must copy every field from the repository type
    /// to the middleware `User` type unchanged.
    #[test]
    fn repo_user_to_auth_user_maps_all_fields() {
        let src = repo_user(42, "alice@example.com", "admin");
        let auth = repo_user_to_auth_user(src);
        assert_eq!(auth.id, 42);
        assert_eq!(auth.email, "alice@example.com");
        assert_eq!(auth.role, "admin");
        assert_eq!(auth.weekly_hours, 39.0);
        assert_eq!(auth.workdays_per_week, 5);
        assert_eq!(auth.hire_date, None);
        assert!(auth.active);
        assert!(!auth.must_change_password);
        assert_eq!(auth.overtime_start_balance_min, 0);
        assert!(auth.tracks_time);
    }

    /// The mapping must faithfully reflect non-default boolean flags.
    #[test]
    fn repo_user_to_auth_user_respects_flag_fields() {
        let mut src = repo_user(7, "bob@example.com", "employee");
        src.hire_date = chrono::NaiveDate::from_ymd_opt(2020, 3, 1);
        src.must_change_password = true;
        src.allow_reopen_without_approval = true;
        src.allow_submission_without_approval = true;
        src.dark_mode = true;
        src.tracks_time = false;
        src.overtime_start_balance_min = 480;
        let auth = repo_user_to_auth_user(src);
        assert_eq!(auth.hire_date, chrono::NaiveDate::from_ymd_opt(2020, 3, 1));
        assert!(auth.must_change_password);
        assert!(auth.allow_reopen_without_approval);
        assert!(auth.allow_submission_without_approval);
        assert!(auth.dark_mode);
        assert!(!auth.tracks_time);
        assert_eq!(auth.overtime_start_balance_min, 480);
    }

    #[test]
    fn normalize_user_name_trims_and_accepts_valid_names() {
        let (first, last) = normalize_user_name("  Alice  ", "  Smith ").unwrap();
        assert_eq!(first, "Alice");
        assert_eq!(last, "Smith");
    }

    #[test]
    fn normalize_user_name_rejects_empty_or_too_long_names() {
        assert!(normalize_user_name("", "Smith").is_err());
        assert!(normalize_user_name("Alice", " ").is_err());

        let too_long = "x".repeat(201);
        assert!(normalize_user_name(&too_long, "Smith").is_err());
        assert!(normalize_user_name("Alice", &too_long).is_err());
    }

    #[test]
    fn normalize_optional_user_name_handles_none_and_validation() {
        assert_eq!(normalize_optional_user_name(None).unwrap(), None);
        assert_eq!(
            normalize_optional_user_name(Some(&"  Bob ".to_string())).unwrap(),
            Some("Bob".to_string())
        );
        assert!(normalize_optional_user_name(Some(&"   ".to_string())).is_err());
    }

    #[test]
    fn generated_password_has_required_strength_character_classes() {
        for _ in 0..128 {
            let password = generate_password();
            assert_eq!(password.len(), 16);
            assert!(password.chars().any(|c| c.is_ascii_lowercase()));
            assert!(password.chars().any(|c| c.is_ascii_uppercase()));
            assert!(password.chars().any(|c| c.is_ascii_digit()));
            assert!(password.chars().any(|c| "!@#*-_+".contains(c)));
        }
    }
}
