use crate::audit;
use crate::auth::{hash_password, validate_password_strength, User};
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

/// Per-user reopen policy. Returned by `GET /team-settings` for every active
/// user; visible and editable by any lead/admin.
#[derive(Serialize)]
pub struct TeamSettings {
    pub user_id: i64,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub allow_reopen_without_approval: bool,
}

async fn assert_can_access_user(
    pool: &crate::db::DatabasePool,
    requester: &User,
    target_id: i64,
) -> AppResult<()> {
    if requester.is_admin() || requester.id == target_id {
        return Ok(());
    }
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let is_direct_report: Option<bool> = sqlx::query_scalar(
        "SELECT TRUE FROM users WHERE id=$1 AND approver_id=$2 AND role!='admin'",
    )
    .bind(target_id)
    .bind(requester.id)
    .fetch_optional(pool)
    .await?;
    if is_direct_report.is_none() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn team_settings_list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<TeamSettings>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    type Row = (i64, String, String, String, String, bool);
    let rows_to_team_settings = |rows: Vec<Row>| -> Vec<TeamSettings> {
        rows.into_iter()
            .map(|(id, email, first_name, last_name, role, allow_reopen_without_approval)| TeamSettings {
                user_id: id,
                email,
                first_name,
                last_name,
                role,
                allow_reopen_without_approval,
            })
            .collect()
    };
    let settings_list = if requester.is_admin() {
        // Admins see all active users.
        rows_to_team_settings(
            sqlx::query_as::<_, Row>(
                "SELECT id, email, first_name, last_name, role, allow_reopen_without_approval \
                 FROM users WHERE active=TRUE \
                 ORDER BY last_name, first_name",
            )
            .fetch_all(&app_state.pool)
            .await?,
        )
    } else {
        // Team leads see themselves + their direct reports.
        rows_to_team_settings(
            sqlx::query_as::<_, Row>(
                "SELECT id, email, first_name, last_name, role, allow_reopen_without_approval \
                 FROM users WHERE active=TRUE AND (id=$1 OR (approver_id=$1 AND role!='admin')) \
                 ORDER BY last_name, first_name",
            )
            .bind(requester.id)
            .fetch_all(&app_state.pool)
            .await?,
        )
    };
    Ok(Json(settings_list))
}

#[derive(Deserialize)]
pub struct UpdateTeamSettings {
    pub allow_reopen_without_approval: bool,
}

