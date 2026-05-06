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
    State(s): State<AppState>,
    u: User,
) -> AppResult<Json<Vec<TeamSettings>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rows: Vec<TeamSettings> = if u.is_admin() {
        // Admins see all active users.
        sqlx::query_as::<_, (i64, String, String, String, String, bool)>(
            "SELECT id, email, first_name, last_name, role, allow_reopen_without_approval \
             FROM users WHERE active=TRUE \
             ORDER BY last_name, first_name",
        )
        .fetch_all(&s.pool)
        .await?
        .into_iter()
        .map(|(id, email, fi, la, role, p)| TeamSettings {
            user_id: id,
            email,
            first_name: fi,
            last_name: la,
            role,
            allow_reopen_without_approval: p,
        })
        .collect()
    } else {
        // Team leads see themselves + their direct reports.
        sqlx::query_as::<_, (i64, String, String, String, String, bool)>(
            "SELECT id, email, first_name, last_name, role, allow_reopen_without_approval \
             FROM users WHERE active=TRUE AND (id=$1 OR (approver_id=$1 AND role!='admin')) \
             ORDER BY last_name, first_name",
        )
        .bind(u.id)
        .fetch_all(&s.pool)
        .await?
        .into_iter()
        .map(|(id, email, fi, la, role, p)| TeamSettings {
            user_id: id,
            email,
            first_name: fi,
            last_name: la,
            role,
            allow_reopen_without_approval: p,
        })
        .collect()
    };
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct UpdateTeamSettings {
    pub allow_reopen_without_approval: bool,
}

