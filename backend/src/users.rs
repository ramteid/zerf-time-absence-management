use crate::audit;
use crate::auth::{hash_password, lock_user_graph, validate_password_strength, User};
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::roles::{
    can_approve_admin_subjects, can_approve_non_admin_subjects, is_admin_role,
    is_assistant_role, is_team_lead_role, normalize_role, ROLE_ASSISTANT,
};
use crate::repository::UserDb;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) fn repo_user_to_auth_user(u: crate::repository::User) -> User {
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
        active: u.active,
        must_change_password: u.must_change_password,
        created_at: u.created_at,
        allow_reopen_without_approval: u.allow_reopen_without_approval,
        dark_mode: u.dark_mode,
        overtime_start_balance_min: u.overtime_start_balance_min,
    }
}

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

pub async fn team_settings_list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<TeamSettings>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rows = if requester.is_admin() {
        app_state.db.users.team_settings_all().await?
    } else {
        app_state
            .db
            .users
            .team_settings_for_lead(requester.id)
            .await?
    };
    let settings_list: Vec<TeamSettings> = rows
        .into_iter()
        .map(|(id, email, first_name, last_name, role, allow_reopen)| TeamSettings {
            user_id: id,
            email,
            first_name,
            last_name,
            role,
            allow_reopen_without_approval: allow_reopen,
        })
        .collect();
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
    // Non-admin leads cannot modify their own reopen policy — only their
    // own approver (a higher lead or admin) may grant them auto-approval.
    if !requester.is_admin() && target_id == requester.id {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only edit their direct reports.
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
    // Transactional read-then-write to prevent TOCTOU races.
    let mut tx = app_state.pool.begin().await?;
    let previous_value: Option<bool> = sqlx::query_scalar(
        "SELECT allow_reopen_without_approval FROM users WHERE id=$1 AND active=TRUE FOR UPDATE",
    )
    .bind(target_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(before) = previous_value else {
        return Err(AppError::BadRequest("User not found or inactive.".into()));
    };
    sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
        .bind(body.allow_reopen_without_approval)
        .bind(target_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "team_settings_updated",
        "users",
        target_id,
        Some(serde_json::json!({"allow_reopen_without_approval": before})),
        Some(serde_json::json!({"allow_reopen_without_approval": body.allow_reopen_without_approval})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<User>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let repo_users = if requester.is_admin() {
        app_state.db.users.find_all_ordered().await?
    } else {
        app_state.db.users.find_for_approver(requester.id).await?
    };
    let user_list: Vec<User> = repo_users.into_iter().map(repo_user_to_auth_user).collect();
    Ok(Json(user_list))
}

pub async fn get_one(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    assert_can_access_user(&app_state, &requester, user_id).await?;
    let user = app_state
        .db
        .users
        .find_by_id(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let approver_ids = app_state
        .db
        .users
        .get_approver_ids(user.id)
        .await
        .unwrap_or_default();
    let user_json = serde_json::json!({
        "id": user.id,
        "email": user.email,
        "first_name": user.first_name,
        "last_name": user.last_name,
        "role": user.role,
        "weekly_hours": user.weekly_hours,
        "workdays_per_week": user.workdays_per_week,
        "start_date": user.start_date,
        "active": user.active,
        "must_change_password": user.must_change_password,
        "created_at": user.created_at,
        "allow_reopen_without_approval": user.allow_reopen_without_approval,
        "dark_mode": user.dark_mode,
        "overtime_start_balance_min": user.overtime_start_balance_min,
        "approver_ids": approver_ids,
    });
    Ok(Json(user_json))
}

#[derive(Deserialize)]
pub struct NewUser {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub weekly_hours: f64,
    #[serde(default)]
    pub workdays_per_week: Option<i16>,
    /// Leave days for the current year (required on creation).
    pub leave_days_current_year: i64,
    /// Leave days for next year (required on creation).
    pub leave_days_next_year: i64,
    pub start_date: NaiveDate,
    pub overtime_start_balance_min: Option<i64>,
    pub password: Option<String>,
    /// Mandatory for non-admin users: list of team leads/admins who can approve this user's submissions.
    #[serde(default)]
    pub approver_ids: Vec<i64>,
}



/// Validate that each approver_id refers to an active lead/admin and is not the user themselves.
/// Also enforces the rule that non-admin users must have at least one approver.
async fn validate_approver_ids(
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

async fn ensure_user_name_available(
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

fn user_unique_conflict(error: &sqlx::Error) -> Option<AppError> {
    let sqlx::Error::Database(db_error) = error else {
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

#[derive(Serialize)]
pub struct CreateResponse {
    pub id: i64,
    pub user: User,
    pub temporary_password: String,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(mut body): Json<NewUser>,
) -> AppResult<Json<CreateResponse>> {
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
    let effective_workdays: i16;
    if is_assistant_role(&body.role) {
        tracing::warn!(
            target: "zerf::assistant_role",
            role = %body.role,
            weekly_hours = body.weekly_hours,
            overtime_start_balance_min = body.overtime_start_balance_min.unwrap_or(0),
            email = %normalized_email,
            "validating assistant invariants during user creation"
        );
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
        effective_workdays = 7;
    } else {
        let wdpw = body.workdays_per_week.unwrap_or(5);
        if !(1..=5).contains(&wdpw) {
            return Err(AppError::BadRequest("Invalid workdays_per_week.".into()));
        }
        effective_workdays = wdpw;
    }
    ensure_email_available(&app_state, &normalized_email, None).await?;
    ensure_user_name_available(&app_state, &first_name, &last_name, None).await?;
    let temporary_password = match body.password {
        Some(provided) if !provided.is_empty() => {
            validate_password_strength(&provided)?;
            provided
        }
        _ => generate_password(),
    };
    let password_hash = hash_password(&temporary_password)?;
    let overtime_balance = body.overtime_start_balance_min.unwrap_or(0);
    if !(-525_600..=525_600).contains(&overtime_balance) {
        return Err(AppError::BadRequest(
            "Invalid overtime_start_balance_min.".into(),
        ));
    }
    let mut transaction = app_state.pool.begin().await?;
    lock_user_graph(&mut transaction).await?;
    validate_approver_ids(&app_state, &body.role, None, &body.approver_ids).await?;
    let new_user_id: i64 = sqlx::query_scalar("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,workdays_per_week,start_date,must_change_password,overtime_start_balance_min) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id")
        .bind(&normalized_email).bind(password_hash).bind(&first_name).bind(&last_name).bind(&body.role)
        .bind(body.weekly_hours).bind(effective_workdays).bind(body.start_date).bind(true)
        .bind(overtime_balance)
        .fetch_one(&mut *transaction).await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "create user insert failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not create user.".into()))
        })?;
    // Insert approver relationships into user_approvers junction table
    for approver_id in &body.approver_ids {
        UserDb::insert_approver_tx(&mut transaction, new_user_id, *approver_id).await?;
    }
    // Seed leave days for current + next year
    let current_year = crate::settings::app_current_year(&app_state.pool).await;
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
    // Send registration email best-effort
    let smtp = crate::settings::load_smtp_config(&app_state.pool)
        .await
        .map(std::sync::Arc::new);
    let login_line = match app_state.cfg.public_url.as_deref() {
        Some(url) => format!("\nURL:      {}\n", url.trim_end_matches('/')),
        None => String::new(),
    };
    let language = i18n::load_ui_language(&app_state.pool)
        .await
        .unwrap_or_default();
    let org_name_raw = crate::settings::load_setting(&app_state.pool, "organization_name", "")
        .await
        .unwrap_or_default();
    let org_name = if org_name_raw.trim().is_empty() {
        "Zerf".to_string()
    } else {
        org_name_raw
    };
    let subject = i18n::translate(&language, "account_created_subject", &[("org_name", org_name)]);
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
    crate::email::send_async(smtp, normalized_email, subject, body_text);
    Ok(Json(CreateResponse {
        id: new_user_id,
        user: created_auth_user,
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
    pub workdays_per_week: Option<i16>,
    /// If provided, sets leave days for the current year.
    pub leave_days_current_year: Option<i64>,
    /// If provided, sets leave days for next year.
    pub leave_days_next_year: Option<i64>,
    pub start_date: Option<NaiveDate>,
    pub active: Option<bool>,
    /// List of approvers (team leads/admins) for this user.
    /// If provided (even as empty list), replaces all existing approvers.
    #[serde(default, deserialize_with = "deserialize_optional_vec")]
    pub approver_ids: Option<Vec<i64>>,
    pub allow_reopen_without_approval: Option<bool>,
    pub overtime_start_balance_min: Option<i64>,
}

fn deserialize_optional_vec<'de, D>(de: D) -> Result<Option<Vec<i64>>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    match Option::<Vec<i64>>::deserialize(de)? {
        None => Ok(None),
        Some(v) => Ok(Some(v)),
    }
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
    let normalized_role = body.role.as_ref().map(|role_value| normalize_role(role_value));
    if let Some(role_value) = &normalized_role {
        if !["employee", "team_lead", "admin", ROLE_ASSISTANT].contains(&role_value.as_str()) {
            return Err(AppError::BadRequest("Invalid role".into()));
        }
    }
    // Anti-lockout: an admin cannot demote themselves out of admin or deactivate
    // their own account; otherwise the only path back is fresh DB bootstrap.
    if user_id == requester.id {
        if let Some(role_value) = &body.role {
            if !is_admin_role(role_value) {
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
    if let Some(workdays_per_week) = body.workdays_per_week {
        if !(1..=5).contains(&workdays_per_week) {
            return Err(AppError::BadRequest("Invalid workdays_per_week.".into()));
        }
    }
    if let Some(d) = body.leave_days_current_year {
        if !(0..=366).contains(&d) {
            return Err(AppError::BadRequest("Invalid leave_days.".into()));
        }
    }
    if let Some(d) = body.leave_days_next_year {
        if !(0..=366).contains(&d) {
            return Err(AppError::BadRequest("Invalid leave_days.".into()));
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
    let mut transaction = app_state.pool.begin().await?;
    lock_user_graph(&mut transaction).await?;
    let previous_user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, workdays_per_week, start_date, active, must_change_password, created_at, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1 FOR UPDATE")
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await?;
    if let Some(email) = &normalized_email {
        ensure_email_available(&app_state, email, Some(user_id)).await?;
    }
    if first_name.is_some() || last_name.is_some() {
        let updated_first_name = first_name
            .clone()
            .unwrap_or_else(|| previous_user.first_name.clone());
        let updated_last_name = last_name
            .clone()
            .unwrap_or_else(|| previous_user.last_name.clone());
        ensure_user_name_available(
            &app_state,
            &updated_first_name,
            &updated_last_name,
            Some(user_id),
        )
        .await?;
    }
    let removing_admin_rights = is_admin_role(&previous_user.role)
        && (normalized_role
            .as_deref()
            .is_some_and(|role_value| role_value != "admin")
            || matches!(body.active, Some(false)));
    // Pre-validate the post-update invariant (non-admin → has approver).
    let new_role = normalized_role.unwrap_or_else(|| previous_user.role.trim().to_ascii_lowercase());
    let effective_weekly_hours = body.weekly_hours.unwrap_or(previous_user.weekly_hours);
    let effective_overtime_start_balance = body
        .overtime_start_balance_min
        .unwrap_or(previous_user.overtime_start_balance_min);
    if is_assistant_role(&new_role) {
        tracing::warn!(
            target: "zerf::assistant_role",
            user_id,
            previous_role = %previous_user.role,
            new_role = %new_role,
            effective_weekly_hours,
            effective_overtime_start_balance,
            "validating assistant invariants during user update"
        );
        if effective_weekly_hours != 0.0 {
            return Err(AppError::BadRequest(
                "Assistants must have weekly_hours set to 0.".into(),
            ));
        }
        if effective_overtime_start_balance != 0 {
            return Err(AppError::BadRequest(
                "Assistants cannot have an overtime start balance.".into(),
            ));
        }
        if body.workdays_per_week.is_some() {
            return Err(AppError::BadRequest(
                "Assistants cannot have fixed working days per week.".into(),
            ));
        }
    }
    // For assistants force workdays_per_week=7 (no fixed days); for others use what was provided.
    let effective_workdays_update: Option<i16> = if is_assistant_role(&new_role) {
        Some(7)
    } else {
        body.workdays_per_week
    };
    let effective_approver_ids = if let Some(approver_ids) = &body.approver_ids {
        approver_ids.clone()
    } else {
        sqlx::query_scalar(
            "SELECT approver_id FROM user_approvers WHERE user_id=$1 ORDER BY approver_id",
        )
        .bind(user_id)
        .fetch_all(&mut *transaction)
        .await?
    };
    validate_approver_ids(
        &app_state,
        &new_role,
        Some(user_id),
        &effective_approver_ids,
    )
    .await?;

    let resulting_active = body.active.unwrap_or(previous_user.active);
    if !can_approve_admin_subjects(&new_role, resulting_active) {
        let admin_direct_reports_count = app_state
            .db
            .users
            .count_admin_direct_reports(user_id)
            .await?;
        if admin_direct_reports_count > 0 {
            return Err(AppError::BadRequest(format!(
                "Cannot change this user to a non-admin approver: {} active admin user(s) still have them as their approver. Reassign them first.",
                admin_direct_reports_count
            )));
        }
    }
    if !can_approve_non_admin_subjects(&new_role, resulting_active) {
        let non_admin_direct_reports_count =
            app_state.db.users.count_direct_reports(user_id).await?;
        if non_admin_direct_reports_count > 0 {
            return Err(AppError::BadRequest(format!(
                "Cannot change this user to a non-approver: {} user(s) still have them as their approver. Reassign them first.",
                non_admin_direct_reports_count
            )));
        }
    }
    // Last-admin protection: checked while the user graph lock is held.
    if removing_admin_rights && previous_user.active {
        let active_admins = UserDb::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot remove the last active admin.".into(),
            ));
        }
    }
    // Use the normalized role for storage so SQL queries with direct string
    // comparisons (e.g. role = 'admin') work reliably.
    let role_to_store: Option<String> = if body.role.is_some() { Some(new_role.clone()) } else { None };
    sqlx::query("UPDATE users SET email=COALESCE($1,email), first_name=COALESCE($2,first_name), last_name=COALESCE($3,last_name), role=COALESCE($4,role), weekly_hours=COALESCE($5,weekly_hours), workdays_per_week=COALESCE($6,workdays_per_week), start_date=COALESCE($7,start_date), active=COALESCE($8,active), allow_reopen_without_approval=COALESCE($9,allow_reopen_without_approval), overtime_start_balance_min=COALESCE($10,overtime_start_balance_min) WHERE id=$11")
        .bind(normalized_email).bind(first_name).bind(last_name).bind(role_to_store)
        .bind(body.weekly_hours).bind(effective_workdays_update).bind(body.start_date).bind(body.active)
        .bind(body.allow_reopen_without_approval).bind(body.overtime_start_balance_min).bind(user_id)
        .execute(&mut *transaction).await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "update user failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not update user.".into()))
        })?;
    // Update leave days if provided
    let current_year = crate::settings::app_current_year(&app_state.pool).await;
    if let Some(d) = body.leave_days_current_year {
        UserDb::set_leave_days_tx(&mut transaction, user_id, current_year, d).await?;
    }
    if let Some(d) = body.leave_days_next_year {
        UserDb::set_leave_days_tx(&mut transaction, user_id, current_year + 1, d).await?;
    }
    // Handle approver_ids update if provided
    if let Some(new_approver_ids) = &body.approver_ids {
        // Delete all existing approver relationships
        sqlx::query("DELETE FROM user_approvers WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
        // Insert new approver relationships
        for approver_id in new_approver_ids {
            UserDb::insert_approver_tx(&mut transaction, user_id, *approver_id).await?;
        }
    }
    // If role changed or user was deactivated, kill all sessions of that user
    // so cached role/state cannot be (ab)used.
    let previous_role_normalized = normalize_role(&previous_user.role);
    let role_changed = body
        .role
        .as_ref()
        .map(|role_value| normalize_role(role_value) != previous_role_normalized)
        .unwrap_or(false);
    let just_deactivated = matches!(body.active, Some(false)) && previous_user.active;
    if role_changed || just_deactivated {
        let _ = crate::repository::SessionDb::delete_for_user_tx(&mut transaction, user_id).await;
    }
    transaction.commit().await?;
    let updated_user = app_state
        .db
        .users
        .find_by_id(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let updated_auth_user = repo_user_to_auth_user(updated_user);
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "users",
        user_id,
        serde_json::to_value(&previous_user).ok(),
        serde_json::to_value(&updated_auth_user).ok(),
    )
    .await;
    Ok(Json(updated_auth_user))
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
    let mut transaction = app_state.pool.begin().await?;
    lock_user_graph(&mut transaction).await?;
    let previous_user: User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, workdays_per_week, start_date, active, must_change_password, created_at, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1 FOR UPDATE")
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await?;
    if previous_user.active && is_admin_role(&previous_user.role) {
        let active_admins = UserDb::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot remove the last active admin.".into(),
            ));
        }
    }
    // Block deactivation if this person is an assigned approver for active users.
    // Run inside the transaction (under the user-graph lock) to avoid TOCTOU.
    let direct_reports_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_approvers \
         WHERE approver_id=$1 AND user_id IN (SELECT id FROM users WHERE active=TRUE)",
    )
    .bind(user_id)
    .fetch_one(&mut *transaction)
    .await?;
    if direct_reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot deactivate: {} active user(s) still have this person as their approver. Reassign them first.",
            direct_reports_count
        )));
    }
    UserDb::deactivate_tx(&mut transaction, user_id).await?;
    crate::repository::SessionDb::delete_for_user_tx(&mut transaction, user_id).await?;
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "deactivated",
        "users",
        user_id,
        serde_json::to_value(&previous_user).ok(),
        Some(serde_json::json!({"active": false})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn delete_user(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if user_id == requester.id {
        return Err(AppError::BadRequest("You cannot delete yourself.".into()));
    }
    let mut transaction = app_state.pool.begin().await?;
    lock_user_graph(&mut transaction).await?;
    let target_user: User = sqlx::query_as(
        "SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, workdays_per_week, \
         start_date, active, must_change_password, created_at, allow_reopen_without_approval, \
         dark_mode, overtime_start_balance_min FROM users WHERE id=$1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if target_user.active && is_admin_role(&target_user.role) {
        let active_admins = UserDb::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot delete the last active admin.".into(),
            ));
        }
    }
    // Run inside the transaction (under the user-graph lock) to avoid TOCTOU.
    let direct_reports_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_approvers \
         WHERE approver_id=$1 AND user_id IN (SELECT id FROM users WHERE active=TRUE)",
    )
    .bind(user_id)
    .fetch_one(&mut *transaction)
    .await?;
    if direct_reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot delete: {} active user(s) still have this person as their approver. Reassign them first.",
            direct_reports_count
        )));
    }
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "deleted",
        "users",
        user_id,
        serde_json::to_value(&target_user).ok(),
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
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
    let mut transaction = app_state.pool.begin().await?;
    let target_active: Option<bool> =
        sqlx::query_scalar("SELECT active FROM users WHERE id=$1 FOR UPDATE")
            .bind(target_id)
            .fetch_optional(&mut *transaction)
            .await?;
    match target_active {
        Some(true) => {}
        Some(false) => return Err(AppError::BadRequest("User is inactive.".into())),
        None => return Err(AppError::NotFound),
    }
    UserDb::update_password(&mut transaction, target_id, &new_password_hash, true).await?;
    // Force re-authentication: kill any existing sessions for this user.
    crate::repository::SessionDb::delete_for_user_tx(&mut transaction, target_id).await?;
    transaction.commit().await?;
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
    Ok(Json(
        serde_json::json!({"temporary_password": temporary_password}),
    ))
}

