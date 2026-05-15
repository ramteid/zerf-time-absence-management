use crate::error::{AppError, AppResult};
use crate::repository::UserDb;
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
    /// User's configured contract workdays per week (1-7, default 5).
    /// Used to calculate daily targets, vacation days, submission status, etc.
    /// ISO weekday semantics: contract days = first N days of week (0=Mon, 1=Tue, ...)
    pub workdays_per_week: i16,
    pub start_date: chrono::NaiveDate,
    pub active: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    /// When TRUE, this user's reopen requests are auto-approved without waiting
    /// for manual review. Explicitly assigned approvers still receive the
    /// corresponding in-app and email notifications.
    pub allow_reopen_without_approval: bool,
    pub dark_mode: bool,
    pub overtime_start_balance_min: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        crate::roles::is_admin_role(&self.role)
    }
    pub fn is_lead(&self) -> bool {
        crate::roles::is_lead_role(&self.role)
    }
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }
}

/// Fetch all explicitly assigned approvers for a user from user_approvers.
///
/// Notification recipients must be explicit assignments. Global admin fallback
/// is intentionally not used for notifications.
pub async fn user_approver_ids(pool: &crate::db::DatabasePool, user_id: i64) -> Vec<i64> {
    let db = UserDb::new(pool.clone());
    db.get_approver_ids(user_id).await.unwrap_or_default()
}

pub async fn lock_user_graph(tx: &mut sqlx::PgConnection) -> AppResult<()> {
    UserDb::lock_user_graph_tx(tx).await
}

/// Fetch all active notification recipients for approval workflows.
/// Recipients are always the user's explicitly assigned approvers.
pub async fn approval_recipient_ids(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    user_approver_ids(pool, requester.id).await
}

/// Fetch approval notification recipients and enforce that non-admin users
/// always have at least one effective approver.
pub async fn required_approval_recipient_ids(
    pool: &crate::db::DatabasePool,
    requester: &User,
) -> AppResult<Vec<i64>> {
    let mut recipient_ids = approval_recipient_ids(pool, requester).await;
    if !requester.is_admin() {
        // Legacy safety: non-admin users must never route approval notifications
        // to themselves, even if stale user_approvers rows exist.
        recipient_ids.retain(|recipient_id| *recipient_id != requester.id);
    }
    if !requester.is_admin() && recipient_ids.is_empty() {
        return Err(AppError::Conflict(
            "No valid approver is available for this request.".into(),
        ));
    }
    Ok(recipient_ids)
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

/// Async wrapper: offloads Argon2 hashing to a blocking thread so the Tokio
/// runtime is not starved during CPU-intensive work (especially important when
/// many integration tests run in parallel, each making concurrent requests).
pub async fn hash_password_async(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| AppError::Internal("password hash task panicked".into()))?
}

/// Async wrapper: offloads Argon2 verification to a blocking thread.
pub async fn verify_password_async(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .unwrap_or(false)
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
    let failures = app_state
        .db
        .sessions
        .count_recent_failures(&email, since)
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

    let user = app_state
        .db
        .users
        .find_by_email(&email)
        .await?
        .map(|u| User {
            id: u.id,
            email: u.email,
            password_hash: u.password_hash,
            first_name: u.first_name,
            last_name: u.last_name,
            role: u.role,
            weekly_hours: u.weekly_hours,
            workdays_per_week: u.workdays_per_week,
            start_date: u.start_date,
            active: u.active,
            must_change_password: u.must_change_password,
            created_at: u.created_at,
            allow_reopen_without_approval: u.allow_reopen_without_approval,
            dark_mode: u.dark_mode,
            overtime_start_balance_min: u.overtime_start_balance_min,
        });
    // Always perform a hash verification to keep timing constant for unknown emails.
    let dummy = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$8ueQukxsrOwHPzjhsRTRppvNN0o3Qx0vg7HHmH64Bmw";
    let password_matches = match &user {
        Some(found_user) => {
            verify_password_async(req.password.clone(), found_user.password_hash.clone()).await
        }
        None => {
            // Always run a dummy verification to keep timing constant for unknown emails.
            verify_password_async(req.password.clone(), dummy.to_string()).await;
            false
        }
    };
    app_state
        .db
        .sessions
        .record_attempt(&email, password_matches)
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
    app_state
        .db
        .sessions
        .create(&hash_token(&session_token), user.id, &csrf_token)
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
    let cookie_value = cookie
        .parse()
        .map_err(|_| AppError::Internal("Failed to build session cookie header.".into()))?;
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie_value);
    Ok(response)
}

