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
use sqlx::{Executor, FromRow, Postgres};
use std::collections::BTreeSet;
use std::sync::Arc;
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
const SESSION_COOKIE_SECURE: &str = "__Host-zerf_session";
const SESSION_COOKIE_PLAIN: &str = "zerf_session";
const USER_GRAPH_LOCK_KEY: i64 = 0x7A_45_52_46_5F_53_54_55_i64;

fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SESSION_COOKIE_SECURE
    } else {
        SESSION_COOKIE_PLAIN
    }
}
const ABSOLUTE_TIMEOUT_HOURS: i64 = 168; // 7 days absolute timeout (since session creation)
const IDLE_TIMEOUT_HOURS: i64 = 8; // sliding idle timeout (since last_active_at)
const MAX_FAILED_LOGINS: i64 = 5;
const LOCKOUT_MIN: i64 = 15;
const MIN_PW_LEN: usize = 12;
const PASSWORD_RESET_TTL_HOURS: i64 = 1;

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
    pub start_date: chrono::NaiveDate,
    pub active: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    /// Lead/admin who reviews this user's requests.
    /// Mandatory for non-admin users.
    pub approver_id: Option<i64>,
    /// When TRUE, this user's reopen requests are auto-approved without waiting
    /// for manual review.  The designated approver and all admins still receive
    /// an in-app + email notification that the auto-approval happened.
    pub allow_reopen_without_approval: bool,
    pub dark_mode: bool,
    pub overtime_start_balance_min: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
    pub fn is_lead(&self) -> bool {
        self.role == "team_lead" || self.role == "admin"
    }
}

fn approver_can_review_subject(subject_role: &str, approver_role: &str) -> bool {
    match subject_role {
        "admin" => approver_role == "admin",
        _ => approver_role == "team_lead" || approver_role == "admin",
    }
}

async fn valid_approval_recipient_id(
    pool: &crate::db::DatabasePool,
    approver_id: i64,
    subject_role: &str,
) -> Option<i64> {
    let approver_row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT id, role, active FROM users WHERE id=$1",
    )
    .bind(approver_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    match approver_row {
        Some((id, approver_role, true))
            if approver_can_review_subject(subject_role, &approver_role) =>
        {
            Some(id)
        }
        _ => None,
    }
}

pub async fn primary_approval_recipient_id(
    pool: &crate::db::DatabasePool,
    requester: &User,
) -> Option<i64> {
    if let Some(approver_id) = requester.approver_id {
        if let Some(valid_id) = valid_approval_recipient_id(pool, approver_id, &requester.role).await {
            return Some(valid_id);
        }
    }
    if requester.is_admin() {
        return Some(requester.id);
    }
    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM users WHERE active=TRUE AND role='admin' ORDER BY id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

pub async fn lock_user_graph<'e, E>(executor: E) -> AppResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    // Serialize mutations that change the approver/admin graph so approval
    // routing and first-boot setup cannot observe stale membership.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(USER_GRAPH_LOCK_KEY)
        .execute(executor)
        .await?;
    Ok(())
}

