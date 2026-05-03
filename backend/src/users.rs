use crate::audit;
use crate::auth::{hash_password, validate_password_strength, User};
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Per-approver self-service policy. Returned by `GET /team-settings` for the
/// current user (lead/admin) or for all approvers (admin only).
#[derive(Serialize)]
pub struct TeamSettings {
    pub approver_id: i64,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub allow_reopen_without_approval: bool,
}

pub async fn team_settings_list(
    State(s): State<AppState>,
    u: User,
) -> AppResult<Json<Vec<TeamSettings>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rows: Vec<TeamSettings> = if u.is_admin() {
        sqlx::query_as::<_, (i64, String, String, String, bool)>(
            "SELECT id, email, first_name, last_name, allow_reopen_without_approval \
             FROM users WHERE active=TRUE AND role IN ('team_lead','admin') \
             ORDER BY last_name, first_name",
        )
        .fetch_all(&s.pool)
        .await?
        .into_iter()
        .map(|(id, email, fi, la, p)| TeamSettings {
            approver_id: id,
            email,
            first_name: fi,
            last_name: la,
            allow_reopen_without_approval: p,
        })
        .collect()
    } else {
        // Team leads see only their own row.
        let row: (String, String, String, bool) = sqlx::query_as(
            "SELECT email, first_name, last_name, allow_reopen_without_approval \
             FROM users WHERE id=$1",
        )
        .bind(u.id)
        .fetch_one(&s.pool)
        .await?;
        vec![TeamSettings {
            approver_id: u.id,
            email: row.0,
            first_name: row.1,
            last_name: row.2,
            allow_reopen_without_approval: row.3,
        }]
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
    Path(approver_id): Path<i64>,
    Json(b): Json<UpdateTeamSettings>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    // Leads may only edit their own policy.
    if !u.is_admin() && approver_id != u.id {
        return Err(AppError::Forbidden);
    }
    // Target must be an active lead/admin.
    let role: Option<(String, bool)> = sqlx::query_as("SELECT role, active FROM users WHERE id=$1")
        .bind(approver_id)
        .fetch_optional(&s.pool)
        .await?;
    match role {
        Some((r, true)) if r == "team_lead" || r == "admin" => {}
        _ => {
            return Err(AppError::BadRequest(
                "Policy can only be set on an active Team lead or Admin.".into(),
            ))
        }
    }
    sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
        .bind(b.allow_reopen_without_approval)
        .bind(approver_id)
        .execute(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "team_settings_updated",
        "users",
        approver_id,
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
    let r = sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users ORDER BY last_name, first_name")
        .fetch_all(&s.pool)
        .await?;
    Ok(Json(r))
}

pub async fn get_one(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<User>> {
    if !u.is_lead() && u.id != id {
        return Err(AppError::Forbidden);
    }
    Ok(Json(
        sqlx::query_as::<_, User>("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
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
    pub password: Option<String>,
    pub generated_password: Option<bool>,
    /// Mandatory for `role == "employee"`. The approver must be an active
    /// `team_lead` or `admin` and cannot be the user themselves.
    pub approver_id: Option<i64>,
}

/// Validate that `approver_id` (if any) refers to an active lead/admin and
/// is not the user themselves.  Also enforces the rule that active employees
/// MUST have an approver.
async fn validate_approver(
    pool: &crate::db::DatabasePool,
    role: &str,
    user_self_id: Option<i64>,
    active: bool,
    approver_id: Option<i64>,
) -> AppResult<()> {
    if role == "employee" && active && approver_id.is_none() {
        return Err(AppError::BadRequest(
            "An approver (Team lead or Admin) is required for employees.".into(),
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

#[derive(Serialize)]
pub struct CreateResponse {
    pub id: i64,
    pub user: User,
    pub temporary_password: Option<String>,
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
    if b.first_name.trim().is_empty()
        || b.last_name.trim().is_empty()
        || b.first_name.len() > 200
        || b.last_name.len() > 200
    {
        return Err(AppError::BadRequest("Invalid name.".into()));
    }
    if !(0.0..=168.0).contains(&b.weekly_hours) {
        return Err(AppError::BadRequest("Invalid weekly_hours.".into()));
    }
    if !(0..=366).contains(&b.annual_leave_days) {
        return Err(AppError::BadRequest("Invalid annual_leave_days.".into()));
    }
    let generated_by_admin = b.generated_password.unwrap_or(false);
    let (password, temp) = match b.password {
        Some(p) if !p.is_empty() => {
            validate_password_strength(&p)?;
            let temp = if generated_by_admin {
                Some(p.clone())
            } else {
                None
            };
            (p, temp)
        }
        _ => {
            let t = generate_password();
            (t.clone(), Some(t))
        }
    };
    let hash = hash_password(&password)?;
    let must_change = temp.is_some();
    validate_approver(&s.pool, &b.role, None, true, b.approver_id).await?;
    let id: i64 = sqlx::query_scalar("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,annual_leave_days,start_date,must_change_password,approver_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id")
        .bind(&email_norm).bind(hash).bind(b.first_name.trim()).bind(b.last_name.trim()).bind(&b.role)
        .bind(b.weekly_hours).bind(b.annual_leave_days).bind(b.start_date).bind(must_change).bind(b.approver_id)
        .fetch_one(&s.pool).await
        .map_err(|e| {
            tracing::warn!(target:"kitazeit::users", "create user insert failed: {e}");
            AppError::Conflict("Email already exists or invalid approver.".into())
        })?;
    let user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
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
        let smtp = s.cfg.smtp.clone().map(std::sync::Arc::new);
        let email_to = email_norm.clone();
        let display_pw = temp.clone().unwrap_or_else(|| "(set by admin)".into());
        let subject = "Welcome to KitaZeit".to_string();
        let body_text = format!(
            "Hello {} {},\n\nYour KitaZeit account has been created.\n\nEmail: {}\nPassword: {}\n\nPlease log in and change your password immediately.",
            b.first_name.trim(), b.last_name.trim(), email_to, display_pw
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
    /// promote an employee to lead.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub approver_id: Option<Option<i64>>,
    pub allow_reopen_without_approval: Option<bool>,
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
    let prev: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    // Pre-validate the post-update invariant (active employee → has approver).
    let next_role = b.role.clone().unwrap_or_else(|| prev.role.clone());
    let next_active = b.active.unwrap_or(prev.active);
    let next_approver = match b.approver_id {
        Some(v) => v,
        None => prev.approver_id,
    };
    validate_approver(&s.pool, &next_role, Some(id), next_active, next_approver).await?;

    sqlx::query("UPDATE users SET email=COALESCE($1,email), first_name=COALESCE($2,first_name), last_name=COALESCE($3,last_name), role=COALESCE($4,role), weekly_hours=COALESCE($5,weekly_hours), annual_leave_days=COALESCE($6,annual_leave_days), start_date=COALESCE($7,start_date), active=COALESCE($8,active), allow_reopen_without_approval=COALESCE($9,allow_reopen_without_approval) WHERE id=$10")
        .bind(email_lc).bind(b.first_name).bind(b.last_name).bind(b.role.clone())
        .bind(b.weekly_hours).bind(b.annual_leave_days).bind(b.start_date).bind(b.active)
        .bind(b.allow_reopen_without_approval).bind(id)
        .execute(&s.pool).await
        .map_err(|_| AppError::Conflict("Could not update user (e.g. email conflict).".into()))?;
    // Approver_id requires special handling because we want to support
    // explicit clearing (Some(None)) which COALESCE cannot express.
    if let Some(v) = b.approver_id {
        sqlx::query("UPDATE users SET approver_id=$1 WHERE id=$2")
            .bind(v)
            .bind(id)
            .execute(&s.pool)
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
            .execute(&s.pool)
            .await;
    }
    let next: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
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
    let prev: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
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