pub async fn team_settings_update(
    State(s): State<AppState>,
    u: User,
    Path(target_id): Path<i64>,
    Json(b): Json<UpdateTeamSettings>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    // Team leads may only edit themselves or their direct reports.
    if !u.is_admin() && target_id != u.id {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id=$1 AND approver_id=$2 AND active=TRUE AND role!='admin'",
        )
        .bind(target_id)
        .bind(u.id)
        .fetch_optional(&s.pool)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Target must be an active user.
    let active: Option<bool> = sqlx::query_scalar("SELECT active FROM users WHERE id=$1")
        .bind(target_id)
        .fetch_optional(&s.pool)
        .await?;
    match active {
        Some(true) => {}
        _ => return Err(AppError::BadRequest("User not found or inactive.".into())),
    }
    sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
        .bind(b.allow_reopen_without_approval)
        .bind(target_id)
        .execute(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "team_settings_updated",
        "users",
        target_id,
        None,
        Some(serde_json::json!({"allow_reopen_without_approval": b.allow_reopen_without_approval})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn list(State(s): State<AppState>, u: User) -> AppResult<Json<Vec<User>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let r = if u.is_admin() {
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users ORDER BY last_name, first_name")
            .fetch_all(&s.pool)
            .await?
    } else {
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1 OR (approver_id=$1 AND role!='admin') ORDER BY last_name, first_name")
            .bind(u.id)
            .fetch_all(&s.pool)
            .await?
    };
    Ok(Json(r))
}

pub async fn get_one(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<User>> {
    assert_can_access_user(&s.pool, &u, id).await?;
    Ok(Json(
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
            .bind(id)
            .fetch_one(&s.pool)
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
        let row: Option<(String, bool)> =
            sqlx::query_as("SELECT role, active FROM users WHERE id=$1")
                .bind(aid)
                .fetch_optional(pool)
                .await?;
        match row {
            Some((r, true)) if r == "team_lead" || r == "admin" => {}
            Some(_) => {
                return Err(AppError::BadRequest(
                    "Approver must be an active Team lead or Admin.".into(),
                ))
            }
            None => return Err(AppError::BadRequest("Approver not found.".into())),
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
    match name {
        Some(value) => {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() || trimmed.len() > 200 {
                return Err(AppError::BadRequest("Invalid name.".into()));
            }
            Ok(Some(trimmed))
        }
        None => Ok(None),
    }
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
    let db_error = match error {
        sqlx::Error::Database(db_error) => db_error,
        _ => return None,
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

#[derive(Serialize)]
pub struct CreateResponse {
    pub id: i64,
    pub user: User,
    pub temporary_password: String,
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewUser>,
) -> AppResult<Json<CreateResponse>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !["employee", "team_lead", "admin"].contains(&b.role.as_str()) {
        return Err(AppError::BadRequest("Invalid role".into()));
    }
    let email_norm = b.email.trim().to_lowercase();
    if email_norm.is_empty() || email_norm.len() > 254 || !email_norm.contains('@') {
        return Err(AppError::BadRequest("Invalid email.".into()));
    }
    let (first_name, last_name) = normalize_user_name(&b.first_name, &b.last_name)?;
    if !(0.0..=168.0).contains(&b.weekly_hours) {
        return Err(AppError::BadRequest("Invalid weekly_hours.".into()));
    }
    if !(0..=366).contains(&b.annual_leave_days) {
        return Err(AppError::BadRequest("Invalid annual_leave_days.".into()));
    }
    ensure_email_available(&s.pool, &email_norm, None).await?;
    ensure_user_name_available(&s.pool, &first_name, &last_name, None).await?;
    let (password, temp) = match b.password {
        Some(p) if !p.is_empty() => {
            validate_password_strength(&p)?;
            (p.clone(), p)
        }
        _ => {
            let t = generate_password();
            (t.clone(), t)
        }
    };
    let hash = hash_password(&password)?;
    validate_approver(&s.pool, &b.role, None, b.approver_id).await?;
    let overtime_balance = b.overtime_start_balance_min.unwrap_or(0);
    let id: i64 = sqlx::query_scalar("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,annual_leave_days,start_date,must_change_password,approver_id,overtime_start_balance_min) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id")
        .bind(&email_norm).bind(hash).bind(&first_name).bind(&last_name).bind(&b.role)
        .bind(b.weekly_hours).bind(b.annual_leave_days).bind(b.start_date).bind(true).bind(b.approver_id)
        .bind(overtime_balance)
        .fetch_one(&s.pool).await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "create user insert failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not create user.".into()))
        })?;
    let user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "created",
        "users",
        id,
        None,
        Some(serde_json::to_value(&user).unwrap()),
    )
    .await;
    // Send registration email best-effort
    {
        let smtp = crate::settings::load_smtp_config(&s.pool)
            .await
            .map(std::sync::Arc::new);
        let email_to = email_norm.clone();
        let display_pw = temp.clone();
        let subject = "Welcome to Zerf".to_string();
        let login_line = match s.cfg.public_url.as_deref() {
            Some(url) => format!(
                "\nURL:      https://{}\n",
                url.trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/')
            ),
            None => String::new(),
        };
        let body_text = format!(
            "Hello {} {},\n\nYour account has been created.\n\nEmail:    {}\nPassword: {}{}\nPlease log in and change your password immediately.",
            first_name, last_name, email_to, display_pw, login_line
        );
        crate::email::send_async(smtp, email_to, subject, body_text);
    }
    Ok(Json(CreateResponse {
        id,
        user,
        temporary_password: temp,
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
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<UpdateUser>,
) -> AppResult<Json<User>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Role allow-list — never trust the client.
    if let Some(r) = &b.role {
        if !["employee", "team_lead", "admin"].contains(&r.as_str()) {
            return Err(AppError::BadRequest("Invalid role".into()));
        }
    }
    // Anti-lockout: an admin cannot demote themselves out of admin or deactivate
    // their own account; otherwise the only path back is fresh DB bootstrap.
    if id == u.id {
        if let Some(r) = &b.role {
            if r != "admin" {
                return Err(AppError::BadRequest(
                    "You cannot remove your own admin role.".into(),
                ));
            }
        }
        if let Some(false) = b.active {
            return Err(AppError::BadRequest(
                "You cannot deactivate yourself.".into(),
            ));
        }
    }
    // Email format / length sanity (lowercase + minimal validation).
    let email_lc = b.email.as_ref().map(|e| e.trim().to_lowercase());
    if let Some(e) = &email_lc {
        if e.is_empty() || e.len() > 254 || !e.contains('@') {
            return Err(AppError::BadRequest("Invalid email.".into()));
        }
    }
    let first_name = normalize_optional_user_name(b.first_name.as_ref())?;
    let last_name = normalize_optional_user_name(b.last_name.as_ref())?;
    let prev: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if let Some(e) = &email_lc {
        ensure_email_available(&s.pool, e, Some(id)).await?;
    }
    if first_name.is_some() || last_name.is_some() {
        let next_first_name = first_name
            .clone()
            .unwrap_or_else(|| prev.first_name.clone());
        let next_last_name = last_name.clone().unwrap_or_else(|| prev.last_name.clone());
        ensure_user_name_available(&s.pool, &next_first_name, &next_last_name, Some(id)).await?;
    }
    // Pre-validate the post-update invariant (non-admin → has approver).
    let next_role = b.role.clone().unwrap_or_else(|| prev.role.clone());
    let next_approver = match b.approver_id {
        Some(v) => v,
        None => prev.approver_id,
    };
    validate_approver(&s.pool, &next_role, Some(id), next_approver).await?;

    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE users SET email=COALESCE($1,email), first_name=COALESCE($2,first_name), last_name=COALESCE($3,last_name), role=COALESCE($4,role), weekly_hours=COALESCE($5,weekly_hours), annual_leave_days=COALESCE($6,annual_leave_days), start_date=COALESCE($7,start_date), active=COALESCE($8,active), allow_reopen_without_approval=COALESCE($9,allow_reopen_without_approval), overtime_start_balance_min=COALESCE($10,overtime_start_balance_min) WHERE id=$11")
        .bind(email_lc).bind(first_name).bind(last_name).bind(b.role.clone())
        .bind(b.weekly_hours).bind(b.annual_leave_days).bind(b.start_date).bind(b.active)
        .bind(b.allow_reopen_without_approval).bind(b.overtime_start_balance_min).bind(id)
        .execute(&mut *tx).await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "update user failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not update user.".into()))
        })?;
    // Approver_id requires special handling because we want to support
    // explicit clearing (Some(None)) which COALESCE cannot express.
    if let Some(v) = b.approver_id {
        sqlx::query("UPDATE users SET approver_id=$1 WHERE id=$2")
            .bind(v)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| AppError::Conflict("Could not update approver.".into()))?;
    }
    // If role changed or user was deactivated, kill all sessions of that user
    // so cached role/state cannot be (ab)used.
    let role_changed = b.role.as_deref().map(|r| r != prev.role).unwrap_or(false);
    let just_deactivated = matches!(b.active, Some(false)) && prev.active;
    if role_changed || just_deactivated {
        let _ = sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await;
    }
    tx.commit().await?;
    let next: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "updated",
        "users",
        id,
        Some(serde_json::to_value(&prev).unwrap()),
        Some(serde_json::to_value(&next).unwrap()),
    )
    .await;
    Ok(Json(next))
}