pub async fn approval_recipient_ids(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    let mut ids: BTreeSet<i64> = BTreeSet::new();

    if let Some(approver_id) = requester.approver_id {
        if let Some(valid_id) = valid_approval_recipient_id(pool, approver_id, &requester.role).await {
            ids.insert(valid_id);
        }
    }

    if ids.is_empty() {
        if requester.is_admin() {
            ids.insert(requester.id);
        } else if let Ok(admins) =
            sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE active=TRUE AND role='admin'")
                .fetch_all(pool)
                .await
        {
            ids.extend(admins);
        }
    }

    ids.into_iter().collect()
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
    let classes = [
        pw.chars().any(|c| c.is_ascii_lowercase()),
        pw.chars().any(|c| c.is_ascii_uppercase()),
        pw.chars().any(|c| c.is_ascii_digit()),
        pw.chars().any(|c| !c.is_ascii_alphanumeric()),
    ]
    .iter()
    .filter(|&&present| present)
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
    State(app_state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginReq>,
) -> AppResult<Response> {
    // Origin / Referer check — defence-in-depth against CSRF on the JSON login.
    enforce_same_origin_headers(&headers, &app_state)?;

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
    .fetch_one(&app_state.pool)
    .await?;
    if failures >= MAX_FAILED_LOGINS {
        // Account is in lockout. We deliberately do NOT insert another failed
        // attempt here. Doing so would let any unauthenticated attacker who
        // knows a target email address keep that account permanently locked
        // out from the public internet by spraying bad logins — including
        // during incident response. The existing failures naturally expire
        // after LOCKOUT_MIN minutes, after which the legitimate user can
        // retry. We log server-side so operators retain visibility.
        tracing::warn!(target: "zerf::auth", email = %email, "login attempt during lockout window — ignored");
        // Generic message — never reveal that the account exists/is locked.
        return Err(AppError::BadRequest("Invalid email or password.".into()));
    }

    let user: Option<User> =
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&app_state.pool)
            .await?;
    // Always perform a hash verification to keep timing constant for unknown emails.
    let dummy = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$8ueQukxsrOwHPzjhsRTRppvNN0o3Qx0vg7HHmH64Bmw";
    let password_matches = match &user {
        Some(found_user) => verify_password(&req.password, &found_user.password_hash),
        None => {
            let _ = verify_password(&req.password, dummy);
            false
        }
    };
    sqlx::query("INSERT INTO login_attempts(email, success) VALUES ($1, $2)")
        .bind(&email)
        .bind(password_matches)
        .execute(&app_state.pool)
        .await?;
    let user = user.ok_or_else(|| AppError::BadRequest("Invalid email or password.".into()))?;
    if !password_matches {
        return Err(AppError::BadRequest("Invalid email or password.".into()));
    }
    if !user.active {
        return Err(AppError::BadRequest("account_deactivated".into()));
    }

    // Session fixation defence: any pre-existing session token sent in the request
    // is ignored; we always issue a fresh, random, never-reused token.
    let session_token = new_token();
    let csrf_token = new_token();
    sqlx::query("INSERT INTO sessions(token, user_id, csrf_token) VALUES ($1, $2, $3)")
        .bind(hash_token(&session_token))
        .bind(user.id)
        .bind(&csrf_token)
        .execute(&app_state.pool)
        .await?;

    let cookie = build_session_cookie(
        &session_token,
        ABSOLUTE_TIMEOUT_HOURS * 3600,
        app_state.cfg.secure_cookies,
    );
    let response_body = Json(serde_json::json!({
        "ok": true,
        "user": user,
        "must_change_password": user.must_change_password,
        "csrf_token": csrf_token,
    }));
    let mut response = response_body.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    Ok(response)
}

pub async fn logout(State(app_state): State<AppState>, req: Request) -> AppResult<Response> {
    if let Some(token) = extract_token(&req) {
        // Per security policy: on logout, all sessions of the affected user are
        // deleted — not just the current one — so a user logging out from one
        // device invalidates all other open sessions too.
        let user_id: Option<i64> =
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
                .bind(hash_token(&token))
                .fetch_optional(&app_state.pool)
                .await?;
        if let Some(user_id) = user_id {
            sqlx::query("DELETE FROM sessions WHERE user_id = $1")
                .bind(user_id)
                .execute(&app_state.pool)
                .await?;
        }
    }
    let cookie = build_session_cookie("", 0, app_state.cfg.secure_cookies);
    let mut resp = Json(serde_json::json!({"ok": true})).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    Ok(resp)
}

