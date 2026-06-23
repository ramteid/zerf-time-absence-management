//! HTTP handlers for authentication: login, logout, me, password change,
//! initial setup, and password reset.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::{
    build_session_cookie, enforce_same_origin_headers, extract_token, hash_token, User,
    ABSOLUTE_TIMEOUT_HOURS,
};
use crate::services::auth::{
    hash_password_async, new_token, validate_password_strength, verify_password,
    verify_password_async,
};
use crate::AppState;
use axum::extract::{Request, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::sync::Arc;

// Re-used constant from middleware
const LOCKOUT_MIN: i64 = crate::middleware::auth::LOCKOUT_MIN;
const MAX_FAILED_LOGINS: i64 = crate::middleware::auth::MAX_FAILED_LOGINS;
const PASSWORD_RESET_TTL_HOURS: i64 = 1;

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
            hire_date: u.hire_date,
            active: u.active,
            must_change_password: u.must_change_password,
            created_at: u.created_at,
            allow_reopen_without_approval: u.allow_reopen_without_approval,
            allow_submission_without_approval: u.allow_submission_without_approval,
            dark_mode: u.dark_mode,
            overtime_start_balance_min: u.overtime_start_balance_min,
            tracks_time: u.tracks_time,
            annual_leave_days: u.annual_leave_days,
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
    // Admins with tracks_time=false are in pure-admin mode: their own time
    // tracking / absence views are hidden, but they still need Dashboard and
    // Reports so they can approve work and inspect team data.
    let has_time_tracking = user.tracks_time;
    let mut navigation_items = vec![];
    if has_time_tracking {
        navigation_items.push(serde_json::json!({"href":"/time","key":"Time","icon":"⏱"}));
        navigation_items.push(serde_json::json!({"href":"/absences","key":"Absences","icon":"📅"}));
        navigation_items.push(serde_json::json!({"href":"/calendar","key":"Calendar","icon":"🗓"}));
    } else if user.is_lead() {
        // Pure-admin users (tracks_time=false) still need the Calendar to
        // coordinate team absences even though they have no own time/absence data.
        navigation_items.push(serde_json::json!({"href":"/calendar","key":"Calendar","icon":"🗓"}));
    }
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
        navigation_items
            .push(serde_json::json!({"href":"/admin/settings","key":"Admin","icon":"⚙"}));
    }
    // Assistants go to /time (no dashboard); everyone else lands on /dashboard.
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
        "hire_date": user.hire_date,
        "overtime_start_balance_min": user.overtime_start_balance_min,
        "active": user.active, "must_change_password": user.must_change_password,
        "must_configure_settings": must_configure_settings,
        "approver_ids": approver_ids,
        "approvers": approvers,
        "allow_reopen_without_approval": user.allow_reopen_without_approval,
        "allow_submission_without_approval": user.allow_submission_without_approval,
        "dark_mode": user.dark_mode,
        "tracks_time": user.tracks_time,
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
    crate::services::auth::change_password(
        &app_state,
        &user,
        &raw_token,
        body.current_password,
        body.new_password,
    )
    .await?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub email: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    /// Whether this admin tracks their own working time.
    /// Defaults to TRUE when omitted; set to FALSE for a pure-admin account.
    #[serde(default = "default_tracks_time")]
    pub tracks_time: bool,
}

fn default_tracks_time() -> bool {
    true
}

/// Returns whether the application needs initial setup (no users exist yet).
pub async fn setup_status(State(app_state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    let user_count = app_state.db.users.count().await?;
    Ok(Json(serde_json::json!({ "needs_setup": user_count == 0 })))
}

/// Create the initial admin user. Only works when no users exist yet.
pub async fn setup(
    State(app_state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> AppResult<Json<serde_json::Value>> {
    crate::services::auth::create_initial_admin(
        &app_state,
        body.email,
        body.password,
        body.first_name,
        body.last_name,
        body.tracks_time,
    )
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Forgot / reset password (unauthenticated)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ForgotPasswordReq {
    pub email: String,
}

pub async fn forgot_password(
    State(app_state): State<AppState>,
    Json(body): Json<ForgotPasswordReq>,
) -> AppResult<Json<serde_json::Value>> {
    let smtp = crate::services::settings::load_smtp_config(&app_state.pool).await;
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
    let Some((user_id, user_email, first_name, last_name)) = user else {
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

    let smtp = crate::services::settings::load_smtp_config(&app_state.pool).await;
    crate::email::send_async(
        smtp.map(Arc::new),
        user_email,
        format!("{} {}", first_name, last_name),
        subject,
        body_text,
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_default_tracks_time_is_true() {
        assert!(default_tracks_time());
    }

    /// `SetupRequest` deserialisation must set `tracks_time = true` when the
    /// field is omitted, and honour an explicit `false` value when provided.
    #[test]
    fn setup_request_tracks_time_defaults_to_true_when_absent() {
        let without_field: SetupRequest = serde_json::from_value(serde_json::json!({
            "email": "admin@example.com",
            "password": "StrongPass1!",
            "first_name": "Ada",
            "last_name": "Lovelace"
        }))
        .unwrap();
        assert!(
            without_field.tracks_time,
            "tracks_time must default to true when omitted"
        );

        let with_false: SetupRequest = serde_json::from_value(serde_json::json!({
            "email": "admin@example.com",
            "password": "StrongPass1!",
            "first_name": "Ada",
            "last_name": "Lovelace",
            "tracks_time": false
        }))
        .unwrap();
        assert!(
            !with_false.tracks_time,
            "tracks_time must honour an explicit false value"
        );
    }

    /// `LoginReq` must deserialise correctly from a standard JSON body.
    #[test]
    fn login_req_deserialises_email_and_password() {
        let req: LoginReq = serde_json::from_value(serde_json::json!({
            "email": "user@example.com",
            "password": "hunter2"
        }))
        .unwrap();
        assert_eq!(req.email, "user@example.com");
        assert_eq!(req.password, "hunter2");
    }

    /// `ForgotPasswordReq` must deserialise correctly.
    #[test]
    fn forgot_password_req_deserialises_email() {
        let req: ForgotPasswordReq =
            serde_json::from_value(serde_json::json!({"email": "test@example.com"})).unwrap();
        assert_eq!(req.email, "test@example.com");
    }

    /// `ResetPasswordTokenReq` must deserialise token and password.
    #[test]
    fn reset_password_token_req_deserialises_token_and_password() {
        let req: ResetPasswordTokenReq = serde_json::from_value(serde_json::json!({
            "token": "abc123",
            "password": "NewSecure1!"
        }))
        .unwrap();
        assert_eq!(req.token, "abc123");
        assert_eq!(req.password, "NewSecure1!");
    }

    /// `PasswordReq` must treat a missing `current_password` field as `None`.
    #[test]
    fn password_req_current_password_is_optional() {
        let with_current: PasswordReq = serde_json::from_value(serde_json::json!({
            "current_password": "OldPass1!",
            "new_password": "NewPass1!"
        }))
        .unwrap();
        assert_eq!(with_current.current_password.as_deref(), Some("OldPass1!"));

        let without_current: PasswordReq = serde_json::from_value(serde_json::json!({
            "new_password": "NewPass1!"
        }))
        .unwrap();
        assert!(without_current.current_password.is_none());
    }
}
