use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::roles::{
    can_approve_admin_subjects, can_approve_non_admin_subjects, is_admin_role, is_assistant_role,
    normalize_role, ROLE_ASSISTANT,
};
use crate::services::auth::lock_user_graph;
use crate::services::users::{
    assert_can_access_user, ensure_email_available, ensure_user_name_available, generate_password,
    normalize_optional_user_name, repo_user_to_auth_user, user_unique_conflict,
    validate_approver_ids, ArchiveRequest, LeaveAccountInput, RestoreRequest,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Distinguishes "field omitted" (`None`, leave unchanged) from "field present"
/// (`Some(value)`, including `Some(None)` for explicit `null` — clear back to the
/// `start_date` fallback). Mirrors `deserialize_nullable_string` in `handlers::categories`.
fn deserialize_nullable_date<'de, D>(deserializer: D) -> Result<Option<Option<NaiveDate>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<NaiveDate>::deserialize(deserializer).map(Some)
}

/// Per-user reopen/submission approval policy. Admins receive the full team
/// settings list; non-admin leads receive only users they directly approve.
#[derive(Serialize)]
pub struct TeamSettings {
    pub user_id: i64,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub allow_reopen_without_approval: bool,
    pub allow_submission_without_approval: bool,
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
        .map(
            |(id, email, first_name, last_name, role, allow_reopen, allow_submission)| {
                TeamSettings {
                    user_id: id,
                    email,
                    first_name,
                    last_name,
                    role,
                    allow_reopen_without_approval: allow_reopen,
                    allow_submission_without_approval: allow_submission,
                }
            },
        )
        .collect();
    Ok(Json(settings_list))
}

#[derive(Deserialize)]
pub struct UpdateTeamSettings {
    pub allow_reopen_without_approval: bool,
    pub allow_submission_without_approval: bool,
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
        body.allow_submission_without_approval,
    )
    .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn earliest_start_date(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    // Leads and admins see data across all users → return global minimum.
    // Regular employees only see their own data → return their own start date.
    let date: Option<NaiveDate> = if requester.is_lead() {
        app_state.db.users.earliest_active_start_date().await?
    } else {
        Some(requester.start_date)
    };
    Ok(Json(serde_json::json!({ "earliest_start_date": date })))
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if requester.is_admin() {
        let repo_users = app_state.db.users.find_all_ordered().await?;
        // Fetch all approver relationships in one query to avoid N+1 per user.
        let approver_map = app_state.db.users.get_all_approver_ids().await?;
        let user_list: Vec<serde_json::Value> = repo_users
            .into_iter()
            .map(|u| {
                let approver_ids = approver_map.get(&u.id).cloned().unwrap_or_default();
                let mut v = serde_json::to_value(repo_user_to_auth_user(u))
                    .unwrap_or(serde_json::Value::Null);
                if let serde_json::Value::Object(ref mut map) = v {
                    map.insert("approver_ids".to_string(), serde_json::json!(approver_ids));
                }
                v
            })
            .collect();
        Ok(Json(serde_json::json!(user_list)))
    } else {
        let repo_users = app_state.db.users.find_for_approver(requester.id).await?;
        let user_list: Vec<User> = repo_users.into_iter().map(repo_user_to_auth_user).collect();
        Ok(Json(serde_json::json!(user_list)))
    }
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
        "hire_date": user.hire_date,
        "active": user.active,
        "must_change_password": user.must_change_password,
        "created_at": user.created_at,
        "allow_reopen_without_approval": user.allow_reopen_without_approval,
        "allow_submission_without_approval": user.allow_submission_without_approval,
        "dark_mode": user.dark_mode,
        "tracks_time": user.tracks_time,
        "receives_error_notifications": user.receives_error_notifications,
        "approver_ids": approver_ids,
    });
    Ok(Json(user_json))
}

/// User-specific values for one leave-account category. The same shape is
/// accepted by the regular-user and scoped assistant-management endpoints.
#[derive(Clone, Deserialize)]
pub struct LeaveAccountRequest {
    pub category_id: i64,
    pub base_days: i64,
    pub current_year_days: i64,
    pub next_year_days: i64,
}

