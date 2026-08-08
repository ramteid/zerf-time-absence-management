use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::reopen_requests::cancel_zombie_reopen_requests;
use crate::services::time_entries::{
    attach_counts_as_work, clear_submission_pending_for_weeks, log_week_status_audit,
    notification_language, notify_week_status_change, repo_entry_to_service, require_tracks_time,
    timesheet_submission_reference_type, week_start, TimeEntry,
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
    // Enforce a maximum date range to prevent unbounded queries (DoS).
    if let (Some(from), Some(to)) = (query.from, query.to) {
        if from > to {
            return Err(AppError::BadRequest("from must not be after to.".into()));
        }
        if (to - from).num_days() > 366 {
            return Err(AppError::BadRequest(
                "Date range must not exceed 366 days.".into(),
            ));
        }
    }
    // Validate status filter against the known set of time entry statuses.
    if let Some(ref s) = query.status {
        if !["draft", "submitted", "approved", "rejected"].contains(&s.as_str()) {
            return Err(AppError::BadRequest("Invalid status filter.".into()));
        }
    }
    // If a specific user_id is requested, verify the target has tracks_time=true.
    // Users with tracks_time=false (pure-admin) have historical entries preserved
    // in the database but they are not accessible via the team endpoint.
    if let Some(target_uid) = query.user_id {
        let target_user = app_state
            .db
            .users
            .find_by_id(target_uid)
            .await?
            .ok_or(AppError::NotFound)?;
        if !target_user.tracks_time || !target_user.active {
            return Err(AppError::Forbidden);
        }
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
    let updated = crate::services::time_entries::update(
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
    .await?;
    Ok(Json(updated))
}

/// Delete a draft time entry. Owners may delete own drafts; admins may delete other's drafts as part of correction (tracks_time check only for own).
pub async fn delete(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let owner_id = app_state.db.time_entries.get_user_id(entry_id).await?;
    if owner_id == requester.id {
        require_tracks_time(&requester)?;
    } else if !requester.is_admin() {
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
/// and notifies all assigned approvers. Users with
/// `allow_submission_without_approval=TRUE` instead go draft → approved
/// directly, silently (see the branch below).
pub async fn submit(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    require_tracks_time(&requester)?;
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "count": 0,
            "auto_approved": requester.allow_submission_without_approval,
        })));
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

    // Users with allow_submission_without_approval=TRUE skip the approval
    // workflow entirely: entries go draft -> approved directly. This is
    // silent by design (mirrors reopen auto-approval) — no one is notified
    // and no emails are sent, to either the requester or the approvers.
    if requester.allow_submission_without_approval {
        let approved = app_state
            .db
            .time_entries
            .submit_batch_auto_approved(requester.id, &body.ids)
            .await?;
        // One audit row per auto-approved week, not per day entry.
        log_week_status_audit(
            &app_state.pool,
            requester.id,
            "auto_approved",
            "draft",
            "approved",
            &approved,
            None,
        )
        .await;
        let user_date_pairs: Vec<(i64, NaiveDate)> =
            approved.iter().map(|e| (e.user_id, e.entry_date)).collect();
        crate::services::reports::requeue_export_for_dates(&app_state.pool, &user_date_pairs).await;
        return Ok(Json(serde_json::json!({
            "ok": true,
            "count": approved.len(),
            "auto_approved": true,
        })));
    }

    // Phase 2: verify approval routing BEFORE any write. Non-admin users
    // without an active assigned approver cannot submit (user-guide), so the
    // check must fail here, while the entries are still editable drafts, not
    // after the transition when they would be stranded in `submitted` with
    // nobody able to review them.
    let approver_ids =
        crate::services::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;

    // Phase 3: atomically submit all draft entries in a single transaction.
    let submitted = app_state
        .db
        .time_entries
        .submit_batch(requester.id, &body.ids)
        .await?;
    // Phase 4: audit logs (best-effort, after commit) — one row per submitted
    // week, mirroring the week the employee actually submitted.
    log_week_status_audit(
        &app_state.pool,
        requester.id,
        "status_changed",
        "draft",
        "submitted",
        &submitted,
        None,
    )
    .await;
    // Phase 5: notify approvers. Submission notifications are week-scoped so
    // deciding one week does not leave a stale unread notification for another.
    let submitted_count = submitted.len();
    let mut submitted_weeks = HashSet::new();
    for entry in &submitted {
        submitted_weeks.insert(week_start(entry.entry_date));
    }
    let mut sorted_submitted_weeks: Vec<NaiveDate> = submitted_weeks.into_iter().collect();
    sorted_submitted_weeks.sort();
    if !sorted_submitted_weeks.is_empty() {
        let language = notification_language(&app_state.pool).await;
        let submitter_name = format!("{} {}", requester.first_name, requester.last_name);

        for &week_monday in &sorted_submitted_weeks {
            let week_list = i18n::format_week_label(&language, week_monday);
            let week_count = i18n::week_count(&language, 1);
            let reference_type = timesheet_submission_reference_type(week_monday);

            for approver_id in &approver_ids {
                let params = vec![
                    ("submitter_name", submitter_name.clone()),
                    ("week_list", week_list.clone()),
                    ("week_count", week_count.clone()),
                ];
                let text = i18n::notification_event_text(&language, "timesheet_submitted", &params);
                let email_body =
                    i18n::notification_email_body(&language, "timesheet_submitted", &params);
                crate::services::notifications::deliver(
                    &app_state,
                    &crate::services::notifications::Outgoing::new(
                        *approver_id,
                        &language,
                        "timesheet_submitted",
                        &text.title,
                        &text.body,
                    )
                    .email_body(&email_body)
                    .reference(&reference_type, Some(requester.id)),
                )
                .await;
            }
        }
    }
    // Phase 6: cancel any "zombie" pending reopen requests for these weeks.
    // After a submission, pending reopen requests for the same week become
    // invisible to approvers (they filter out weeks that have submitted entries)
    // but remain in the DB, blocking the employee from creating a new request.
    // Cancelling them here closes the state machine cleanly. Best-effort only —
    // a failure does not fail the submission.
    cancel_zombie_reopen_requests(&app_state, requester.id, &sorted_submitted_weeks).await;

    Ok(Json(
        serde_json::json!({"ok": true, "count": submitted_count, "auto_approved": false}),
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
    // One audit row per approved week — an approver decides whole weeks, so the
    // trail records the week and embeds a snapshot of every entry in it.
    log_week_status_audit(
        &app_state.pool,
        requester.id,
        "approved",
        "submitted",
        "approved",
        &approved_entries,
        None,
    )
    .await;
    // Send one consolidated notification per affected user.
    if !approved_entries.is_empty() {
        notify_week_status_change(
            &app_state,
            requester.id,
            &approved_entries,
            "timesheet_approved",
            None,
        )
        .await;
        clear_submission_pending_for_entries(&app_state, &approved_entries).await;
        // A past-month PDF can be uploaded once all weeks are submitted, before
        // every entry is approved. Approval changes which minutes reach the
        // PDF Total row, so archived months must be regenerated.
        let user_date_pairs: Vec<(i64, NaiveDate)> = approved_entries
            .iter()
            .map(|e| (e.user_id, e.entry_date))
            .collect();
        crate::services::reports::requeue_export_for_dates(&app_state.pool, &user_date_pairs).await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": approved_entries.len()}),
    ))
}

async fn clear_submission_pending_for_entries(
    app_state: &AppState,
    entries: &[crate::repository::TimeEntry],
) {
    let mut weeks_by_user: std::collections::HashMap<i64, Vec<NaiveDate>> =
        std::collections::HashMap::new();
    for entry in entries {
        weeks_by_user
            .entry(entry.user_id)
            .or_default()
            .push(week_start(entry.entry_date));
    }
    for (user_id, week_mondays) in weeks_by_user {
        clear_submission_pending_for_weeks(app_state, user_id, &week_mondays).await;
    }
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
    // One audit row per rejected week, carrying the reason the approver gave.
    log_week_status_audit(
        &app_state.pool,
        requester.id,
        "rejected",
        "submitted",
        "rejected",
        &rejected_entries,
        Some(&rejection_reason),
    )
    .await;
    // Send one consolidated rejection notification per affected user.
    if !rejected_entries.is_empty() {
        notify_week_status_change(
            &app_state,
            requester.id,
            &rejected_entries,
            "timesheet_rejected",
            Some(&rejection_reason),
        )
        .await;
        clear_submission_pending_for_entries(&app_state, &rejected_entries).await;
        // Re-queue the Nextcloud archive export for any already-uploaded month
        // that was just mutated. Rejection changes which entries count towards
        // the Total, so the archived PDF no longer matches the live ledger.
        let user_date_pairs: Vec<(i64, NaiveDate)> = rejected_entries
            .iter()
            .map(|e| (e.user_id, e.entry_date))
            .collect();
        crate::services::reports::requeue_export_for_dates(&app_state.pool, &user_date_pairs).await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": rejected_entries.len()}),
    ))
}