// ---------------------------------------------------------------------------
// Annual leave facade — single source of truth backed by user_annual_leave.
// ---------------------------------------------------------------------------

/// Row returned by the leave endpoints.
#[derive(serde::Serialize, sqlx::FromRow)]
pub struct AnnualLeaveRow {
    pub user_id: i64,
    pub year: i32,
    pub days: i64,
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

/// Set the leave days for `user_id` in `year` (upsert).
pub async fn set_leave_days<'e, E>(executor: E, user_id: i64, year: i32, days: i64) -> AppResult<()>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query(
        "INSERT INTO user_annual_leave(user_id, year, days) VALUES ($1,$2,$3) \
         ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days",
    )
    .bind(user_id)
    .bind(year)
    .bind(days)
    .execute(executor)
    .await?;
    Ok(())
}

// HTTP: GET /users/{id}/leave-days — returns current + next year rows
pub async fn get_leave_days_handler(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<Vec<AnnualLeaveRow>>> {
    assert_can_access_user(&app_state, &requester, user_id).await?;
    let current_year = crate::settings::app_current_year(&app_state.pool).await;
    let this = get_leave_days(&app_state.pool, user_id, current_year).await?;
    let next = get_leave_days(&app_state.pool, user_id, current_year + 1).await?;
    Ok(Json(vec![
        AnnualLeaveRow {
            user_id,
            year: current_year,
            days: this,
        },
        AnnualLeaveRow {
            user_id,
            year: current_year + 1,
            days: next,
        },
    ]))
}

#[derive(Deserialize)]
pub struct SetLeaveBody {
    pub year: i32,
    pub days: i64,
}

// HTTP: PUT /users/{id}/leave-days — admin sets a specific year
pub async fn set_leave_days_handler(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<SetLeaveBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let current_year = crate::settings::app_current_year(&app_state.pool).await;
    if body.year < current_year - 1 {
        return Err(AppError::BadRequest(
            "Leave days cannot be set for years before the previous year.".into(),
        ));
    }
    if body.year > current_year + 1 {
        return Err(AppError::BadRequest(
            "Leave days cannot be set more than one year ahead.".into(),
        ));
    }
    if !(0..=366).contains(&body.days) {
        return Err(AppError::BadRequest("Invalid days value.".into()));
    }
    let is_active = app_state
        .db
        .users
        .get_active_flag(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !is_active {
        return Err(AppError::BadRequest("User is inactive.".into()));
    }
    app_state
        .db
        .users
        .set_leave_days(user_id, body.year, body.days)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "users",
        user_id,
        None,
        Some(serde_json::json!({"annual_leave": {"year": body.year, "days": body.days}})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
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