impl From<LeaveAccountRequest> for LeaveAccountInput {
    fn from(value: LeaveAccountRequest) -> Self {
        Self {
            category_id: value.category_id,
            base_days: value.base_days,
            current_year_days: value.current_year_days,
            next_year_days: value.next_year_days,
        }
    }
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
    /// Optional values for individual leave accounts. Omitted accounts are
    /// initialized with their category default (or zero for assistants).
    #[serde(default)]
    pub leave_accounts: Option<Vec<LeaveAccountRequest>>,
    pub start_date: NaiveDate,
    /// Optional employment start date used to anchor annual-leave proration
    /// instead of `start_date`. Useful when onboarding an employee who already
    /// worked the full year before adopting Zerf mid-year.
    #[serde(default)]
    pub hire_date: Option<NaiveDate>,
    /// Flextime hours the employee already carried when the account is opened,
    /// in signed minutes. Booked once as an `opening_balance` adjustment dated
    /// on `start_date` — it is not a user setting and cannot be edited later;
    /// see `services::flextime_adjustments`. Ignored for assistants (no
    /// flextime account) and for users created with `tracks_time = false`.
    #[serde(default)]
    pub flextime_opening_balance_min: Option<i64>,
    pub password: Option<String>,
    /// Mandatory for non-admin users: list of team leads/admins who can approve this user's submissions.
    #[serde(default)]
    pub approver_ids: Vec<i64>,
    /// For admin users only: when FALSE the user is in pure-admin mode with no
    /// time or absence tracking. Defaults to TRUE (normal tracking enabled).
    #[serde(default = "default_tracks_time")]
    pub tracks_time: bool,
    /// Time categories enabled for this employee. Omitted/null means "all
    /// existing categories" (the previous default behavior); an explicit
    /// list (including an empty one) is used as-is.
    #[serde(default)]
    pub category_ids: Option<Vec<i64>>,
    /// Absence categories enabled for this employee. Same omitted/null
    /// semantics as `category_ids`.
    #[serde(default)]
    pub absence_category_ids: Option<Vec<i64>>,
    /// Admin-only: opt in to technical error notifications. Ignored (forced
    /// FALSE) for non-admin roles.
    #[serde(default)]
    pub receives_error_notifications: bool,
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
        leave_accounts: body
            .leave_accounts
            .map(|accounts| accounts.into_iter().map(Into::into).collect()),
        start_date: body.start_date,
        hire_date: body.hire_date,
        flextime_opening_balance_min: body.flextime_opening_balance_min,
        password: body.password,
        approver_ids: body.approver_ids,
        tracks_time: body.tracks_time,
        category_ids: body.category_ids,
        absence_category_ids: body.absence_category_ids,
        receives_error_notifications: body.receives_error_notifications,
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
    /// Omitted means leave accounts unchanged; supplied values replace the
    /// affected base/current/next-year values atomically.
    #[serde(default, deserialize_with = "deserialize_optional_leave_accounts")]
    pub leave_accounts: Option<Vec<LeaveAccountRequest>>,
    pub start_date: Option<NaiveDate>,
    /// Triple state via double-Option: omitted = leave unchanged, `null` =
    /// clear back to the `start_date` fallback, value = set explicitly.
    #[serde(default, deserialize_with = "deserialize_nullable_date")]
    pub hire_date: Option<Option<NaiveDate>>,
    /// List of approvers (team leads/admins) for this user.
    /// If provided (even as empty list), replaces all existing approvers.
    #[serde(default, deserialize_with = "deserialize_optional_vec")]
    pub approver_ids: Option<Vec<i64>>,
    pub allow_reopen_without_approval: Option<bool>,
    pub allow_submission_without_approval: Option<bool>,
    // Deliberately no flextime balance field: the carry-in balance is a dated
    // ledger booking, not a profile setting. Editing it here used to rewrite
    // the employee's whole flextime history at once (see migration 043).
    // Later changes go through POST /users/{id}/flextime-adjustments.
    /// For admin users only: when FALSE the user is in pure-admin mode with no
    /// time or absence tracking. Existing time and absence data is retained but
    /// excluded from all views and calculations.
    pub tracks_time: Option<bool>,
    /// Admin-only: opt in to technical error notifications. Omitted = leave
    /// unchanged. Forced FALSE by the service when the effective role is not admin.
    pub receives_error_notifications: Option<bool>,
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

fn deserialize_optional_leave_accounts<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<LeaveAccountRequest>>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    Option::<Vec<LeaveAccountRequest>>::deserialize(deserializer)
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
    let normalized_role = body
        .role
        .as_ref()
        .map(|role_value| normalize_role(role_value));
    if let Some(role_value) = &normalized_role {
        if !["employee", "team_lead", "admin", ROLE_ASSISTANT].contains(&role_value.as_str()) {
            return Err(AppError::BadRequest("Invalid role".into()));
        }
    }
    // Anti-lockout: an admin cannot demote themselves out of admin;
    // otherwise the only path back is fresh DB bootstrap.
    if user_id == requester.id {
        if let Some(role_value) = &body.role {
            if !is_admin_role(role_value) {
                return Err(AppError::BadRequest(
                    "You cannot remove your own admin role.".into(),
                ));
            }
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
    let previous_user: User =
        crate::services::users::fetch_for_update(&mut transaction, user_id).await?;
    let previous_audit_snapshot =
        crate::services::users::user_audit_snapshot(&app_state, &previous_user)
            .await
            .or_else(|| serde_json::to_value(&previous_user).ok());
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
        && normalized_role
            .as_deref()
            .is_some_and(|role_value| role_value != "admin");
    // Pre-validate the post-update invariant (non-admin → has approver).
    let new_role =
        normalized_role.unwrap_or_else(|| previous_user.role.trim().to_ascii_lowercase());
    let effective_weekly_hours = body.weekly_hours.unwrap_or(previous_user.weekly_hours);
    if is_assistant_role(&new_role) {
        tracing::warn!(
            target: "zerf::assistant_role",
            user_id,
            previous_role = %previous_user.role,
            new_role = %new_role,
            effective_weekly_hours,
            "validating assistant invariants during user update"
        );
        if effective_weekly_hours != 0.0 {
            return Err(AppError::BadRequest(
                "Assistants must have weekly_hours set to 0.".into(),
            ));
        }
        // Any flextime adjustments the user accumulated in a previous role are
        // left in place, not deleted: assistants have no flextime account, so
        // every balance path already ignores them, and keeping the rows means
        // a change back to a flextime-bearing role restores the exact balance
        // instead of silently starting from zero.
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

    if !can_approve_admin_subjects(&new_role, previous_user.active) {
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
    if !can_approve_non_admin_subjects(&new_role, previous_user.active) {
        // Archived dependents still carry a `user_approvers` row (kept so a
        // future restore can show/replace it), but they impose no real
        // constraint on this role change — they cannot log in, don't appear
        // in approver pickers, and restore always requires fresh approver_ids
        // anyway. Only active dependents should block the change.
        let non_admin_direct_reports_count =
            crate::services::users::count_active_direct_reports_tx(&mut transaction, user_id)
                .await?;
        if non_admin_direct_reports_count > 0 {
            return Err(AppError::BadRequest(format!(
                "Cannot change this user to a non-approver: {} user(s) still have them as their approver. Reassign them first.",
                non_admin_direct_reports_count
            )));
        }
    }
    // Last-admin protection: checked while the user graph lock is held.
    if removing_admin_rights && previous_user.active {
        let active_admins =
            crate::services::users::count_active_admins_tx(&mut transaction).await?;
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
    // tracks_time=false, silently restore tracking so the new non-admin role
    // has full time-tracking access.
    let effective_tracks_time: Option<bool> =
        if !is_admin_role(&new_role) && !previous_user.tracks_time {
            Some(true)
        } else {
            body.tracks_time
        };
    // When disabling time tracking for an admin who previously had it enabled,
    // existing time and absence data is kept immutably in the database.
    // All queries that build team views or reports filter by tracks_time=TRUE,
    // so the retained rows are silently excluded without any deletions.
    // Any items still sitting in an approval queue (submitted entries, pending
    // absences/reopen requests) are closed out atomically so they don't
    // reappear in queues if tracking is ever re-enabled.
    let disabling_time_tracking = effective_tracks_time == Some(false) && previous_user.tracks_time;
    let submitted_weeks_to_clear = if disabling_time_tracking {
        crate::services::users::close_pending_for_user_tx(&mut transaction, user_id, requester.id)
            .await?
    } else {
        Vec::new()
    };
    // When (re-)enabling time tracking for an admin who currently has it
    // disabled, reset the start_date to today unless the caller is explicitly
    // setting a different start_date. Without this, the admin's old start_date
    // (e.g. years in the past from when the account was first created) would
    // produce a huge negative flextime balance the moment tracking is turned
    // back on.
    let enabling_time_tracking = effective_tracks_time == Some(true) && !previous_user.tracks_time;
    let effective_start_date = if enabling_time_tracking && body.start_date.is_none() {
        Some(crate::services::settings::app_today(&app_state.pool).await)
    } else {
        body.start_date
    };
    let start_date_change_to_requeue =
        effective_start_date.filter(|new_start_date| *new_start_date != previous_user.start_date);
    // Use the normalized role for storage so SQL queries with direct string
    // comparisons (e.g. role = 'admin') work reliably.
    let role_to_store: Option<String> = if body.role.is_some() {
        Some(new_role.clone())
    } else {
        None
    };
    // The effective role after this update (for the admin-only error-notification
    // flag below), captured before `role_to_store` is moved into the update call.
    let effective_role = role_to_store
        .clone()
        .unwrap_or_else(|| previous_user.role.clone());
    crate::services::users::update_basic_tx(
        &mut transaction,
        user_id,
        normalized_email,
        first_name,
        last_name,
        role_to_store,
        body.weekly_hours,
        effective_workdays_update,
        effective_start_date,
        body.hire_date,
        body.allow_reopen_without_approval,
        body.allow_submission_without_approval,
        effective_tracks_time,
    )
    .await
    .map_err(|e| {
        tracing::warn!(target:"zerf::users", "update user failed: {e}");
        user_unique_conflict(&e)
            .unwrap_or_else(|| AppError::Conflict("Could not update user.".into()))
    })?;
    crate::services::users::seed_leave_accounts_for_user_tx(
        &mut transaction,
        user_id,
        &effective_role,
    )
    .await?;
    if let Some(leave_accounts) = body.leave_accounts {
        let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
        let leave_account_inputs: Vec<LeaveAccountInput> =
            leave_accounts.into_iter().map(Into::into).collect();
        crate::services::users::apply_leave_account_values_tx(
            &mut transaction,
            user_id,
            current_year,
            &leave_account_inputs,
        )
        .await?;
    }
    // Handle approver_ids update if provided
    if let Some(new_approver_ids) = &body.approver_ids {
        crate::services::users::set_approvers_tx(&mut transaction, user_id, new_approver_ids)
            .await?;
    }
    // Technical error notifications are admin-only. Force the flag off whenever
    // the effective role is not admin (covers demotions); otherwise apply the
    // submitted value if the client sent one.
    if !crate::roles::is_admin_role(&effective_role) {
        crate::services::users::set_receives_error_notifications_tx(
            &mut transaction,
            user_id,
            false,
        )
        .await?;
    } else if let Some(enabled) = body.receives_error_notifications {
        crate::services::users::set_receives_error_notifications_tx(
            &mut transaction,
            user_id,
            enabled,
        )
        .await?;
    }
    // Kill sessions on role change so cached role cannot be (ab)used.
    let previous_role_normalized = normalize_role(&previous_user.role);
    let role_changed = body
        .role
        .as_ref()
        .map(|role_value| normalize_role(role_value) != previous_role_normalized)
        .unwrap_or(false);
    if role_changed {
        let _ =
            crate::services::users::delete_sessions_for_user_tx(&mut transaction, user_id).await;
    }
    transaction.commit().await?;
    if let Some(new_start_date) = start_date_change_to_requeue {
        crate::services::reports::requeue_export_for_start_date_change(
            &app_state.pool,
            user_id,
            previous_user.start_date,
            new_start_date,
        )
        .await;
    }
    crate::services::time_entries::clear_submission_pending_for_weeks(
        &app_state,
        user_id,
        &submitted_weeks_to_clear,
    )
    .await;
    let updated_user = app_state
        .db
        .users
        .find_by_id(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let updated_auth_user = repo_user_to_auth_user(updated_user);
    let updated_audit_snapshot =
        crate::services::users::user_audit_snapshot(&app_state, &updated_auth_user)
            .await
            .or_else(|| serde_json::to_value(&updated_auth_user).ok());
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "users",
        user_id,
        previous_audit_snapshot,
        updated_audit_snapshot,
    )
    .await;
    Ok(Json(updated_auth_user))
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
    let target_user: User =
        crate::services::users::fetch_for_update(&mut transaction, user_id).await?;
    if target_user.active && is_admin_role(&target_user.role) {
        let active_admins =
            crate::services::users::count_active_admins_tx(&mut transaction).await?;
        if active_admins <= 1 {
            return Err(AppError::BadRequest(
                "Cannot delete the last active admin.".into(),
            ));
        }
    }
    // Run inside the transaction (under the user-graph lock) to avoid TOCTOU.
    let direct_reports_count =
        crate::services::users::count_active_direct_reports_tx(&mut transaction, user_id).await?;
    if direct_reports_count > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot delete: {} active user(s) still have this person as their approver. Reassign them first.",
            direct_reports_count
        )));
    }
    // Guard: users with historical time/absence data must be archived, not hard-deleted.
    // This preserves audit trails, reports, and absence history.
    let has_data = crate::services::users::has_time_data_tx(&mut transaction, user_id).await?;
    if has_data {
        return Err(AppError::BadRequest(
            "User has historical data. Use archive instead.".into(),
        ));
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
    let new_password_hash =
        crate::services::auth::hash_password_async(temporary_password.clone()).await?;
    let mut transaction = app_state.db.users.begin().await?;
    let target_user = crate::services::users::fetch_for_update(&mut transaction, target_id).await?;
    if !target_user.active {
        return Err(AppError::BadRequest("User is inactive.".into()));
    }
    crate::services::users::update_password_tx(
        &mut transaction,
        target_id,
        &new_password_hash,
        true,
    )
    .await?;
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
    // Send password-reset notification email (best-effort, fire-and-forget).
    let language = crate::i18n::load_ui_language(&app_state.pool)
        .await
        .unwrap_or_default();
    let login_line = i18n::email_login_line(&language, app_state.cfg.public_url.as_deref());
    let org_name_raw =
        crate::services::settings::load_setting(&app_state.pool, "organization_name", "")
            .await
            .unwrap_or_default();
    let org_name = i18n::email_organization_name(&language, &org_name_raw);
    let text = i18n::notification_text(
        &language,
        "admin_password_reset_subject",
        "admin_password_reset_body",
        &[
            ("org_name", org_name),
            ("first_name", target_user.first_name.clone()),
            ("last_name", target_user.last_name.clone()),
            ("email", target_user.email.clone()),
            ("password", temporary_password.clone()),
            ("login_line", login_line),
        ],
    );
    // Email-only transactional mail (temporary password); no in-app row and no
    // footer — the body is already the complete reset message.
    crate::services::notifications::deliver(
        &app_state,
        &crate::services::notifications::Outgoing::new(
            target_id,
            &language,
            "admin_password_reset",
            &text.title,
            &text.body,
        )
        .channels(crate::services::notifications::Channels::EmailOnly)
        .append_email_footer(false),
    )
    .await;
    Ok(Json(
        serde_json::json!({"temporary_password": temporary_password}),
    ))
}

/// GET /leave-accounts - account definitions available to the requester.
pub async fn list_leave_accounts(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<crate::services::users::LeaveAccountDefinition>>> {
    Ok(Json(
        crate::services::users::leave_account_definitions(&app_state, &requester).await?,
    ))
}

/// GET /users/{id}/leave-accounts - user-specific base and current/next-year
/// values for every category-specific leave account.
pub async fn get_user_leave_accounts(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<Vec<crate::services::users::UserLeaveAccountDetails>>> {
    Ok(Json(
        crate::services::users::leave_accounts_for_user(&app_state, &requester, user_id).await?,
    ))
}

// ---------------------------------------------------------------------------
// Archive / Restore / List Archived
// ---------------------------------------------------------------------------

/// Request body for POST /users/{id}/archive.
#[derive(Deserialize)]
pub struct ArchiveBody {
    /// Map of user_id -> new_approver_id for every active user currently
    /// approved by the target. Required only when the target is an approver
    /// for active users. May be omitted or empty otherwise.
    #[serde(default)]
    pub approver_replacements: HashMap<String, i64>,
}

/// POST /users/{id}/archive — admin only.
pub async fn archive_user(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<ArchiveBody>,
) -> AppResult<Json<serde_json::Value>> {
    // Convert string keys from JSON to i64 (JSON object keys are always strings).
    let replacements: HashMap<i64, i64> = body
        .approver_replacements
        .into_iter()
        .map(|(k, v)| {
            k.parse::<i64>()
                .map_err(|_| {
                    AppError::BadRequest("Invalid user id key in approver_replacements.".into())
                })
                .map(|id| (id, v))
        })
        .collect::<AppResult<HashMap<i64, i64>>>()?;

    crate::services::users::archive(
        &app_state,
        &requester,
        user_id,
        ArchiveRequest {
            approver_replacements: replacements,
        },
    )
    .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Request body for POST /users/{id}/restore.
#[derive(Deserialize)]
pub struct RestoreBody {
    /// Optional new start date. When provided, resets the user's start date
    /// to avoid negative flextime accumulation from the archived period.
    pub start_date: Option<NaiveDate>,
    /// Approver IDs for the restored user. Required for non-admin users.
    #[serde(default)]
    pub approver_ids: Vec<i64>,
}

/// POST /users/{id}/restore — admin only.
pub async fn restore_user(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<RestoreBody>,
) -> AppResult<Json<User>> {
    let updated = crate::services::users::restore(
        &app_state,
        &requester,
        user_id,
        RestoreRequest {
            new_start_date: body.start_date,
            approver_ids: body.approver_ids,
        },
    )
    .await?;
    Ok(Json(updated))
}

/// GET /users/archived — admin only. Returns archived users ordered by archived_at DESC.
pub async fn list_archived(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<crate::repository::users::ArchivedUser>>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let archived = app_state.db.users.find_archived_ordered().await?;
    Ok(Json(archived))
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
