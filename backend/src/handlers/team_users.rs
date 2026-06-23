//! Scoped self-service user management for non-admin team leads: lets a team
//! lead create and manage "assistant" (Aushilfe) users that are assigned to
//! them as approver, when an admin has enabled
//! `allow_team_lead_manage_assistants`. Every handler delegates authorization
//! to `services::users::assert_team_lead_assistant_list_access` /
//! `assert_team_lead_can_manage_assistant` — admins use the regular
//! `/users*` endpoints instead.
//!
//! Team leads may archive and restore an assistant via the dedicated
//! `/team-users/{id}/archive` and `/team-users/{id}/restore` endpoints.
//! Delete capability is intentionally absent — only an admin can hard-delete.

use crate::audit;
use crate::error::{AppError, AppResult};
use crate::handlers::users::CreateResponse;
use crate::middleware::auth::User;
use crate::roles::{is_assistant_role, ROLE_ASSISTANT};
use crate::services::users::{
    assert_team_lead_assistant_list_access, assert_team_lead_can_manage_assistant,
    ensure_email_available, ensure_user_name_available, normalize_optional_user_name,
    repo_user_to_auth_user, set_leave_days_tx, update_basic_tx, user_unique_conflict,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Row returned by `GET /team-users`. Non-manageable colleagues (anyone who
/// isn't an assistant, including the requester's own row) carry only
/// `id`/`first_name`/`last_name` — no other field is ever serialized for
/// them, so confidentiality doesn't depend on the frontend hiding anything.
/// The list includes archived assistants so a lead can see and restore them.
#[derive(Serialize)]
pub struct TeamUserRow {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub can_manage: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<TeamUserRow>>> {
    assert_team_lead_assistant_list_access(&app_state, &requester).await?;
    let repo_users = app_state
        .db
        .users
        .find_for_approver_including_inactive(requester.id)
        .await?;
    let rows = repo_users
        .into_iter()
        .map(|u| {
            let can_manage = is_assistant_role(&u.role);
            TeamUserRow {
                id: u.id,
                first_name: u.first_name,
                last_name: u.last_name,
                can_manage,
                email: can_manage.then_some(u.email),
                role: can_manage.then_some(u.role),
                archived_at: can_manage.then(|| u.archived_at).flatten(),
            }
        })
        .collect();
    Ok(Json(rows))
}

pub async fn get_one(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let target = assert_team_lead_can_manage_assistant(&app_state, &requester, user_id).await?;
    Ok(Json(serde_json::json!({
        "id": target.id,
        "email": target.email,
        "first_name": target.first_name,
        "last_name": target.last_name,
        "role": target.role,
        "start_date": target.start_date,
        "hire_date": target.hire_date,
        "archived_at": target.archived_at,
        "annual_leave_days": target.annual_leave_days,
    })))
}

#[derive(Deserialize)]
pub struct NewTeamAssistant {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub leave_days_current_year: i64,
    pub leave_days_next_year: i64,
    pub annual_leave_days: i64,
    pub start_date: NaiveDate,
    #[serde(default)]
    pub hire_date: Option<NaiveDate>,
    pub password: Option<String>,
    #[serde(default)]
    pub category_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub absence_category_ids: Option<Vec<i64>>,
}

// Note: there is deliberately no `role` or `approver_ids` field above — the
// role is always "assistant" and the approver is always the requester.
// `services::users::create()` enforces both, ignoring any client input.
pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewTeamAssistant>,
) -> AppResult<Json<CreateResponse>> {
    assert_team_lead_assistant_list_access(&app_state, &requester).await?;
    let service_body = crate::services::users::NewUser {
        email: body.email,
        first_name: body.first_name,
        last_name: body.last_name,
        role: ROLE_ASSISTANT.to_string(),
        weekly_hours: 0.0,
        workdays_per_week: None,
        leave_days_current_year: body.leave_days_current_year,
        leave_days_next_year: body.leave_days_next_year,
        annual_leave_days: body.annual_leave_days,
        start_date: body.start_date,
        hire_date: body.hire_date,
        overtime_start_balance_min: None,
        password: body.password,
        approver_ids: vec![requester.id],
        tracks_time: true,
        category_ids: body.category_ids,
        absence_category_ids: body.absence_category_ids,
    };
    let created = crate::services::users::create(&app_state, &requester, service_body).await?;
    Ok(Json(CreateResponse {
        id: created.id,
        user: created.user,
        temporary_password: created.temporary_password,
    }))
}

#[derive(Deserialize)]
pub struct UpdateTeamAssistant {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub leave_days_current_year: Option<i64>,
    pub leave_days_next_year: Option<i64>,
    pub annual_leave_days: Option<i64>,
    pub start_date: Option<NaiveDate>,
    /// Absent/null = leave unchanged, value = set explicitly. Unlike the admin
    /// endpoint there is no separate "clear back to start_date" sentinel here
    /// — out of scope for the deliberately small assistant-management surface.
    #[serde(default)]
    pub hire_date: Option<NaiveDate>,
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<UpdateTeamAssistant>,
) -> AppResult<Json<User>> {
    let previous_user =
        assert_team_lead_can_manage_assistant(&app_state, &requester, user_id).await?;

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
    if let Some(d) = body.annual_leave_days {
        if !(0..=366).contains(&d) {
            return Err(AppError::BadRequest("Invalid annual_leave_days.".into()));
        }
    }
    let normalized_email = body.email.as_ref().map(|email| email.trim().to_lowercase());
    if let Some(email) = &normalized_email {
        if email.is_empty() || email.len() > 254 || !email.contains('@') {
            return Err(AppError::BadRequest("Invalid email.".into()));
        }
    }
    let first_name = normalize_optional_user_name(body.first_name.as_ref())?;
    let last_name = normalize_optional_user_name(body.last_name.as_ref())?;

    let mut transaction = app_state.db.users.begin().await?;
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
        ensure_user_name_available(&app_state, &updated_first_name, &updated_last_name, Some(user_id))
            .await?;
    }
    update_basic_tx(
        &mut transaction,
        user_id,
        normalized_email,
        first_name,
        last_name,
        None, // role: locked to "assistant", never changed here
        None, // weekly_hours: locked to 0 for assistants
        None, // workdays_per_week: locked (no fixed days for assistants)
        body.start_date,
        body.hire_date.map(Some),
        None,
        None,
        None, // overtime_start_balance_min: locked to 0 for assistants
        None, // tracks_time: locked to true for assistants
        body.annual_leave_days,
    )
    .await
    .map_err(|e| {
        tracing::warn!(target: "zerf::team_users", "update assistant failed: {e}");
        user_unique_conflict(&e).unwrap_or_else(|| AppError::Conflict("Could not update user.".into()))
    })?;
    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
    if let Some(d) = body.leave_days_current_year {
        set_leave_days_tx(&mut transaction, user_id, current_year, d).await?;
    }
    if let Some(d) = body.leave_days_next_year {
        set_leave_days_tx(&mut transaction, user_id, current_year + 1, d).await?;
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

/// POST /team-users/{id}/archive — team lead only.
/// Archives the assistant if the requester is their approver and the
/// `allow_team_lead_manage_assistants` setting is enabled.
pub async fn archive_assistant(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    crate::services::users::archive_assistant(&app_state, &requester, user_id).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Request body for POST /team-users/{id}/restore.
#[derive(serde::Deserialize)]
pub struct RestoreAssistantBody {
    /// Optional new start date to avoid negative flextime accumulation.
    pub start_date: Option<chrono::NaiveDate>,
}

/// POST /team-users/{id}/restore — team lead only.
/// Restores the archived assistant if the requester is their approver.
pub async fn restore_assistant(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<RestoreAssistantBody>,
) -> AppResult<Json<User>> {
    let updated = crate::services::users::restore_assistant(
        &app_state,
        &requester,
        user_id,
        body.start_date,
    )
    .await?;
    Ok(Json(updated))
}