pub async fn team_settings_update(
    State(app_state): State<AppState>,
    requester: User,
    Path(target_id): Path<i64>,
    Json(body): Json<UpdateTeamSettings>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    // Team leads may only edit themselves or their direct reports.
    if !requester.is_admin() && target_id != requester.id {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id=$1 AND approver_id=$2 AND active=TRUE AND role!='admin'",
        )
        .bind(target_id)
        .bind(requester.id)
        .fetch_optional(&app_state.pool)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Target must be an active user.
    let is_active: Option<bool> = sqlx::query_scalar("SELECT active FROM users WHERE id=$1")
        .bind(target_id)
        .fetch_optional(&app_state.pool)
        .await?;
    if !is_active.unwrap_or(false) {
        return Err(AppError::BadRequest("User not found or inactive.".into()));
    }
    sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
        .bind(body.allow_reopen_without_approval)
        .bind(target_id)
        .execute(&app_state.pool)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "team_settings_updated",
        "users",
        target_id,
        None,
        Some(serde_json::json!({"allow_reopen_without_approval": body.allow_reopen_without_approval})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn list(State(app_state): State<AppState>, requester: User) -> AppResult<Json<Vec<User>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let user_list = if requester.is_admin() {
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users ORDER BY last_name, first_name")
            .fetch_all(&app_state.pool)
            .await?
    } else {
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1 OR (approver_id=$1 AND role!='admin') ORDER BY last_name, first_name")
            .bind(requester.id)
            .fetch_all(&app_state.pool)
            .await?
    };
    Ok(Json(user_list))
}

pub async fn get_one(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<User>> {
    assert_can_access_user(&app_state.pool, &requester, user_id).await?;
    Ok(Json(
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
            .bind(user_id)
            .fetch_one(&app_state.pool)
            .await?,
    ))
}

#[derive(Deserialize)]
pub struct NewUser {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub weekly_hours: f64,
    pub annual_leave_days: i64,
    pub start_date: NaiveDate,
    pub overtime_start_balance_min: Option<i64>,
    pub password: Option<String>,
    /// Mandatory for non-admin users. The approver must be an active
    /// `team_lead` or `admin` and cannot be the user themselves.
    pub approver_id: Option<i64>,
}

/// Validate that `approver_id` (if any) refers to an active lead/admin and
/// is not the user themselves. Also enforces the rule that non-admin users
/// must have an approver.
async fn validate_approver(
    pool: &crate::db::DatabasePool,
    role: &str,
    user_self_id: Option<i64>,
    approver_id: Option<i64>,
) -> AppResult<()> {
    if role != "admin" && approver_id.is_none() {
        return Err(AppError::BadRequest(
            "An approver (Team lead or Admin) is required for non-admin users.".into(),
        ));
    }
    if let Some(aid) = approver_id {
        if Some(aid) == user_self_id {
            return Err(AppError::BadRequest(
                "Approver cannot be the user themselves.".into(),
            ));
        }
        let approver_row: Option<(String, bool)> =
            sqlx::query_as("SELECT role, active FROM users WHERE id=$1")
                .bind(aid)
                .fetch_optional(pool)
                .await?;
        match approver_row {
            None => return Err(AppError::BadRequest("Approver not found.".into())),
            Some((role, true)) if role == "team_lead" || role == "admin" => {}
            Some(_) => return Err(AppError::BadRequest("Approver must be an active Team lead or Admin.".into())),
        }
    }
    Ok(())
}

fn normalize_user_name(first_name: &str, last_name: &str) -> AppResult<(String, String)> {
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

fn normalize_optional_user_name(name: Option<&String>) -> AppResult<Option<String>> {
    let Some(value) = name else { return Ok(None) };
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return Err(AppError::BadRequest("Invalid name.".into()));
    }
    Ok(Some(trimmed))
}

async fn ensure_email_available(
    pool: &crate::db::DatabasePool,
    email: &str,
    excluded_user_id: Option<i64>,
) -> AppResult<()> {
    let existing_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM users WHERE email=$1 AND ($2::BIGINT IS NULL OR id<>$2) LIMIT 1",
    )
    .bind(email)
    .bind(excluded_user_id)
    .fetch_optional(pool)
    .await?;
    if existing_id.is_some() {
        return Err(AppError::Conflict("Email already exists.".into()));
    }
    Ok(())
}

async fn ensure_user_name_available(
    pool: &crate::db::DatabasePool,
    first_name: &str,
    last_name: &str,
    excluded_user_id: Option<i64>,
) -> AppResult<()> {
    let existing_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM users WHERE first_name=$1 AND last_name=$2 \
         AND ($3::BIGINT IS NULL OR id<>$3) LIMIT 1",
    )
    .bind(first_name)
    .bind(last_name)
    .bind(excluded_user_id)
    .fetch_optional(pool)
    .await?;
    if existing_id.is_some() {
        return Err(AppError::Conflict(
            "First name and last name already exist.".into(),
        ));
    }
    Ok(())
}

fn user_unique_conflict(error: &sqlx::Error) -> Option<AppError> {
    let sqlx::Error::Database(db_error) = error else { return None };
    match db_error.constraint() {
        Some("users_email_key") => Some(AppError::Conflict("Email already exists.".into())),
        Some("idx_users_first_last_name_unique") => Some(AppError::Conflict("First name and last name already exist.".into())),
        _ if db_error.code().as_deref() == Some("23505") && db_error.table() == Some("users") => {
            Some(AppError::Conflict("User already exists.".into()))
        }
        _ => None,
    }
}

