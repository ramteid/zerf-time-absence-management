use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::time_entries::{
    attach_counts_as_work, notification_language, notify_week_status_change, repo_entry_to_service,
    require_tracks_time, week_start, TimeEntry,
};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct RangeQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub user_id: Option<i64>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct NewTimeEntry {
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct IdsBody {
    pub ids: Vec<i64>,
}

#[derive(Deserialize)]
pub struct BatchRejectBody {
    pub ids: Vec<i64>,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// CRUD handlers
// ---------------------------------------------------------------------------

/// List time entries for the requesting user, optionally filtered by date range.
pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    require_tracks_time(&requester)?;
    let entries = app_state
        .db
        .time_entries
        .list_for_user(requester.id, query.from, query.to)
        .await?;
    let mut mapped: Vec<TimeEntry> = entries.into_iter().map(repo_entry_to_service).collect();
    attach_counts_as_work(&app_state, &mut mapped).await?;
    Ok(Json(mapped))
}

/// List time entries across all users (leads/admins only).
/// Admins see everything; team leads see only their direct reports.
pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let entries = app_state
        .db
        .time_entries
        .list_all(
            requester.is_admin(),
            requester.id,
            query.from,
            query.to,
            query.user_id,
            query.status,
        )
        .await?;
    let mut mapped: Vec<TimeEntry> = entries.into_iter().map(repo_entry_to_service).collect();
    attach_counts_as_work(&app_state, &mut mapped).await?;
    Ok(Json(mapped))
}

/// Create a new draft time entry for the requesting user.
pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewTimeEntry>,
) -> AppResult<Json<TimeEntry>> {
    require_tracks_time(&requester)?;
    Ok(Json(
        crate::services::time_entries::create(
            &app_state,
            &requester,
            body.entry_date,
            body.start_time,
            body.end_time,
            body.category_id,
            body.comment,
        )
        .await?,
    ))
}

/// Update a draft time entry. Only the owner (or an admin) may edit.
/// Admins with `tracks_time=false` are in pure-admin mode and cannot manage
/// their own time data, but they CAN edit other users' entries (admin
/// correction path). The guard is applied only when the requester owns the
/// entry being edited.
pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
    Json(body): Json<NewTimeEntry>,
) -> AppResult<Json<TimeEntry>> {
    Ok(Json(
        crate::services::time_entries::update(
            &app_state,
            &requester,
            entry_id,
            crate::services::time_entries::TimeEntryInput {
                entry_date: body.entry_date,
                start_time: body.start_time,
                end_time: body.end_time,
                category_id: body.category_id,
                comment: body.comment,
            },
        )
        .await?,
    ))
}

