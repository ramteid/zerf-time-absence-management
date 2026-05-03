use crate::error::{AppError, AppResult};
use crate::AppState;
use argon2::password_hash::{
    rand_core::{OsRng, RngCore},
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::{Algorithm, Argon2, Params, Version};
use axum::extract::{Request, State};
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use subtle::ConstantTimeEq;

// In production (Secure cookies), use the `__Host-` prefix so that:
//   * the cookie is only sent over HTTPS,
//   * the browser refuses any cookie with a `Domain=` attribute under this
//     name, which prevents a sibling subdomain (or a network attacker that
//     manages to inject Set-Cookie at the parent domain) from overwriting
//     / fixating the session cookie for our origin,
//   * Path is forced to "/".
// In dev (plain HTTP) browsers reject `__Host-` cookies, so we fall back to
// the plain name there.
const SESSION_COOKIE_SECURE: &str = "__Host-kitazeit_session";
const SESSION_COOKIE_PLAIN: &str = "kitazeit_session";

fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SESSION_COOKIE_SECURE
    } else {
        SESSION_COOKIE_PLAIN
    }
}
const IDLE_TIMEOUT_HOURS: i64 = 8; // tightened from 24h spec to 8h idle
const ABSOLUTE_TIMEOUT_HOURS: i64 = 24; // hard cap regardless of activity
const MAX_FAILED_LOGINS: i64 = 5;
const LOCKOUT_MIN: i64 = 15;
const MIN_PW_LEN: usize = 12;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub weekly_hours: f64,
    pub annual_leave_days: i64,
    pub start_date: chrono::NaiveDate,
    pub active: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    /// Lead/admin who reviews this user's reopen requests.
    /// Mandatory for active employees (DB CHECK constraint),
    /// optional for leads/admins (they may self-service).
    pub approver_id: Option<i64>,
    /// When TRUE, reopen requests authored by employees whose
    /// `approver_id` is this user are auto-approved without manual review.
    pub allow_reopen_without_approval: bool,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
    pub fn is_lead(&self) -> bool {
        self.role == "team_lead" || self.role == "admin"
    }
}

pub fn argon2_instance() -> Argon2<'static> {
    // OWASP-recommended Argon2id parameters (memory=19 MiB, t=2, p=1).
    let params = Params::new(19456, 2, 1, None).expect("argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2_instance()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    if let Ok(parsed) = PasswordHash::new(hash) {
        argon2_instance()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    } else {
        false
    }
}

/// Reject obviously weak passwords (length, character classes).
/// Spec asks for "stark gehasht" + admin-controlled passwords; we still
/// enforce a sensible minimum policy to protect users when they self-service.
pub fn validate_password_strength(pw: &str) -> AppResult<()> {
    if pw.len() < MIN_PW_LEN {
        return Err(AppError::BadRequest(format!(
            "Password must be at least {MIN_PW_LEN} characters."
        )));
    }
    if pw.len() > 256 {
        return Err(AppError::BadRequest(
            "Password is too long (max 256 chars).".into(),
        ));
    }
    let has_lower = pw.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = pw.chars().any(|c| c.is_ascii_uppercase());
    let has_digit = pw.chars().any(|c| c.is_ascii_digit());
    let has_other = pw.chars().any(|c| !c.is_ascii_alphanumeric());
    let classes = [has_lower, has_upper, has_digit, has_other]
        .iter()
        .filter(|x| **x)
        .count();
    if classes < 3 {
        return Err(AppError::BadRequest(
            "Password must include at least 3 of: lowercase, uppercase, digit, symbol.".into(),
        ));
    }
    Ok(())
}

pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Hash a raw session token with SHA-256 before storing in the DB.
/// The cookie always carries the raw token; only the hash is persisted,
/// so a DB breach cannot be used to directly replay session cookies.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn build_session_cookie(token: &str, max_age: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    let name = cookie_name(secure);
    format!("{name}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure_flag}")
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