#[derive(Serialize)]
pub struct CreateResponse {
    pub id: i64,
    pub user: User,
    pub temporary_password: String,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewUser>,
) -> AppResult<Json<CreateResponse>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !["employee", "team_lead", "admin"].contains(&body.role.as_str()) {
        return Err(AppError::BadRequest("Invalid role".into()));
    }
    let normalized_email = body.email.trim().to_lowercase();
    if normalized_email.is_empty() || normalized_email.len() > 254 || !normalized_email.contains('@') {
        return Err(AppError::BadRequest("Invalid email.".into()));
    }
    let (first_name, last_name) = normalize_user_name(&body.first_name, &body.last_name)?;
    if !(0.0..=168.0).contains(&body.weekly_hours) {
        return Err(AppError::BadRequest("Invalid weekly_hours.".into()));
    }
    if !(0..=366).contains(&body.annual_leave_days) {
        return Err(AppError::BadRequest("Invalid annual_leave_days.".into()));
    }
    ensure_email_available(&app_state.pool, &normalized_email, None).await?;
    ensure_user_name_available(&app_state.pool, &first_name, &last_name, None).await?;
    let temporary_password = match body.password {
        Some(provided) if !provided.is_empty() => {
            validate_password_strength(&provided)?;
            provided
        }
        _ => generate_password(),
    };
    let password_hash = hash_password(&temporary_password)?;
    validate_approver(&app_state.pool, &body.role, None, body.approver_id).await?;
    let overtime_balance = body.overtime_start_balance_min.unwrap_or(0);
    let new_user_id: i64 = sqlx::query_scalar("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,annual_leave_days,start_date,must_change_password,approver_id,overtime_start_balance_min) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id")
        .bind(&normalized_email).bind(password_hash).bind(&first_name).bind(&last_name).bind(&body.role)
        .bind(body.weekly_hours).bind(body.annual_leave_days).bind(body.start_date).bind(true).bind(body.approver_id)
        .bind(overtime_balance)
        .fetch_one(&app_state.pool).await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "create user insert failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not create user.".into()))
        })?;
    let created_user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(new_user_id)
        .fetch_one(&app_state.pool)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "users",
        new_user_id,
        None,
        Some(serde_json::to_value(&created_user).unwrap()),
    )
    .await;
    // Send registration email best-effort
    let smtp = crate::settings::load_smtp_config(&app_state.pool).await.map(std::sync::Arc::new);
    let login_line = match app_state.cfg.public_url.as_deref() {
        Some(url) => format!(
            "\nURL:      https://{}\n",
            url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
        ),
        None => String::new(),
    };
    let body_text = format!(
        "Hello {} {},\n\nYour account has been created.\n\nEmail:    {}\nPassword: {}{}\nPlease log in and change your password immediately.",
        first_name, last_name, normalized_email, temporary_password, login_line
    );
    crate::email::send_async(smtp, normalized_email, "Welcome to Zerf".to_string(), body_text);
    Ok(Json(CreateResponse {
        id: new_user_id,
        user: created_user,
        temporary_password,
    }))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub role: Option<String>,
    pub weekly_hours: Option<f64>,
    pub annual_leave_days: Option<i64>,
    pub start_date: Option<NaiveDate>,
    pub active: Option<bool>,
    /// Distinguish "field omitted" (`None`) from "explicit null"
    /// (`Some(None)`) so the admin can clear an approver when they
    /// promote a user to admin.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub approver_id: Option<Option<i64>>,
    pub allow_reopen_without_approval: Option<bool>,
    pub overtime_start_balance_min: Option<i64>,
}

