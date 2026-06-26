//! Auth middleware: session validation, User extractor, cookie/CSRF helpers.
//! This module is the single source of the `User` type.

use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

// Session timing policy — also referenced by services::auth::cleanup_loop.
pub const SESSION_COOKIE_SECURE: &str = "__Host-zerf_session";
pub const SESSION_COOKIE_PLAIN: &str = "zerf_session";
pub const ABSOLUTE_TIMEOUT_HOURS: i64 = 168; // 7 days absolute timeout (since session creation)
pub const IDLE_TIMEOUT_HOURS: i64 = 8; // sliding idle timeout (since last_active_at)
pub const MAX_FAILED_LOGINS: i64 = 5;
pub const LOCKOUT_MIN: i64 = 15;

pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SESSION_COOKIE_SECURE
    } else {
        SESSION_COOKIE_PLAIN
    }
}

#[derive(Debug, Clone, Serialize)]
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
    /// Optional employment start date that anchors annual-leave proration.
    /// Falls back to `start_date` when `None` — see `absence_balance::leave_entitlement_anchor`.
    pub hire_date: Option<chrono::NaiveDate>,
    pub active: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    /// When TRUE, this user's reopen requests are auto-approved without waiting
    /// for manual review. Silent: no notifications or emails are sent to
    /// anyone (requester or approvers).
    pub allow_reopen_without_approval: bool,
    /// When TRUE, this user's submitted weeks are auto-approved (draft ->
    /// approved directly) without requiring approver action. Silent: no
    /// notifications or emails are sent to anyone (requester or approvers).
    pub allow_submission_without_approval: bool,
    pub dark_mode: bool,
    pub overtime_start_balance_min: i64,
    /// When FALSE (admin only), this user operates in pure-admin mode: no time
    /// entries or absences are tracked, all related endpoints are blocked, and
    /// the corresponding navigation items are hidden in the frontend.
    pub tracks_time: bool,
    /// Base annual leave entitlement (days/year), used whenever no explicit
    /// `user_annual_leave` override exists for a given year.
    pub annual_leave_days: i64,
    /// Set when the user has been archived. Archived users cannot log in.
    /// Cleared on restore.
    pub archived_at: Option<DateTime<Utc>>,
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

/// Hash a raw session token with SHA-256 before storing in the DB.
/// The cookie always carries the raw token; only the hash is persisted,
/// so a DB breach cannot be used to directly replay session cookies.
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn build_session_cookie(token: &str, max_age: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    let name = cookie_name(secure);
    format!("{name}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure_flag}")
}

/// Extract the raw session token from an Axum `Request`'s cookie header.
pub fn extract_token(req: &Request) -> Option<String> {
    let cookie_header = req.headers().get(header::COOKIE)?.to_str().ok()?;
    extract_token_from_cookie_str(cookie_header)
}

fn extract_token_from_cookie_str(cookie_str: &str) -> Option<String> {
    extract_token_from_cookie_str_secure(cookie_str, false)
}