pub async fn deactivate(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    if id == u.id {
        return Err(AppError::BadRequest(
            "You cannot deactivate yourself.".into(),
        ));
    }
    let prev: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    // Block deactivation if this person is the assigned approver for active users.
    // Orphaned approver_id references would leave those users in a broken state.
    let reports_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE approver_id=$1 AND active=TRUE")
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    if reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot deactivate: {} active user(s) still have this person as their approver. Reassign them first.",
            reports_count
        )));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE users SET active=FALSE WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(
        &s.pool,
        u.id,
        "deactivated",
        "users",
        id,
        Some(serde_json::to_value(&prev).unwrap()),
        Some(serde_json::json!({"active": false})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn reset_password(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    let temp = generate_password();
    let hash = hash_password(&temp)?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash=$1, must_change_password=TRUE WHERE id=$2")
        .bind(hash)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // Force re-authentication: kill any existing sessions for this user.
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(
        &s.pool,
        u.id,
        "password_reset",
        "users",
        id,
        None,
        Some(serde_json::json!({"password_reset": true})),
    )
    .await;
    Ok(Json(serde_json::json!({"temporary_password": temp})))
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
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<LeaveOverride>>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query_as::<_, LeaveOverride>(
        "SELECT user_id, year, days FROM user_annual_leave_overrides WHERE user_id=$1 ORDER BY year",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn set_leave_override(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<LeaveOverrideBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    let current_year = chrono::Local::now().year();
    if b.year < current_year || b.year > current_year + 1 {
        return Err(AppError::BadRequest(
            "Leave overrides can only be set for the current or next year.".into(),
        ));
    }
    if !(0..=366).contains(&b.days) {
        return Err(AppError::BadRequest("Invalid days value.".into()));
    }
    // Verify user exists and is active
    let is_active: bool = sqlx::query_scalar("SELECT active FROM users WHERE id=$1")
        .bind(id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or(AppError::NotFound)?;
    if !is_active {
        return Err(AppError::BadRequest("User is inactive.".into()));
    }
    sqlx::query(
        "INSERT INTO user_annual_leave_overrides(user_id, year, days) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days",
    )
    .bind(id)
    .bind(b.year)
    .bind(b.days)
    .execute(&s.pool)
    .await?;
    audit::log(
        &s.pool,
        u.id,
        "updated",
        "users",
        id,
        None,
        Some(serde_json::json!({"leave_override": {"year": b.year, "days": b.days}})),
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
    let lower: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    let upper: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let digit: &[u8] = b"23456789";
    // Avoid characters that may confuse shells / JSON / URLs when copy-pasted:
    // backslash, quotes, $, &, ?, =, %, /
    let symbol: &[u8] = b"!@#*-_+";
    let pools = [lower, upper, digit, symbol];
    let mut rng = UnwrapErr(SysRng);
    let mut out: Vec<u8> = pools
        .iter()
        .map(|p| {
            let mut buf = [0u8; 1];
            rng.fill_bytes(&mut buf);
            p[(buf[0] as usize) % p.len()]
        })
        .collect();
    let all: Vec<u8> = pools.iter().flat_map(|p| p.iter().copied()).collect();
    while out.len() < 16 {
        let mut buf = [0u8; 1];
        rng.fill_bytes(&mut buf);
        out.push(all[(buf[0] as usize) % all.len()]);
    }
    out.shuffle(&mut rng);
    String::from_utf8(out).unwrap()
}