fn deserialize_double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<UpdateUser>,
) -> AppResult<Json<User>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Role allow-list — never trust the client.
    if let Some(role_value) = &body.role {
        if !["employee", "team_lead", "admin"].contains(&role_value.as_str()) {
            return Err(AppError::BadRequest("Invalid role".into()));
        }
    }
    // Anti-lockout: an admin cannot demote themselves out of admin or deactivate
    // their own account; otherwise the only path back is fresh DB bootstrap.
    if user_id == requester.id {
        if let Some(role_value) = &body.role {
            if role_value != "admin" {
                return Err(AppError::BadRequest(
                    "You cannot remove your own admin role.".into(),
                ));
            }
        }
        if let Some(false) = body.active {
            return Err(AppError::BadRequest(
                "You cannot deactivate yourself.".into(),
            ));
        }
    }
    // Numeric bounds validation (same constraints as create).
    if let Some(weekly_hours) = body.weekly_hours {
        if !(0.0..=168.0).contains(&weekly_hours) {
            return Err(AppError::BadRequest("Invalid weekly_hours.".into()));
        }
    }
    if let Some(annual_leave_days) = body.annual_leave_days {
        if !(0..=366).contains(&annual_leave_days) {
            return Err(AppError::BadRequest("Invalid annual_leave_days.".into()));
        }
    }
    if let Some(overtime_start_balance) = body.overtime_start_balance_min {
        if !(-525_600..=525_600).contains(&overtime_start_balance) {
            return Err(AppError::BadRequest(
                "Invalid overtime_start_balance_min.".into(),
            ));
        }
    }
    // Email format / length sanity (lowercase + minimal validation).
    let normalized_email = body.email.as_ref().map(|email| email.trim().to_lowercase());
    if let Some(email) = &normalized_email {
        if email.is_empty() || email.len() > 254 || !email.contains('@') {
            return Err(AppError::BadRequest("Invalid email.".into()));
        }
    }
    let first_name = normalize_optional_user_name(body.first_name.as_ref())?;
    let last_name = normalize_optional_user_name(body.last_name.as_ref())?;
    let previous_user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&app_state.pool)
        .await?;
    if let Some(email) = &normalized_email {
        ensure_email_available(&app_state.pool, email, Some(user_id)).await?;
    }
    if first_name.is_some() || last_name.is_some() {
        let updated_first_name = first_name
            .clone()
            .unwrap_or_else(|| previous_user.first_name.clone());
        let updated_last_name = last_name.clone().unwrap_or_else(|| previous_user.last_name.clone());
        ensure_user_name_available(&app_state.pool, &updated_first_name, &updated_last_name, Some(user_id)).await?;
    }
    let removing_admin_rights = previous_user.role == "admin"
        && (body.role.as_deref().is_some_and(|role_value| role_value != "admin")
            || matches!(body.active, Some(false)));
    // Pre-validate the post-update invariant (non-admin → has approver).
    let new_role = body.role.clone().unwrap_or_else(|| previous_user.role.clone());
    let new_approver_id = match body.approver_id {
        Some(value) => value,
        None => previous_user.approver_id,
    };
    validate_approver(&app_state.pool, &new_role, Some(user_id), new_approver_id).await?;

    let mut tx = app_state.pool.begin().await?;
    // Last-admin protection: checked inside the transaction to prevent TOCTOU.
    if removing_admin_rights {
        let active_admins: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE active=TRUE AND role='admin'",
        )
        .fetch_one(&mut *tx)
        .await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot remove the last active admin.".into(),
            ));
        }
    }
    sqlx::query("UPDATE users SET email=COALESCE($1,email), first_name=COALESCE($2,first_name), last_name=COALESCE($3,last_name), role=COALESCE($4,role), weekly_hours=COALESCE($5,weekly_hours), annual_leave_days=COALESCE($6,annual_leave_days), start_date=COALESCE($7,start_date), active=COALESCE($8,active), allow_reopen_without_approval=COALESCE($9,allow_reopen_without_approval), overtime_start_balance_min=COALESCE($10,overtime_start_balance_min) WHERE id=$11")
        .bind(normalized_email).bind(first_name).bind(last_name).bind(body.role.clone())
        .bind(body.weekly_hours).bind(body.annual_leave_days).bind(body.start_date).bind(body.active)
        .bind(body.allow_reopen_without_approval).bind(body.overtime_start_balance_min).bind(user_id)
        .execute(&mut *tx).await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "update user failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not update user.".into()))
        })?;
    // Approver_id requires special handling because we want to support
    // explicit clearing (Some(None)) which COALESCE cannot express.
    if let Some(approver_value) = body.approver_id {
        sqlx::query("UPDATE users SET approver_id=$1 WHERE id=$2")
            .bind(approver_value)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| AppError::Conflict("Could not update approver.".into()))?;
    }
    // If role changed or user was deactivated, kill all sessions of that user
    // so cached role/state cannot be (ab)used.
    let role_changed = body.role.as_deref().map(|role_value| role_value != previous_user.role).unwrap_or(false);
    let just_deactivated = matches!(body.active, Some(false)) && previous_user.active;
    if role_changed || just_deactivated {
        let _ = sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *tx)
            .await;
    }
    tx.commit().await?;
    let updated_user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&app_state.pool)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "users",
        user_id,
        Some(serde_json::to_value(&previous_user).unwrap()),
        Some(serde_json::to_value(&updated_user).unwrap()),
    )
    .await;
    Ok(Json(updated_user))
}

