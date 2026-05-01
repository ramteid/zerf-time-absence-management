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

pub async fn list(State(s): State<AppState>, u: User) -> AppResult<Json<Vec<User>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let r = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY last_name, first_name")
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
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id=$1")
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
}

#[derive(Serialize)]
pub struct CreateResponse {
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
    let (password, temp) = match b.password {
        Some(p) if !p.is_empty() => {
            validate_password_strength(&p)?;
            (p, None)
        }
        _ => {
            let t = generate_password();
            (t.clone(), Some(t))
        }
    };
    let hash = hash_password(&password)?;
    let must_change = temp.is_some();
    let id: i64 = sqlx::query_scalar("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,annual_leave_days,start_date,must_change_password) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id")
        .bind(&email_norm).bind(hash).bind(b.first_name.trim()).bind(b.last_name.trim()).bind(&b.role)
        .bind(b.weekly_hours).bind(b.annual_leave_days).bind(b.start_date).bind(must_change)
        .fetch_one(&s.pool).await
        .map_err(|_| AppError::Conflict("Email already exists".into()))?;
    let user: User = sqlx::query_as("SELECT * FROM users WHERE id=$1")
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
    Ok(Json(CreateResponse {
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
    let prev: User = sqlx::query_as("SELECT * FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    sqlx::query("UPDATE users SET email=COALESCE($1,email), first_name=COALESCE($2,first_name), last_name=COALESCE($3,last_name), role=COALESCE($4,role), weekly_hours=COALESCE($5,weekly_hours), annual_leave_days=COALESCE($6,annual_leave_days), start_date=COALESCE($7,start_date), active=COALESCE($8,active) WHERE id=$9")
        .bind(email_lc).bind(b.first_name).bind(b.last_name).bind(b.role.clone())
        .bind(b.weekly_hours).bind(b.annual_leave_days).bind(b.start_date).bind(b.active).bind(id)
        .execute(&s.pool).await
        .map_err(|_| AppError::Conflict("Could not update user (e.g. email conflict).".into()))?;
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
    let next: User = sqlx::query_as("SELECT * FROM users WHERE id=$1")
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
    audit::log(&s.pool, u.id, "deactivated", "users", id, None, None).await;
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
    audit::log(&s.pool, u.id, "password_reset", "users", id, None, None).await;
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