pub async fn login(
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginReq>,
) -> AppResult<Response> {
    // Origin / Referer check — defence-in-depth against CSRF on the JSON login.
    enforce_same_origin_headers(&headers, &s)?;

    let email = req.email.trim().to_lowercase();
    if email.is_empty() || email.len() > 254 || req.password.is_empty() || req.password.len() > 1024
    {
        return Err(AppError::BadRequest("Invalid email or password.".into()));
    }

    let since: DateTime<Utc> = Utc::now() - Duration::minutes(LOCKOUT_MIN);
    let failures: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts WHERE email = $1 AND success = FALSE AND attempted_at > $2",
    )
    .bind(&email)
    .bind(since)
    .fetch_one(&s.pool)
    .await?;
    if failures >= MAX_FAILED_LOGINS {
        // Generic message — never reveal that the account exists/is locked.
        return Err(AppError::BadRequest("Invalid email or password.".into()));
    }

    let user: Option<User> =
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE email = $1 AND active = TRUE")
            .bind(&email)
            .fetch_optional(&s.pool)
            .await?;
    // Always perform a hash verification to keep timing constant for unknown emails.
    let dummy = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$8ueQukxsrOwHPzjhsRTRppvNN0o3Qx0vg7HHmH64Bmw";
    let ok = match &user {
        Some(u) => verify_password(&req.password, &u.password_hash),
        None => {
            let _ = verify_password(&req.password, dummy);
            false
        }
    };
    sqlx::query("INSERT INTO login_attempts(email, success) VALUES ($1, $2)")
        .bind(&email)
        .bind(ok)
        .execute(&s.pool)
        .await?;
    let user = user.ok_or_else(|| AppError::BadRequest("Invalid email or password.".into()))?;
    if !ok {
        return Err(AppError::BadRequest("Invalid email or password.".into()));
    }

    // Session fixation defence: any pre-existing session token sent in the request
    // is ignored; we always issue a fresh, random, never-reused token.
    let token = new_token();
    let csrf = new_token();
    sqlx::query("INSERT INTO sessions(token, user_id, csrf_token) VALUES ($1, $2, $3)")
        .bind(hash_token(&token))
        .bind(user.id)
        .bind(&csrf)
        .execute(&s.pool)
        .await?;

    // Best-effort: drop any failed-attempt rows for this email so the lockout window resets.
    sqlx::query("DELETE FROM login_attempts WHERE email = $1 AND success = FALSE")
        .bind(&email)
        .execute(&s.pool)
        .await
        .ok();

    let cookie = build_session_cookie(&token, IDLE_TIMEOUT_HOURS * 3600, s.cfg.secure_cookies);
    let body = Json(serde_json::json!({
        "ok": true,
        "user": user,
        "must_change_password": user.must_change_password,
        "csrf_token": csrf,
    }));
    let mut resp = body.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    Ok(resp)
}

pub async fn logout(State(s): State<AppState>, req: Request) -> AppResult<Response> {
    if let Some(token) = extract_token(&req) {
        // Per security policy: on logout, all sessions of the affected user are
        // deleted — not just the current one — so a user logging out from one
        // device invalidates all other open sessions too.
        let uid: Option<i64> = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
            .bind(hash_token(&token))
            .fetch_optional(&s.pool)
            .await?;
        if let Some(user_id) = uid {
            sqlx::query("DELETE FROM sessions WHERE user_id = $1")
                .bind(user_id)
                .execute(&s.pool)
                .await?;
        }
    }
    let cookie = build_session_cookie("", 0, s.cfg.secure_cookies);
    let mut resp = Json(serde_json::json!({"ok": true})).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    Ok(resp)
}

