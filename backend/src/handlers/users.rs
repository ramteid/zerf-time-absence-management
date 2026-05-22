use crate::audit;
use crate::services::auth::lock_user_graph;
use crate::middleware::auth::User;
use crate::error::{AppError, AppResult};
use crate::roles::{
    can_approve_admin_subjects, can_approve_non_admin_subjects, is_admin_role,
    is_assistant_role, normalize_role, ROLE_ASSISTANT,
};
use crate::services::users::{
    assert_can_access_user, ensure_email_available, ensure_user_name_available,
    generate_password, get_leave_days, normalize_optional_user_name,
    repo_user_to_auth_user, user_unique_conflict, validate_approver_ids,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
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
    crate::services::users::team_settings_update(
        &app_state,
        &requester,
        target_id,
        body.allow_reopen_without_approval,
    )
    .await?;
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
        "tracks_time": user.tracks_time,
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
    /// For admin users only: when FALSE the user is in pure-admin mode with no
    /// time or absence tracking. Defaults to TRUE (normal tracking enabled).
    #[serde(default = "default_tracks_time")]
    pub tracks_time: bool,
}

fn default_tracks_time() -> bool {
    true
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
    let service_body = crate::services::users::NewUser {
        email: body.email,
        first_name: body.first_name,
        last_name: body.last_name,
        role: body.role,
        weekly_hours: body.weekly_hours,
        workdays_per_week: body.workdays_per_week,
        leave_days_current_year: body.leave_days_current_year,
        leave_days_next_year: body.leave_days_next_year,
        start_date: body.start_date,
        overtime_start_balance_min: body.overtime_start_balance_min,
        password: body.password,
        approver_ids: body.approver_ids,
        tracks_time: body.tracks_time,
    };
    let created = crate::services::users::create(&app_state, &requester, service_body).await?;
    Ok(Json(CreateResponse {
        id: created.id,
        user: created.user,
        temporary_password: created.temporary_password,
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
    /// For admin users only: when FALSE the user is in pure-admin mode with no
    /// time or absence tracking. Setting to FALSE deletes all existing time and
    /// absence data for the user.
    pub tracks_time: Option<bool>,
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
    let mut transaction = app_state.db.users.begin().await?;
    lock_user_graph(&mut transaction).await?;
    let previous_user: User = crate::services::users::fetch_for_update(&mut transaction, user_id).await?;
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
    // For assistants force workdays_per_week=7 (no fixed days).
    // When switching FROM assistant TO another role, reset to 5 (default) unless the
    // admin explicitly provides a value — otherwise the sentinel 7 would persist via
    // COALESCE and produce wrong daily-target calculations for the new role.
    let effective_workdays_update: Option<i16> = if is_assistant_role(&new_role) {
        Some(7)
    } else if is_assistant_role(&previous_user.role) {
        Some(body.workdays_per_week.unwrap_or(5))
    } else {
        body.workdays_per_week
    };
    let effective_approver_ids = if let Some(approver_ids) = &body.approver_ids {
        approver_ids.clone()
    } else {
        crate::services::users::get_approver_ids_tx(&mut transaction, user_id).await?
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
        let active_admins = crate::services::users::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot remove the last active admin.".into(),
            ));
        }
    }
    // tracks_time=false is only valid for admin users. Reject explicit attempts
    // to set it on a non-admin, and auto-restore it to true when an admin is
    // demoted (the DB CHECK constraint enforces the same invariant as a safety net).
    if let Some(false) = body.tracks_time {
        if !is_admin_role(&new_role) {
            return Err(AppError::BadRequest(
                "tracks_time can only be disabled for admin users.".into(),
            ));
        }
    }
    // When the role changes away from admin and the user currently has
    // tracks_time=false, silently restore tracking. No data to delete since
    // they never had tracking enabled as a non-admin.
    let effective_tracks_time: Option<bool> = if !is_admin_role(&new_role) && !previous_user.tracks_time {
        Some(true)
    } else {
        body.tracks_time
    };
    // When disabling time tracking for an admin who previously had it enabled,
    // delete all their time entries, absences, and reopen requests atomically.
    let disabling_time_tracking = effective_tracks_time == Some(false) && previous_user.tracks_time;
    if disabling_time_tracking {
        crate::services::users::delete_time_data_for_user_tx(&mut transaction, user_id).await?;
    }
    // Use the normalized role for storage so SQL queries with direct string
    // comparisons (e.g. role = 'admin') work reliably.
    let role_to_store: Option<String> = if body.role.is_some() {
        Some(new_role.clone())
    } else {
        None
    };
    crate::services::users::update_basic_tx(
        &mut transaction,
        user_id,
        normalized_email,
        first_name,
        last_name,
        role_to_store,
        body.weekly_hours,
        effective_workdays_update,
        body.start_date,
        body.active,
        body.allow_reopen_without_approval,
        body.overtime_start_balance_min,
        effective_tracks_time,
    )
    .await
        .map_err(|e| {
            tracing::warn!(target:"zerf::users", "update user failed: {e}");
            user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not update user.".into()))
        })?;
    // Update leave days if provided
    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
    if let Some(d) = body.leave_days_current_year {
        crate::services::users::set_leave_days_tx(&mut transaction, user_id, current_year, d).await?;
    }
    if let Some(d) = body.leave_days_next_year {
        crate::services::users::set_leave_days_tx(&mut transaction, user_id, current_year + 1, d).await?;
    }
    // Handle approver_ids update if provided
    if let Some(new_approver_ids) = &body.approver_ids {
        crate::services::users::set_approvers_tx(&mut transaction, user_id, new_approver_ids).await?;
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
        let _ = crate::services::users::delete_sessions_for_user_tx(&mut transaction, user_id).await;
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
    let mut transaction = app_state.db.users.begin().await?;
    lock_user_graph(&mut transaction).await?;
    let previous_user: User = crate::services::users::fetch_for_update(&mut transaction, user_id).await?;
    if previous_user.active && is_admin_role(&previous_user.role) {
        let active_admins = crate::services::users::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot remove the last active admin.".into(),
            ));
        }
    }
    // Block deactivation if this person is an assigned approver for active users.
    // Run inside the transaction (under the user-graph lock) to avoid TOCTOU.
    let direct_reports_count = crate::services::users::count_active_direct_reports_tx(&mut transaction, user_id).await?;
    if direct_reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot deactivate: {} active user(s) still have this person as their approver. Reassign them first.",
            direct_reports_count
        )));
    }
    crate::services::users::deactivate_tx(&mut transaction, user_id).await?;
    crate::services::users::delete_sessions_for_user_tx(&mut transaction, user_id).await?;
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
    let mut transaction = app_state.db.users.begin().await?;
    lock_user_graph(&mut transaction).await?;
    let target_user: User = crate::services::users::fetch_for_update(&mut transaction, user_id).await?;
    if target_user.active && is_admin_role(&target_user.role) {
        let active_admins = crate::services::users::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot delete the last active admin.".into(),
            ));
        }
    }
    // Run inside the transaction (under the user-graph lock) to avoid TOCTOU.
    let direct_reports_count = crate::services::users::count_active_direct_reports_tx(&mut transaction, user_id).await?;
    if direct_reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot delete: {} active user(s) still have this person as their approver. Reassign them first.",
            direct_reports_count
        )));
    }
    crate::services::users::delete_tx(&mut transaction, user_id).await?;
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
    let new_password_hash = crate::services::auth::hash_password_async(temporary_password.clone()).await?;
    let mut transaction = app_state.db.users.begin().await?;
    let target_user = crate::services::users::fetch_for_update(&mut transaction, target_id).await?;
    if !target_user.active {
        return Err(AppError::BadRequest("User is inactive.".into()));
    }
    crate::services::users::update_password_tx(&mut transaction, target_id, &new_password_hash, true).await?;
    // Force re-authentication: kill any existing sessions for this user.
    crate::services::users::delete_sessions_for_user_tx(&mut transaction, target_id).await?;
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
#[derive(serde::Serialize)]
pub struct AnnualLeaveRow {
    pub user_id: i64,
    pub year: i32,
    pub days: i64,
}

// HTTP: GET /users/{id}/leave-days — returns current + next year rows
pub async fn get_leave_days_handler(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<Vec<AnnualLeaveRow>>> {
    assert_can_access_user(&app_state, &requester, user_id).await?;
    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
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
    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_optional_vec_distinguishes_absent_null_and_values() {
        let absent: UpdateUser = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(absent.approver_ids, None);

        let null_value: UpdateUser =
            serde_json::from_value(serde_json::json!({"approver_ids": null})).unwrap();
        assert_eq!(null_value.approver_ids, None);

        let explicit_list: UpdateUser =
            serde_json::from_value(serde_json::json!({"approver_ids": [1, 2]})).unwrap();
        assert_eq!(explicit_list.approver_ids, Some(vec![1, 2]));

        let explicit_empty: UpdateUser =
            serde_json::from_value(serde_json::json!({"approver_ids": []})).unwrap();
        assert_eq!(explicit_empty.approver_ids, Some(Vec::new()));
    }

    #[test]
    fn default_tracks_time_is_enabled() {
        assert!(default_tracks_time());
    }
}