pub async fn me(
    State(app_state): State<AppState>,
    user: User,
    req: Request,
) -> AppResult<Json<serde_json::Value>> {
    // Expose the CSRF token to the SPA so it can include it on subsequent
    // state-changing requests as `X-CSRF-Token`.
    let raw_token = extract_token(&req).unwrap_or_default();
    let csrf_token: Option<String> =
        sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = $1")
            .bind(hash_token(&raw_token))
            .fetch_optional(&app_state.pool)
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
        "can_view_dashboard": true,
        "can_view_reports": true,
    });
    let mut nav = vec![
        serde_json::json!({"href":"/time","key":"Time","icon":"⏱"}),
        serde_json::json!({"href":"/absences","key":"Absences","icon":"📅"}),
        serde_json::json!({"href":"/calendar","key":"Calendar","icon":"🗓"}),
        serde_json::json!({"href":"/dashboard","key":"Dashboard","icon":"🔔"}),
        serde_json::json!({"href":"/reports","key":"Reports","icon":"📊"}),
        serde_json::json!({"href":"/account","key":"Account","icon":"👤"}),
    ];
    if user.is_lead() {
        nav.push(serde_json::json!({"href":"/team-settings","key":"TeamSettings","icon":"🛡"}));
    }
    if user.is_admin() {
        nav.push(serde_json::json!({"href":"/admin/users","key":"Admin","icon":"⚙"}));
    }
    let home = "/dashboard";
    // For admins: flag whether initial setup (country, working-time defaults,
    // and admin profile name) has been completed. Until it is, the SPA
    // redirects to /admin/settings.
    let must_configure_settings = if user.is_admin() {
        let country: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'country'")
                .fetch_optional(&app_state.pool)
                .await?;
        let dwh: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'default_weekly_hours'")
                .fetch_optional(&app_state.pool)
                .await?;
        let dal: Option<String> = sqlx::query_scalar(
            "SELECT value FROM app_settings WHERE key = 'default_annual_leave_days'",
        )
        .fetch_optional(&app_state.pool)
        .await?;
        let needs_name = user.first_name.is_empty() || user.last_name.is_empty();
        country.is_none_or(|v| v.is_empty())
            || dwh.is_none_or(|v| v.is_empty())
            || dal.is_none_or(|v| v.is_empty())
            || needs_name
    } else {
        false
    };
    Ok(Json(serde_json::json!({
        "id": user.id, "email": user.email,
        "first_name": user.first_name, "last_name": user.last_name,
        "role": user.role, "weekly_hours": user.weekly_hours,
        "start_date": user.start_date,
        "overtime_start_balance_min": user.overtime_start_balance_min,
        "active": user.active, "must_change_password": user.must_change_password,
        "must_configure_settings": must_configure_settings,
        "approver_id": user.approver_id,
        "allow_reopen_without_approval": user.allow_reopen_without_approval,
        "dark_mode": user.dark_mode,
        "csrf_token": csrf_token.unwrap_or_default(),
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

#[derive(Deserialize)]
pub struct PreferencesReq {
    pub dark_mode: bool,
}

pub async fn update_preferences(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<PreferencesReq>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query("UPDATE users SET dark_mode=$1 WHERE id=$2")
        .bind(body.dark_mode)
        .bind(user.id)
        .execute(&app_state.pool)
        .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn change_password(
    State(app_state): State<AppState>,
    user: User,
    req: Request,
) -> AppResult<Response> {
    let raw_token = extract_token(&req).ok_or(AppError::Unauthorized)?;
    let (_, raw_body) = req.into_parts();
    let body_bytes = axum::body::to_bytes(raw_body, 1024 * 1024)
        .await
        .map_err(|_| AppError::BadRequest("Invalid body".into()))?;
    let body: PasswordReq = serde_json::from_slice(&body_bytes)
        .map_err(|_| AppError::BadRequest("Invalid JSON".into()))?;
    // When the user is forced to change a temporary password, skip the current-password check
    // (they may not even know the generated string). Otherwise, require and verify it.
    if !user.must_change_password {
        let current_password = body
            .current_password
            .as_deref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| AppError::BadRequest("Current password required.".into()))?;
        if !verify_password(current_password, &user.password_hash) {
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
    let new_password_hash = hash_password(&body.new_password)?;
    let current_token_hash = hash_token(&raw_token);
    let mut tx = app_state.pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash=$1, must_change_password=FALSE WHERE id=$2")
        .bind(new_password_hash)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    // On password change, all OTHER sessions for this user are revoked, but
    // the caller's current session is preserved so they remain logged in.
    sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND token<>$2")
        .bind(user.id)
        .bind(&current_token_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}

fn extract_token(req: &Request) -> Option<String> {
    let cookie_header = req.headers().get(header::COOKIE)?.to_str().ok()?;
    extract_token_from_cookie_str(cookie_header)
}

fn extract_token_from_cookie_str(cookie_str: &str) -> Option<String> {
    extract_token_from_cookie_str_secure(cookie_str, false)
}

fn extract_token_from_cookie_str_secure(cookie_str: &str, secure_only: bool) -> Option<String> {
    // When secure_cookies is enabled, only accept the `__Host-` prefixed cookie
    // to prevent sibling-subdomain fixation attacks via the plain name.
    let prefixes: &[&str] = if secure_only {
        &[concat!("__Host-zerf_session", "=")]
    } else {
        &[
            concat!("__Host-zerf_session", "="),
            concat!("zerf_session", "="),
        ]
    };
    for part in cookie_str.split(';') {
        let cookie_part = part.trim();
        for prefix in prefixes {
            if let Some(token_value) = cookie_part.strip_prefix(prefix) {
                return Some(token_value.to_string());
            }
        }
    }
    None
}

/// Extract scheme + lowercase host + port from a URL or origin string.
/// Returns `None` for unparseable or opaque values.
fn parse_origin_parts(value: &str) -> Option<(String, String, u16)> {
    // The Origin header is just `scheme://host[:port]`, while Referer is a
    // full URL.  We parse the first slash-delimited authority regardless.
    let trimmed = value.trim();
    // Find scheme
    let (scheme, rest) = trimmed.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    // Strip path / query / fragment (take authority only)
    let authority = rest.split('/').next().unwrap_or(rest);
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        // Only treat as port if it parses as a number; otherwise it may be
        // part of an IPv6 address without brackets.
        if let Ok(port_num) = p.parse::<u16>() {
            (h.to_ascii_lowercase(), port_num)
        } else {
            (authority.to_ascii_lowercase(), default_port(&scheme))
        }
    } else {
        (authority.to_ascii_lowercase(), default_port(&scheme))
    };
    // Strip trailing dot from DNS names
    let host = host.trim_end_matches('.').to_string();
    Some((scheme, host, port))
}

fn default_port(scheme: &str) -> u16 {
    match scheme {
        "https" => 443,
        "http" => 80,
        _ => 0,
    }
}

fn enforce_same_origin_headers(
    headers: &axum::http::HeaderMap,
    app_state: &AppState,
) -> AppResult<()> {
    if !app_state.cfg.enforce_origin {
        return Ok(());
    }
    let header_origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let header_referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok());
    let allowed_origins = &app_state.cfg.allowed_origins;

    let origin_matches = |origin_value: &str| {
        let Some(req_parts) = parse_origin_parts(origin_value) else {
            return false;
        };
        allowed_origins.iter().any(|allowed| {
            parse_origin_parts(allowed)
                .is_some_and(|allowed_parts| allowed_parts == req_parts)
        })
    };
    let is_origin_allowed = match (header_origin, header_referer) {
        (Some(origin), _) => origin_matches(origin),
        (None, Some(referer)) => origin_matches(referer),
        (None, None) => false,
    };
    if !is_origin_allowed {
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
    app_state: &AppState,
    csrf_token: &str,
) -> AppResult<()> {
    if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(());
    }
    enforce_same_origin_headers(&parts.headers, app_state)?;
    if !app_state.cfg.enforce_csrf {
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
    State(app_state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();
    let session_token = extract_token_from_cookie_str_secure(
        parts
            .headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or(""),
        app_state.cfg.secure_cookies,
    )
    .ok_or(AppError::Unauthorized)?;

    let token_hash = hash_token(&session_token);
    let session_row: Option<(i64, DateTime<Utc>, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT user_id, created_at, last_active_at, csrf_token FROM sessions WHERE token = $1",
    )
    .bind(&token_hash)
    .fetch_optional(&app_state.pool)
    .await?;
    let (user_id, session_created_at, session_last_active_at, csrf_token) =
        session_row.ok_or(AppError::Unauthorized)?;
    let now = Utc::now();
    // Enforce BOTH the absolute lifetime (since creation) and the sliding idle
    // timeout (since last activity) directly in the middleware, so we never
    // depend on the background cleanup task for authn correctness.
    if now - session_created_at > Duration::hours(ABSOLUTE_TIMEOUT_HOURS)
        || now - session_last_active_at > Duration::hours(IDLE_TIMEOUT_HOURS)
    {
        sqlx::query("DELETE FROM sessions WHERE token=$1")
            .bind(&token_hash)
            .execute(&app_state.pool)
            .await?;
        return Err(AppError::Unauthorized);
    }

    enforce_csrf(&parts, &app_state, &csrf_token).await?;

    sqlx::query("UPDATE sessions SET last_active_at=CURRENT_TIMESTAMP WHERE token=$1")
        .bind(&token_hash)
        .execute(&app_state.pool)
        .await?;
    let user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1 AND active=TRUE")
        .bind(user_id)
        .fetch_optional(&app_state.pool)
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
/// Matches the timeout policy enforced in `auth_middleware`:
/// idle > IDLE_TIMEOUT_HOURS OR absolute age > ABSOLUTE_TIMEOUT_HOURS.
pub async fn cleanup_loop(pool: crate::db::DatabasePool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
    let session_cleanup_sql = format!(
        "DELETE FROM sessions \
         WHERE last_active_at < CURRENT_TIMESTAMP - INTERVAL '{IDLE_TIMEOUT_HOURS} hours' \
            OR created_at < CURRENT_TIMESTAMP - INTERVAL '{ABSOLUTE_TIMEOUT_HOURS} hours'"
    );
    loop {
        interval.tick().await;
        if let Err(e) = sqlx::query(&session_cleanup_sql)
        .execute(&pool)
        .await
        {
            tracing::warn!(target: "zerf::cleanup", "session cleanup failed: {e}");
        }
        if let Err(e) = sqlx::query(
            "DELETE FROM login_attempts WHERE attempted_at < CURRENT_TIMESTAMP - INTERVAL '1 day'",
        )
        .execute(&pool)
        .await
        {
            tracing::warn!(target: "zerf::cleanup", "login_attempts cleanup failed: {e}");
        }
        if let Err(e) =
            sqlx::query("DELETE FROM password_reset_tokens WHERE expires_at <= CURRENT_TIMESTAMP")
                .execute(&pool)
                .await
        {
            tracing::warn!(target: "zerf::cleanup", "password_reset_tokens cleanup failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Initial setup (first-boot admin creation)
// ---------------------------------------------------------------------------

/// Returns whether the application needs initial setup (no users exist yet).
pub async fn setup_status(State(app_state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&app_state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "needs_setup": user_count == 0 })))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub email: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
}

/// Create the initial admin user. Only works when no users exist yet.
pub async fn setup(
    State(app_state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> AppResult<Json<serde_json::Value>> {
    // Validate inputs before acquiring a transaction.
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || email.len() > 254 || !email.contains('@') {
        return Err(AppError::BadRequest("Invalid email address.".into()));
    }
    let first_name = body.first_name.trim().to_string();
    let last_name = body.last_name.trim().to_string();
    if first_name.is_empty() || last_name.is_empty() {
        return Err(AppError::BadRequest(
            "First name and last name are required.".into(),
        ));
    }
    if first_name.len() > 200 || last_name.len() > 200 {
        return Err(AppError::BadRequest("Name too long.".into()));
    }
    let password = &body.password;
    validate_password_strength(password)?;

    let password_hash = hash_password(password)?;
    let today = chrono::Utc::now().date_naive();

    // Prevent race conditions where two concurrent requests both observe
    // zero users and both insert an admin. `pool.begin()` runs at the
    // default READ COMMITTED isolation, which does NOT serialize the
    // SELECT/INSERT pair on its own. We take a transaction-scoped Postgres
    // advisory lock so any concurrent setup call blocks until ours commits,
    // and then sees the row we just inserted.
    let mut tx = app_state.pool.begin().await?;
    lock_user_graph(&mut *tx).await?;
    let existing_user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    if existing_user_count > 0 {
        tracing::warn!(target: "zerf::auth", "POST /auth/setup called after initial setup is already complete — possible probing");
        return Err(AppError::BadRequest(
            "Setup has already been completed.".into(),
        ));
    }
    let default_leave_days: i64 = sqlx::query_scalar(
        "SELECT COALESCE(value::BIGINT, 30) FROM app_settings WHERE key='default_annual_leave_days'",
    )
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(30);
    sqlx::query(
        "INSERT INTO users(email, password_hash, first_name, last_name, role, \
         weekly_hours, start_date, must_change_password, \
         overtime_start_balance_min) \
         VALUES ($1, $2, $3, $4, 'admin', 39.0, $5, FALSE, 0)",
    )
    .bind(&email)
    .bind(&password_hash)
    .bind(&first_name)
    .bind(&last_name)
    .bind(today)
    .execute(&mut *tx)
    .await?;
    let new_user_id: i64 =
        sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
            .bind(&email)
            .fetch_one(&mut *tx)
            .await?;
    let current_year = chrono::Utc::now().date_naive().year();
    sqlx::query(
        "INSERT INTO user_annual_leave(user_id, year, days) VALUES ($1,$2,$3),($1,$4,$3) \
         ON CONFLICT DO NOTHING",
    )
    .bind(new_user_id)
    .bind(current_year)
    .bind(default_leave_days)
    .bind(current_year + 1)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// Forgot / reset password (unauthenticated)

#[derive(Deserialize)]
pub struct ForgotPasswordReq {
    pub email: String,
}

pub async fn forgot_password(
    State(app_state): State<AppState>,
    Json(body): Json<ForgotPasswordReq>,
) -> AppResult<Json<serde_json::Value>> {
    let smtp = crate::settings::load_smtp_config(&app_state.pool).await;
    if smtp.is_none() {
        // Never reveal deployment configuration to unauthenticated clients.
        tracing::warn!(target: "zerf::auth", "forgot_password called but SMTP is not configured");
        return Ok(Json(serde_json::json!({ "ok": true })));
    }

    let base_url = match app_state
        .cfg
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        Some(url) => url.to_string(),
        None => {
            // Don't disclose deployment-config state to unauthenticated
            // callers — log server-side and return the same generic OK we
            // return for unknown emails / SMTP-not-configured.
            tracing::warn!(target: "zerf::auth", "forgot_password called but ZERF_PUBLIC_URL is not configured");
            return Ok(Json(serde_json::json!({ "ok": true })));
        }
    };

    let email = body.email.trim().to_lowercase();
    // Bound the email length BEFORE writing it to login_attempts. Without
    // this, an attacker can stuff up to ~1 MiB strings (the request-body
    // limit) into the rate-limit table at 3 rows per 15 min per "email",
    // causing slow storage/index bloat. Always return the same generic
    // success response so we don't introduce an enumeration oracle.
    if email.is_empty() || email.len() > 254 {
        return Ok(Json(serde_json::json!({ "ok": true })));
    }

    // Rate-limit: max 3 reset attempts per email per 15 minutes.
    let since: DateTime<Utc> = Utc::now() - Duration::minutes(15);
    let reset_attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts WHERE email = $1 AND success = FALSE AND attempted_at > $2",
    )
    .bind(&format!("reset:{}", email))
    .bind(since)
    .fetch_one(&app_state.pool)
    .await
    .unwrap_or(0);
    if reset_attempts >= 3 {
        // Silently return OK to prevent enumeration / timing leaks.
        return Ok(Json(serde_json::json!({ "ok": true })));
    }
    // Record this reset attempt for rate-limiting purposes.
    let _ = sqlx::query("INSERT INTO login_attempts(email, success) VALUES ($1, FALSE)")
        .bind(&format!("reset:{}", email))
        .execute(&app_state.pool)
        .await;

    let user: Option<(i64, String)> =
        sqlx::query_as("SELECT id, email FROM users WHERE lower(email)=$1 AND active=TRUE")
            .bind(&email)
            .fetch_optional(&app_state.pool)
            .await?;

    // Always return 200 to prevent email enumeration.
    let Some((user_id, user_email)) = user else {
        return Ok(Json(serde_json::json!({ "ok": true })));
    };

    let raw_token = new_token();
    let token_hash = hash_token(&raw_token);
    let expires_at = Utc::now() + Duration::hours(PASSWORD_RESET_TTL_HOURS);

    sqlx::query(
        "INSERT INTO password_reset_tokens(token_hash, user_id, expires_at) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (user_id) DO UPDATE SET \
             token_hash = EXCLUDED.token_hash, \
             expires_at = EXCLUDED.expires_at, \
             created_at = CURRENT_TIMESTAMP",
    )
    .bind(&token_hash)
    .bind(user_id)
    .bind(expires_at)
    .execute(&app_state.pool)
    .await?;

    let reset_link = format!(
        "{}/login?reset_token={}",
        base_url.trim_end_matches('/'),
        raw_token
    );

    let language = crate::i18n::load_ui_language(&app_state.pool)
        .await
        .unwrap_or_default();
    let subject = crate::i18n::translate(&language, "password_reset_subject", &[]);
    let body_text = crate::i18n::translate(
        &language,
        "password_reset_body",
        &[("reset_link", reset_link)],
    );

    crate::email::send_async(smtp.map(Arc::new), user_email, subject, body_text);

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ResetPasswordTokenReq {
    pub token: String,
    pub password: String,
}

pub async fn reset_password_with_token(
    State(app_state): State<AppState>,
    Json(body): Json<ResetPasswordTokenReq>,
) -> AppResult<Json<serde_json::Value>> {
    let token_hash = hash_token(body.token.trim());
    let mut tx = app_state.pool.begin().await?;

    let expired_user_id: Option<i64> = sqlx::query_scalar(
        "DELETE FROM password_reset_tokens \
         WHERE token_hash=$1 AND expires_at <= CURRENT_TIMESTAMP \
         RETURNING user_id",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    if expired_user_id.is_some() {
        tx.commit().await?;
        return Err(AppError::BadRequest("reset_token_expired".into()));
    }

    let user_id: Option<i64> = sqlx::query_scalar(
        "DELETE FROM password_reset_tokens \
         WHERE token_hash=$1 AND expires_at > CURRENT_TIMESTAMP \
         RETURNING user_id",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let user_id = match user_id {
        Some(id) => id,
        None => return Err(AppError::BadRequest("reset_token_invalid".into())),
    };

    let current_password_hash: Option<String> = sqlx::query_scalar(
        "SELECT password_hash FROM users WHERE id=$1 AND active=TRUE FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(current_password_hash) = current_password_hash else {
        tx.commit().await?;
        return Err(AppError::BadRequest("reset_token_invalid".into()));
    };

    if let Err(err) = validate_password_strength(&body.password) {
        tx.rollback().await?;
        return Err(err);
    }

    if verify_password(&body.password, &current_password_hash) {
        tx.rollback().await?;
        return Err(AppError::BadRequest(
            "New password must differ from the current one.".into(),
        ));
    }

    let new_hash = hash_password(&body.password)?;
    let update_result = sqlx::query(
        "UPDATE users SET password_hash=$1, must_change_password=FALSE WHERE id=$2 AND active=TRUE",
    )
    .bind(&new_hash)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    if update_result.rows_affected() != 1 {
        tx.commit().await?;
        return Err(AppError::BadRequest("reset_token_invalid".into()));
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