pub async fn me(
    State(s): State<AppState>,
    user: User,
    req: Request,
) -> AppResult<Json<serde_json::Value>> {
    // Expose the CSRF token to the SPA so it can include it on subsequent
    // state-changing requests as `X-CSRF-Token`.
    let token = extract_token(&req).unwrap_or_default();
    let csrf: Option<String> =
        sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = $1")
            .bind(hash_token(&token))
            .fetch_optional(&s.pool)
            .await?;
    let permissions = serde_json::json!({
        "is_admin": user.is_admin(),
        "is_lead": user.is_lead(),
        "can_manage_users": user.is_admin(),
        "can_manage_categories": user.is_admin(),
        "can_manage_holidays": user.is_admin(),
        "can_view_audit_log": user.is_admin(),
        "can_manage_settings": user.is_admin(),
        "can_manage_team_settings": user.is_lead(),
        "can_approve": user.is_lead(),
        "can_view_team_reports": user.is_lead(),
        "can_view_dashboard": user.is_lead(),
    });
    let mut nav = vec![
        serde_json::json!({"href":"/time","key":"Time","icon":"⏱"}),
        serde_json::json!({"href":"/absences","key":"Absences","icon":"📅"}),
        serde_json::json!({"href":"/calendar","key":"Calendar","icon":"🗓"}),
        serde_json::json!({"href":"/account","key":"Account","icon":"👤"}),
    ];
    if user.is_lead() {
        nav.push(serde_json::json!({"href":"/dashboard","key":"Dashboard","icon":"🔔"}));
        nav.push(serde_json::json!({"href":"/reports","key":"Reports","icon":"📊"}));
        nav.push(serde_json::json!({"href":"/team-settings","key":"TeamSettings","icon":"🛡"}));
    }
    if user.is_admin() {
        nav.push(serde_json::json!({"href":"/admin/users","key":"Admin","icon":"⚙"}));
    }
    let home = if user.role == "employee" {
        "/time"
    } else {
        "/dashboard"
    };
    Ok(Json(serde_json::json!({
        "id": user.id, "email": user.email,
        "first_name": user.first_name, "last_name": user.last_name,
        "role": user.role, "weekly_hours": user.weekly_hours,
        "annual_leave_days": user.annual_leave_days, "start_date": user.start_date,
        "active": user.active, "must_change_password": user.must_change_password,
        "approver_id": user.approver_id,
        "allow_reopen_without_approval": user.allow_reopen_without_approval,
        "csrf_token": csrf.unwrap_or_default(),
        "permissions": permissions,
        "nav": nav,
        "home": home,
    })))
}

#[derive(Deserialize)]
pub struct PasswordReq {
    pub current_password: Option<String>,
    pub new_password: String,
}

pub async fn change_password(
    State(s): State<AppState>,
    user: User,
    req: Request,
) -> AppResult<Response> {
    let token = extract_token(&req).ok_or(AppError::Unauthorized)?;
    let (parts, body_b) = req.into_parts();
    let body_bytes = axum::body::to_bytes(body_b, 1024 * 1024)
        .await
        .map_err(|_| AppError::BadRequest("Invalid body".into()))?;
    let body: PasswordReq = serde_json::from_slice(&body_bytes)
        .map_err(|_| AppError::BadRequest("Invalid JSON".into()))?;
    let _ = parts;
    // When the user is forced to change a temporary password, skip
    // the current-password check (they may not even know the generated
    // string).  Otherwise, require and verify the current password.
    if user.must_change_password {
        // No current password needed for forced change.
    } else {
        let cur = body
            .current_password
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("Current password required.".into()))?;
        if !verify_password(cur, &user.password_hash) {
            return Err(AppError::BadRequest(
                "Current password is incorrect.".into(),
            ));
        }
    }
    validate_password_strength(&body.new_password)?;
    if verify_password(&body.new_password, &user.password_hash) {
        return Err(AppError::BadRequest(
            "New password must differ from the current one.".into(),
        ));
    }
    let h = hash_password(&body.new_password)?;
    let cur_token_hash = hash_token(&token);
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash=$1, must_change_password=FALSE WHERE id=$2")
        .bind(h)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    // On password change, all OTHER sessions for this user are revoked, but
    // the caller's current session is preserved so they remain logged in.
    sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND token<>$2")
        .bind(user.id)
        .bind(&cur_token_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}

fn extract_token(req: &Request) -> Option<String> {
    let h = req.headers().get(header::COOKIE)?.to_str().ok()?;
    extract_token_from_cookie_str(h)
}