pub async fn logout(State(app_state): State<AppState>, req: Request) -> AppResult<Response> {
    if let Some(token) = extract_token(&req) {
        // Per security policy: on logout, all sessions of the affected user are
        // deleted — not just the current one — so a user logging out from one
        // device invalidates all other open sessions too.
        let user_id = app_state
            .db
            .sessions
            .get_user_id(&hash_token(&token))
            .await?;
        if let Some(user_id) = user_id {
            app_state.db.sessions.delete_for_user(user_id).await?;
        }
    }
    let cookie = build_session_cookie("", 0, app_state.cfg.secure_cookies);
    let mut response = Json(serde_json::json!({"ok": true})).into_response();
    let cookie_value = cookie
        .parse()
        .map_err(|_| AppError::Internal("Failed to clear session cookie header.".into()))?;
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie_value);
    Ok(response)
}

pub async fn me(
    State(app_state): State<AppState>,
    user: User,
    req: Request,
) -> AppResult<Json<serde_json::Value>> {
    // Expose the CSRF token to the SPA so it can include it on subsequent
    // state-changing requests as `X-CSRF-Token`.
    let raw_token = extract_token(&req).unwrap_or_default();
    let csrf_token = app_state
        .db
        .sessions
        .get_csrf_token(&hash_token(&raw_token))
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
        "can_view_dashboard": !crate::roles::is_assistant_role(&user.role),
        "can_view_reports": true,
    });
    let is_assistant = crate::roles::is_assistant_role(&user.role);
    let mut navigation_items = vec![
        serde_json::json!({"href":"/time","key":"Time","icon":"⏱"}),
        serde_json::json!({"href":"/absences","key":"Absences","icon":"📅"}),
        serde_json::json!({"href":"/calendar","key":"Calendar","icon":"🗓"}),
    ];
    if !is_assistant {
        navigation_items
            .push(serde_json::json!({"href":"/dashboard","key":"Dashboard","icon":"🔔"}));
    }
    navigation_items.push(serde_json::json!({"href":"/reports","key":"Reports","icon":"📊"}));
    navigation_items.push(serde_json::json!({"href":"/account","key":"Account","icon":"👤"}));
    if user.is_lead() {
        navigation_items
            .push(serde_json::json!({"href":"/team-settings","key":"TeamSettings","icon":"🛡"}));
    }
    if user.is_admin() {
        navigation_items.push(serde_json::json!({"href":"/admin/settings","key":"Admin","icon":"⚙"}));
    }
    let home = if is_assistant { "/time" } else { "/dashboard" };
    // For admins: flag whether initial setup (country, working-time defaults,
    // and admin profile name) has been completed. Until it is, the SPA
    // redirects to /admin/settings.
    let must_configure_settings = if user.is_admin() {
        let country = app_state.db.settings.get_raw("country").await?;
        let default_weekly_hours = app_state
            .db
            .settings
            .get_raw("default_weekly_hours")
            .await?;
        let default_annual_leave_days = app_state
            .db
            .settings
            .get_raw("default_annual_leave_days")
            .await?;
        let needs_name = user.first_name.is_empty() || user.last_name.is_empty();
        country.is_none_or(|value| value.is_empty())
            || default_weekly_hours.is_none_or(|value| value.is_empty())
            || default_annual_leave_days.is_none_or(|value| value.is_empty())
            || needs_name
    } else {
        false
    };
    let approver_ids = app_state
        .db
        .users
        .get_approver_ids(user.id)
        .await
        .unwrap_or_default();
    let approvers: Vec<serde_json::Value> = app_state
        .db
        .users
        .get_approver_details(user.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(id, first_name, last_name)| {
            serde_json::json!({"id": id, "first_name": first_name, "last_name": last_name})
        })
        .collect();
    Ok(Json(serde_json::json!({
        "id": user.id, "email": user.email,
        "first_name": user.first_name, "last_name": user.last_name,
        "role": user.role, "weekly_hours": user.weekly_hours,
        "workdays_per_week": user.workdays_per_week,
        "start_date": user.start_date,
        "overtime_start_balance_min": user.overtime_start_balance_min,
        "active": user.active, "must_change_password": user.must_change_password,
        "must_configure_settings": must_configure_settings,
        "approver_ids": approver_ids,
        "approvers": approvers,
        "allow_reopen_without_approval": user.allow_reopen_without_approval,
        "dark_mode": user.dark_mode,
        "csrf_token": csrf_token.unwrap_or_default(),
        "permissions": permissions,
        "nav": navigation_items,
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
    app_state
        .db
        .users
        .update_dark_mode(user.id, body.dark_mode)
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
        if !verify_password_async(current_password.to_string(), user.password_hash.clone()).await {
            return Err(AppError::BadRequest(
                "Current password is incorrect.".into(),
            ));
        }
    }
    validate_password_strength(&body.new_password)?;
    if verify_password_async(body.new_password.clone(), user.password_hash.clone()).await {
        return Err(AppError::BadRequest(
            "New password must differ from the current one.".into(),
        ));
    }
    let new_password_hash = hash_password_async(body.new_password.clone()).await?;
    let current_token_hash = hash_token(&raw_token);
    let mut transaction = app_state.pool.begin().await?;
    UserDb::update_password(&mut transaction, user.id, &new_password_hash, false).await?;
    // On password change, all OTHER sessions for this user are revoked, but
    // the caller's current session is preserved so they remain logged in.
    crate::repository::SessionDb::delete_except_tx(&mut transaction, user.id, &current_token_hash)
        .await?;
    transaction.commit().await?;
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
            parse_origin_parts(allowed).is_some_and(|allowed_parts| allowed_parts == req_parts)
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
    let session_info = app_state.db.sessions.get_session_info(&token_hash).await?;
    let session_info = session_info.ok_or(AppError::Unauthorized)?;
    let (user_id, session_created_at, session_last_active_at, csrf_token) = (
        session_info.user_id,
        session_info.created_at,
        session_info.last_active_at,
        session_info.csrf_token,
    );
    let now = Utc::now();
    // Enforce BOTH the absolute lifetime (since creation) and the sliding idle
    // timeout (since last activity) directly in the middleware, so we never
    // depend on the background cleanup task for authn correctness.
    if now - session_created_at > Duration::hours(ABSOLUTE_TIMEOUT_HOURS)
        || now - session_last_active_at > Duration::hours(IDLE_TIMEOUT_HOURS)
    {
        app_state.db.sessions.delete(&token_hash).await?;
        return Err(AppError::Unauthorized);
    }

    enforce_csrf(&parts, &app_state, &csrf_token).await?;

    app_state.db.sessions.touch(&token_hash).await?;
    let repo_user = app_state
        .db
        .users
        .find_by_id_active(user_id)
        .await?
        .ok_or(AppError::Unauthorized)?;
    let user = User {
        id: repo_user.id,
        email: repo_user.email,
        password_hash: repo_user.password_hash,
        first_name: repo_user.first_name,
        last_name: repo_user.last_name,
        role: repo_user.role,
        weekly_hours: repo_user.weekly_hours,
        workdays_per_week: repo_user.workdays_per_week,
        start_date: repo_user.start_date,
        active: repo_user.active,
        must_change_password: repo_user.must_change_password,
        created_at: repo_user.created_at,
        allow_reopen_without_approval: repo_user.allow_reopen_without_approval,
        dark_mode: repo_user.dark_mode,
        overtime_start_balance_min: repo_user.overtime_start_balance_min,
    };
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
    let sessions = crate::repository::SessionDb::new(pool);
    loop {
        interval.tick().await;
        sessions
            .cleanup_expired_sessions(IDLE_TIMEOUT_HOURS, ABSOLUTE_TIMEOUT_HOURS)
            .await;
        sessions.cleanup_login_attempts().await;
        sessions.cleanup_reset_tokens().await;
    }
}

// ---------------------------------------------------------------------------
// Initial setup (first-boot admin creation)
// ---------------------------------------------------------------------------

/// Returns whether the application needs initial setup (no users exist yet).
pub async fn setup_status(State(app_state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    let user_count = app_state.db.users.count().await?;
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

    let password_hash = hash_password_async(body.password.clone()).await?;
    let today = crate::settings::app_today(&app_state.pool).await;

    // Prevent race conditions where two concurrent requests both observe
    // zero users and both insert an admin. `pool.begin()` runs at the
    // default READ COMMITTED isolation, which does NOT serialize the
    // SELECT/INSERT pair on its own. We take a transaction-scoped Postgres
    // advisory lock so any concurrent setup call blocks until ours commits,
    // and then sees the row we just inserted.
    let mut transaction = app_state.pool.begin().await?;
    UserDb::lock_user_graph_tx(&mut transaction).await?;
    let existing_user_count = UserDb::count_tx(&mut transaction).await?;
    if existing_user_count > 0 {
        tracing::warn!(target: "zerf::auth", "POST /auth/setup called after initial setup is already complete — possible probing");
        return Err(AppError::BadRequest(
            "Setup has already been completed.".into(),
        ));
    }
    let new_user_id = UserDb::create_initial_admin(
        &mut transaction,
        &email,
        &password_hash,
        &first_name,
        &last_name,
        today,
    )
    .await?;
    let current_year = crate::settings::app_current_year(&app_state.pool).await;
    let default_leave_days = UserDb::get_default_leave_days_tx(&mut transaction).await?;
    UserDb::set_leave_days_tx(
        &mut transaction,
        new_user_id,
        current_year,
        default_leave_days,
    )
    .await?;
    UserDb::set_leave_days_tx(
        &mut transaction,
        new_user_id,
        current_year + 1,
        default_leave_days,
    )
    .await?;
    transaction.commit().await?;

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
        tracing::warn!(target: "zerf::auth", "forgot_password called but SMTP is not configured");
        return Err(crate::error::AppError::BadRequest(
            "password_reset_unavailable".into(),
        ));
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
            tracing::warn!(target: "zerf::auth", "forgot_password called but ZERF_PUBLIC_URL is not configured");
            return Err(crate::error::AppError::BadRequest(
                "password_reset_unavailable".into(),
            ));
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
    let rate_limit_key = format!("reset:{}", email);
    let reset_attempts = app_state
        .db
        .sessions
        .count_reset_attempts(&rate_limit_key, since)
        .await;
    if reset_attempts >= 3 {
        // Silently return OK to prevent enumeration / timing leaks.
        return Ok(Json(serde_json::json!({ "ok": true })));
    }
    // Record this reset attempt for rate-limiting purposes.
    app_state
        .db
        .sessions
        .record_reset_attempt(&rate_limit_key)
        .await;

    let user = app_state
        .db
        .sessions
        .get_active_user_by_email(&email)
        .await?;

    // Always return 200 to prevent email enumeration.
    let Some((user_id, user_email)) = user else {
        return Ok(Json(serde_json::json!({ "ok": true })));
    };

    let raw_token = new_token();
    let token_hash = hash_token(&raw_token);
    let expires_at = Utc::now() + Duration::hours(PASSWORD_RESET_TTL_HOURS);

    app_state
        .db
        .sessions
        .upsert_reset_token(&token_hash, user_id, expires_at)
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

    // Check for an expired token before password validation so callers receive
    // a meaningful error even when the supplied password is too short.
    app_state
        .db
        .sessions
        .check_and_consume_expired_token(&token_hash)
        .await?;

    validate_password_strength(&body.password)?;
    let new_hash = hash_password_async(body.password.clone()).await?;

    let password = body.password;
    let reuse_check =
        move |current_hash: &str| -> bool { verify_password(&password, current_hash) };

    app_state
        .db
        .sessions
        .consume_reset_token_and_update_password_checked(&token_hash, &new_hash, Some(&reuse_check))
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