pub fn extract_token_from_cookie_str_secure(cookie_str: &str, secure_only: bool) -> Option<String> {
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

/// Pure inner logic for same-origin checking: returns `true` when either the
/// `Origin` or (as fallback) the `Referer` header matches one of the
/// `allowed_origins` entries.  Extracted from `enforce_same_origin_headers` so
/// it can be exercised by unit tests without needing an `AppState`.
pub fn check_origin_allowed(
    header_origin: Option<&str>,
    header_referer: Option<&str>,
    allowed_origins: &[String],
) -> bool {
    let origin_matches = |origin_value: &str| {
        let Some(req_parts) = parse_origin_parts(origin_value) else {
            return false;
        };
        allowed_origins.iter().any(|allowed| {
            parse_origin_parts(allowed).is_some_and(|allowed_parts| allowed_parts == req_parts)
        })
    };
    match (header_origin, header_referer) {
        (Some(origin), _) => origin_matches(origin),
        (None, Some(referer)) => origin_matches(referer),
        (None, None) => false,
    }
}

pub fn enforce_same_origin_headers(
    headers: &axum::http::HeaderMap,
    app_state: &AppState,
) -> AppResult<()> {
    if !app_state.cfg.enforce_origin {
        return Ok(());
    }
    let header_origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let header_referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok());
    let allowed_origins = &app_state.cfg.allowed_origins;
    if !check_origin_allowed(header_origin, header_referer, allowed_origins) {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Pure CSRF-token comparison: returns `true` when `header_token` is
/// non-empty and matches `csrf_token` in constant time.
pub fn csrf_token_matches(header_token: &str, csrf_token: &str) -> bool {
    use subtle::ConstantTimeEq;
    !header_token.is_empty()
        && header_token
            .as_bytes()
            .ct_eq(csrf_token.as_bytes())
            .unwrap_u8()
            == 1
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
    if let Err(e) = enforce_same_origin_headers(&parts.headers, app_state) {
        let origin = parts.headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
        let referer = parts.headers.get(header::REFERER).and_then(|v| v.to_str().ok());
        tracing::warn!(
            target: "zerf::auth",
            method = %parts.method,
            path = %parts.uri.path(),
            ?origin,
            ?referer,
            allowed = ?app_state.cfg.allowed_origins,
            "CSRF origin check failed"
        );
        return Err(e);
    }
    if !app_state.cfg.enforce_csrf {
        return Ok(());
    }
    let header_token = parts
        .headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !csrf_token_matches(header_token, csrf_token) {
        tracing::warn!(
            target: "zerf::auth",
            method = %parts.method,
            path = %parts.uri.path(),
            "CSRF token mismatch"
        );
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
        hire_date: repo_user.hire_date,
        active: repo_user.active,
        must_change_password: repo_user.must_change_password,
        created_at: repo_user.created_at,
        allow_reopen_without_approval: repo_user.allow_reopen_without_approval,
        allow_submission_without_approval: repo_user.allow_submission_without_approval,
        dark_mode: repo_user.dark_mode,
        overtime_start_balance_min: repo_user.overtime_start_balance_min,
        tracks_time: repo_user.tracks_time,
        annual_leave_days: repo_user.annual_leave_days,
        archived_at: repo_user.archived_at,
    };

    // Enforce must_change_password: users with a temporary password are only
    // allowed to access identity and password-change endpoints. All other API
    // calls are blocked until the password is changed. This prevents temporary
    // credentials from being used to access sensitive data.
    // Exception: admins may reset other users' passwords even before changing
    // their own, so they can onboard new team members during initial setup.
    if user.must_change_password {
        let request_path = parts.uri.path();
        let allowed_paths = [
            "/auth/me",
            "/auth/password",
            "/auth/logout",
            "/auth/preferences",
            "/settings/public",
        ];
        let is_admin_reset_password = user.is_admin()
            && parts.method == Method::POST
            && request_path.starts_with("/users/")
            && request_path.ends_with("/reset-password");
        if !allowed_paths.contains(&request_path) && !is_admin_reset_password {
            tracing::warn!(
                target: "zerf::auth",
                user_id = user.id,
                path = request_path,
                method = %parts.method,
                role = &user.role,
                "must_change_password gate: blocked"
            );
            return Err(AppError::Forbidden);
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_user(role: &str) -> User {
        User {
            id: 1,
            email: "user@example.com".to_string(),
            password_hash: "hash".to_string(),
            first_name: "Ada".to_string(),
            last_name: "Lovelace".to_string(),
            role: role.to_string(),
            weekly_hours: 40.0,
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
            annual_leave_days: 30,
            archived_at: None,
        }
    }

    #[test]
    fn user_role_helpers_follow_normalized_role_rules() {
        let admin = sample_user(" Admin ");
        assert!(admin.is_admin());
        assert!(admin.is_lead());

        let lead = sample_user("team_lead");
        assert!(!lead.is_admin());
        assert!(lead.is_lead());

        let employee = sample_user("employee");
        assert!(!employee.is_admin());
        assert!(!employee.is_lead());
        assert_eq!(employee.full_name(), "Ada Lovelace");
    }

    #[test]
    fn cookie_name_switches_between_secure_and_plain() {
        assert_eq!(cookie_name(true), "__Host-zerf_session");
        assert_eq!(cookie_name(false), "zerf_session");
    }

    #[test]
    fn build_session_cookie_contains_expected_flags() {
        let secure = build_session_cookie("token", 3600, true);
        assert!(secure.contains("__Host-zerf_session=token"));
        assert!(secure.contains("HttpOnly"));
        assert!(secure.contains("SameSite=Strict"));
        assert!(secure.contains("Max-Age=3600"));
        assert!(secure.contains("; Secure"));

        let insecure = build_session_cookie("token", 60, false);
        assert!(insecure.contains("zerf_session=token"));
        assert!(!insecure.contains("; Secure"));
    }

    // Cookie extraction is FIFO: the first matching cookie in the string wins.
    // In secure_only=false mode BOTH names are accepted; in secure_only=true
    // only __Host- is accepted regardless of string position.
    #[test]
    fn extract_token_returns_first_matching_cookie_and_respects_secure_mode() {
        // plain cookie comes first → plain is returned in non-secure mode
        let mixed = "foo=bar; zerf_session=plain; __Host-zerf_session=secure";
        assert_eq!(
            extract_token_from_cookie_str_secure(mixed, false),
            Some("plain".to_string())
        );
        // secure_only=true ignores plain cookie even when it comes first
        assert_eq!(
            extract_token_from_cookie_str_secure(mixed, true),
            Some("secure".to_string())
        );

        // __Host- cookie comes first → it is returned (FIFO, not preference)
        let host_first = "foo=bar; __Host-zerf_session=secure; zerf_session=plain";
        assert_eq!(
            extract_token_from_cookie_str_secure(host_first, false),
            Some("secure".to_string())
        );

        let plain_only = "foo=bar; zerf_session=plain";
        assert_eq!(
            extract_token_from_cookie_str_secure(plain_only, false),
            Some("plain".to_string())
        );
        assert_eq!(extract_token_from_cookie_str_secure(plain_only, true), None);
    }

    #[test]
    fn parse_origin_parts_normalizes_and_derives_ports() {
        assert_eq!(
            parse_origin_parts("https://Example.COM."),
            Some(("https".to_string(), "example.com".to_string(), 443))
        );
        assert_eq!(
            parse_origin_parts("http://example.com:8080/path?x=1"),
            Some(("http".to_string(), "example.com".to_string(), 8080))
        );
        assert_eq!(
            parse_origin_parts("custom://host"),
            Some(("custom".to_string(), "host".to_string(), 0))
        );
        assert_eq!(parse_origin_parts("example.com"), None);
    }

    // ── check_origin_allowed ──────────────────────────────────────────────────

    fn origins(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// An exact match on the Origin header must be accepted.
    #[test]
    fn check_origin_allowed_accepts_matching_origin() {
        let allowed = origins(&["https://app.example.com"]);
        assert!(check_origin_allowed(
            Some("https://app.example.com"),
            None,
            &allowed
        ));
    }

    /// A different scheme (http vs https) must be rejected even if host/port match.
    #[test]
    fn check_origin_allowed_rejects_wrong_scheme() {
        let allowed = origins(&["https://app.example.com"]);
        assert!(!check_origin_allowed(
            Some("http://app.example.com"),
            None,
            &allowed
        ));
    }

    /// A different host must be rejected even if the scheme matches.
    #[test]
    fn check_origin_allowed_rejects_wrong_host() {
        let allowed = origins(&["https://app.example.com"]);
        assert!(!check_origin_allowed(
            Some("https://evil.example.com"),
            None,
            &allowed
        ));
    }

    /// When no Origin header is present the Referer is used as a fallback.
    #[test]
    fn check_origin_allowed_falls_back_to_referer() {
        let allowed = origins(&["https://app.example.com"]);
        // No Origin, Referer contains a path → still accepted if host matches.
        assert!(check_origin_allowed(
            None,
            Some("https://app.example.com/some/path?q=1"),
            &allowed
        ));
    }

    /// When both Origin and Referer are absent the request must be rejected.
    #[test]
    fn check_origin_allowed_rejects_when_both_headers_absent() {
        let allowed = origins(&["https://app.example.com"]);
        assert!(!check_origin_allowed(None, None, &allowed));
    }

    /// An unparseable Origin value (no scheme) must be rejected.
    #[test]
    fn check_origin_allowed_rejects_unparseable_origin() {
        let allowed = origins(&["https://app.example.com"]);
        assert!(!check_origin_allowed(
            Some("not-a-valid-origin"),
            None,
            &allowed
        ));
    }

    /// Origin with an explicit non-standard port must match exactly.
    #[test]
    fn check_origin_allowed_matches_non_standard_port() {
        let allowed = origins(&["http://localhost:3000"]);
        assert!(check_origin_allowed(
            Some("http://localhost:3000"),
            None,
            &allowed
        ));
        // Port 3001 ≠ 3000 → rejected.
        assert!(!check_origin_allowed(
            Some("http://localhost:3001"),
            None,
            &allowed
        ));
    }

    /// Multiple allowed origins: any one matching is sufficient.
    #[test]
    fn check_origin_allowed_accepts_any_from_allowed_list() {
        let allowed = origins(&["https://app.example.com", "https://admin.example.com"]);
        assert!(check_origin_allowed(
            Some("https://admin.example.com"),
            None,
            &allowed
        ));
    }

    // ── csrf_token_matches ────────────────────────────────────────────────────

    /// Identical tokens must match in constant time.
    #[test]
    fn csrf_token_matches_accepts_identical_tokens() {
        assert!(csrf_token_matches("secret-token-123", "secret-token-123"));
    }

    /// A different token must be rejected.
    #[test]
    fn csrf_token_matches_rejects_wrong_token() {
        assert!(!csrf_token_matches("bad-token", "secret-token-123"));
    }

    /// An empty header token must always be rejected, even if the session
    /// CSRF token were somehow also empty.
    #[test]
    fn csrf_token_matches_rejects_empty_header_token() {
        assert!(!csrf_token_matches("", "secret-token-123"));
        assert!(!csrf_token_matches("", ""));
    }

    /// `extract_token` must read the session token from a real axum Request's
    /// Cookie header, and return None when no matching cookie is present.
    #[test]
    fn extract_token_reads_cookie_header_from_request() {
        use axum::body::Body;

        let req_with_cookie = Request::builder()
            .header(header::COOKIE, "zerf_session=mytoken123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_token(&req_with_cookie),
            Some("mytoken123".to_string())
        );

        let req_no_cookie = Request::builder().body(Body::empty()).unwrap();
        assert_eq!(extract_token(&req_no_cookie), None);

        let req_wrong_cookie = Request::builder()
            .header(header::COOKIE, "other=value")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req_wrong_cookie), None);
    }

    /// When the authority contains a colon but the right-hand side is not a
    /// valid port number, `parse_origin_parts` must fall back to the scheme's
    /// default port and treat the whole authority as the host.
    #[test]
    fn parse_origin_parts_falls_back_on_non_numeric_port() {
        let result = parse_origin_parts("https://host:notaport");
        assert_eq!(
            result,
            Some(("https".to_string(), "host:notaport".to_string(), 443))
        );
    }
}