/// Delete a draft time entry. Only the owner may delete their own entries.
pub async fn delete(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    require_tracks_time(&requester)?;
    let owner_id = app_state.db.time_entries.get_user_id(entry_id).await?;
    if owner_id != requester.id {
        return Err(AppError::Forbidden);
    }
    let deleted = app_state.db.time_entries.delete(entry_id).await?;
    let time_entry = repo_entry_to_service(deleted);
    audit::log(
        &app_state.pool,
        requester.id,
        "deleted",
        "time_entries",
        entry_id,
        serde_json::to_value(&time_entry).ok(),
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------------------------------------------------------------------------
// Week-level submission, approval, and rejection
// ---------------------------------------------------------------------------

/// Submit draft entries for approval. The employee selects entries by ID;
/// the backend transitions them from draft → submitted in a single transaction
/// and notifies all assigned approvers.
pub async fn submit(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    require_tracks_time(&requester)?;
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    if body.ids.len() > 500 {
        return Err(AppError::BadRequest("Too many entries (max 500).".into()));
    }
    // Phase 1: validate ownership for ALL entries before any writes, so a
    // mixed-ownership batch never partially submits.
    if !app_state
        .db
        .time_entries
        .all_entries_owned_by_user(&body.ids, requester.id)
        .await?
    {
        return Err(AppError::Forbidden);
    }
    // Phase 2: atomically submit all draft entries in a single transaction.
    let submitted_ids = app_state
        .db
        .time_entries
        .submit_batch(requester.id, &body.ids)
        .await?;
    // Phase 3: audit logs (best-effort, after commit).
    for entry_id in &submitted_ids {
        audit::log(
            &app_state.pool,
            requester.id,
            "status_changed",
            "time_entries",
            *entry_id,
            Some(serde_json::json!({"status": "draft"})),
            Some(serde_json::json!({"status": "submitted"})),
        )
        .await;
    }
    // Phase 4: notify approvers with a consolidated week count.
    let submitted_count = submitted_ids.len();
    let mut submitted_weeks = HashSet::new();
    for entry_date in app_state
        .db
        .time_entries
        .entry_dates_for_ids(&submitted_ids)
        .await?
    {
        submitted_weeks.insert(week_start(entry_date));
    }
    if !submitted_weeks.is_empty() {
        let approver_ids =
            crate::services::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
        let language = notification_language(&app_state.pool).await;
        let mut sorted_weeks: Vec<NaiveDate> = submitted_weeks.into_iter().collect();
        sorted_weeks.sort();
        let week_list = sorted_weeks
            .iter()
            .map(|ws| i18n::format_week_label(&language, *ws))
            .collect::<Vec<_>>()
            .join("\n");
        let week_count = i18n::week_count(&language, sorted_weeks.len() as i64);
        let submitter_name = format!("{} {}", requester.first_name, requester.last_name);

        // Build JSON body for frontend rendering.
        let week_iso_strings: Vec<String> = sorted_weeks
            .iter()
            .map(|ws| ws.format("%Y-%m-%d").to_string())
            .collect();
        let frontend_body = serde_json::json!({
            "submitter_name": submitter_name,
            "weeks": week_iso_strings,
        })
        .to_string();

        for approver_id in approver_ids {
            crate::services::notifications::create_with_frontend_body(
                &app_state,
                &language,
                approver_id,
                "timesheet_submitted",
                "timesheet_submitted_title",
                "timesheet_submitted_body",
                vec![
                    ("submitter_name", submitter_name.clone()),
                    ("week_list", week_list.clone()),
                    ("week_count", week_count.clone()),
                ],
                &frontend_body,
                true,
                Some("time_entries"),
                None,
            )
            .await;
        }
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": submitted_count}),
    ))
}

/// Approve submitted entries in batch (week-level approval).
/// Only leads (team_lead / admin) may approve. Admins can approve any user;
/// team leads can only approve their direct reports. Entries that are not in
/// "submitted" status or not under the reviewer's purview are silently skipped.
pub async fn batch_approve(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    if body.ids.len() > 500 {
        return Err(AppError::BadRequest("Too many entries (max 500).".into()));
    }
    let approved_entries = app_state
        .db
        .time_entries
        .batch_approve(&body.ids, requester.id, requester.is_admin())
        .await?;
    // Audit each entry individually for traceability.
    for entry in &approved_entries {
        audit::log(
            &app_state.pool,
            requester.id,
            "approved",
            "time_entries",
            entry.id,
            serde_json::to_value(entry).ok(),
            Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
        )
        .await;
    }
    // Send one consolidated notification per affected user.
    if !approved_entries.is_empty() {
        notify_week_status_change(
            &app_state,
            requester.id,
            &approved_entries,
            "timesheet_approved",
            "timesheet_approved_title",
            "timesheet_batch_approved_body",
            None,
        )
        .await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": approved_entries.len()}),
    ))
}

/// Reject submitted entries in batch (week-level rejection).
/// Same authorization rules as batch_approve. A rejection reason is required.
pub async fn batch_reject(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<BatchRejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rejection_reason = body.reason.trim().to_string();
    if rejection_reason.is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if rejection_reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    if body.ids.len() > 500 {
        return Err(AppError::BadRequest("Too many entries (max 500).".into()));
    }
    let rejected_entries = app_state
        .db
        .time_entries
        .batch_reject(
            &body.ids,
            requester.id,
            requester.is_admin(),
            &rejection_reason,
        )
        .await?;
    // Audit each rejected entry individually for traceability.
    for entry in &rejected_entries {
        audit::log(
            &app_state.pool,
            requester.id,
            "rejected",
            "time_entries",
            entry.id,
            serde_json::to_value(entry).ok(),
            Some(serde_json::json!({"status": "rejected", "reason": rejection_reason})),
        )
        .await;
    }
    // Send one consolidated rejection notification per affected user.
    if !rejected_entries.is_empty() {
        notify_week_status_change(
            &app_state,
            requester.id,
            &rejected_entries,
            "timesheet_rejected",
            "timesheet_rejected_title",
            "timesheet_batch_rejected_body",
            Some(&rejection_reason),
        )
        .await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": rejected_entries.len()}),
    ))
}