pub async fn deactivate(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if user_id == requester.id {
        return Err(AppError::BadRequest(
            "You cannot deactivate yourself.".into(),
        ));
    }
    let previous_user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&app_state.pool)
        .await?;
    // Block deactivation if this person is the assigned approver for active users.
    // Orphaned approver_id references would leave those users in a broken state.
    let direct_reports_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE approver_id=$1 AND active=TRUE")
            .bind(user_id)
            .fetch_one(&app_state.pool)
            .await?;
    if direct_reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot deactivate: {} active user(s) still have this person as their approver. Reassign them first.",
            direct_reports_count
        )));
    }
    let mut tx = app_state.pool.begin().await?;
    sqlx::query("UPDATE users SET active=FALSE WHERE id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "deactivated",
        "users",
        user_id,
        Some(serde_json::to_value(&previous_user).unwrap()),
        Some(serde_json::json!({"active": false})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn reset_password(
    State(app_state): State<AppState>,
    requester: User,
    Path(target_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let temporary_password = generate_password();
    let new_password_hash = hash_password(&temporary_password)?;
    let mut tx = app_state.pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash=$1, must_change_password=TRUE WHERE id=$2")
        .bind(new_password_hash)
        .bind(target_id)
        .execute(&mut *tx)
        .await?;
    // Force re-authentication: kill any existing sessions for this user.
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(target_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "password_reset",
        "users",
        target_id,
        None,
        Some(serde_json::json!({"password_reset": true})),
    )
    .await;
    Ok(Json(serde_json::json!({"temporary_password": temporary_password})))
}

// -- Per-user per-year vacation day overrides ---

#[derive(Deserialize)]
pub struct LeaveOverrideBody {
    pub year: i32,
    pub days: i64,
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct LeaveOverride {
    pub user_id: i64,
    pub year: i32,
    pub days: i64,
}

pub async fn get_leave_overrides(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<Vec<LeaveOverride>>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let override_rows = sqlx::query_as::<_, LeaveOverride>(
        "SELECT user_id, year, days FROM user_annual_leave_overrides WHERE user_id=$1 ORDER BY year",
    )
    .bind(user_id)
    .fetch_all(&app_state.pool)
    .await?;
    Ok(Json(override_rows))
}

pub async fn set_leave_override(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<LeaveOverrideBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let current_year = chrono::Local::now().year();
    if body.year < current_year || body.year > current_year + 1 {
        return Err(AppError::BadRequest(
            "Leave overrides can only be set for the current or next year.".into(),
        ));
    }
    if !(0..=366).contains(&body.days) {
        return Err(AppError::BadRequest("Invalid days value.".into()));
    }
    // Verify user exists and is active
    let is_active: bool = sqlx::query_scalar("SELECT active FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_optional(&app_state.pool)
        .await?
        .ok_or(AppError::NotFound)?;
    if !is_active {
        return Err(AppError::BadRequest("User is inactive.".into()));
    }
    sqlx::query(
        "INSERT INTO user_annual_leave_overrides(user_id, year, days) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days",
    )
    .bind(user_id)
    .bind(body.year)
    .bind(body.days)
    .execute(&app_state.pool)
    .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "users",
        user_id,
        None,
        Some(serde_json::json!({"leave_override": {"year": body.year, "days": body.days}})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Generate a 16-char temporary password with at least one of each class
/// (lower / upper / digit / symbol) so it satisfies the strength policy.
/// Uses the OS CSPRNG (`SysRng`) — never the thread RNG — for security.
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
    let mut password_bytes: Vec<u8> = character_pools
        .iter()
        .map(|pool| {
            let mut buf = [0u8; 1];
            rng.fill_bytes(&mut buf);
            pool[(buf[0] as usize) % pool.len()]
        })
        .collect();
    let all_chars: Vec<u8> = character_pools.iter().flat_map(|pool| pool.iter().copied()).collect();
    while password_bytes.len() < 16 {
        let mut buf = [0u8; 1];
        rng.fill_bytes(&mut buf);
        password_bytes.push(all_chars[(buf[0] as usize) % all_chars.len()]);
    }
    password_bytes.shuffle(&mut rng);
    String::from_utf8(password_bytes).unwrap()
}