fn extract_token_from_cookie_str(h: &str) -> Option<String> {
    // Accept both the `__Host-` prefixed (production) and the plain (dev) names
    // so that an upgrade to secure cookies on a running deployment doesn't
    // break already-issued sessions.
    let prefixes = [
        concat!("__Host-kitazeit_session", "="),
        concat!("kitazeit_session", "="),
    ];
    for part in h.split(';') {
        let p = part.trim();
        for pref in prefixes {
            if let Some(rest) = p.strip_prefix(pref) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn enforce_same_origin_headers(headers: &axum::http::HeaderMap, s: &AppState) -> AppResult<()> {
    if !s.cfg.enforce_origin {
        return Ok(());
    }
    let header_origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let header_referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok());
    let allowed = &s.cfg.allowed_origins;
    let matches = |val: &str| {
        allowed
            .iter()
            .any(|a| val == a || val.starts_with(&format!("{a}/")))
    };
    let ok = match (header_origin, header_referer) {
        (Some(o), _) => matches(o),
        (None, Some(r)) => matches(r),
        (None, None) => false,
    };
    if !ok {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// CSRF: for non-GET/HEAD/OPTIONS, require the same Origin/Referer to match the
/// configured allow-list AND a double-submit `X-CSRF-Token` header that matches
/// the session's csrf_token. SameSite=Strict already prevents most CSRF, this
/// is defence-in-depth.
async fn enforce_csrf(
    parts: &axum::http::request::Parts,
    s: &AppState,
    csrf_token: &str,
) -> AppResult<()> {
    if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(());
    }
    enforce_same_origin_headers(&parts.headers, s)?;
    if !s.cfg.enforce_csrf {
        return Ok(());
    }
    let header_token = parts
        .headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if header_token.is_empty()
        || header_token
            .as_bytes()
            .ct_eq(csrf_token.as_bytes())
            .unwrap_u8()
            == 0
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn auth_middleware(
    State(s): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();
    let token = extract_token_from_cookie_str(
        parts
            .headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    )
    .ok_or(AppError::Unauthorized)?;

    let row: Option<(i64, DateTime<Utc>, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT user_id, last_active_at, created_at, csrf_token FROM sessions WHERE token = $1",
    )
    .bind(hash_token(&token))
    .fetch_optional(&s.pool)
    .await?;
    let (uid, last, created, csrf) = row.ok_or(AppError::Unauthorized)?;
    let now = Utc::now();
    if now - last > Duration::hours(IDLE_TIMEOUT_HOURS)
        || now - created > Duration::hours(ABSOLUTE_TIMEOUT_HOURS)
    {
        sqlx::query("DELETE FROM sessions WHERE token=$1")
            .bind(hash_token(&token))
            .execute(&s.pool)
            .await?;
        return Err(AppError::Unauthorized);
    }

    enforce_csrf(&parts, &s, &csrf).await?;

    sqlx::query("UPDATE sessions SET last_active_at=CURRENT_TIMESTAMP WHERE token=$1")
        .bind(hash_token(&token))
        .execute(&s.pool)
        .await?;
    let user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1 AND active=TRUE")
        .bind(uid)
        .fetch_optional(&s.pool)
        .await?
        .ok_or(AppError::Unauthorized)?;
    parts.extensions.insert(user);
    Ok(next.run(Request::from_parts(parts, body)).await)
}

impl<S> axum::extract::FromRequestParts<S> for User
where
    S: Send + Sync,
{
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<User>()
            .cloned()
            .ok_or(AppError::Unauthorized)
    }
}

/// Periodic cleanup of expired sessions and old login attempts.
/// Matches the actual timeout policy: idle > 8 h OR absolute age > 24 h.
pub async fn cleanup_loop(pool: crate::db::DatabasePool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        interval.tick().await;
        let _ = sqlx::query(
            "DELETE FROM sessions \
             WHERE last_active_at < CURRENT_TIMESTAMP - INTERVAL '8 hours' \
                OR created_at < CURRENT_TIMESTAMP - INTERVAL '24 hours'",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "DELETE FROM login_attempts WHERE attempted_at < CURRENT_TIMESTAMP - INTERVAL '1 day'",
        )
        .execute(&pool)
        .await;
    }
}
