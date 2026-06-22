//! Weekly reopen-request workflow HTTP handlers.

use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::notifications;
use crate::services::reopen_requests::{
    approver_ids_to_notify, assert_monday, audit_reopened_entries, notification_language,
    notify_assigned_approvers_if_admin_acted, repo_rr_to_service, ReopenRequest,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct NewReopen {
    pub week_start: chrono::NaiveDate,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewReopen>,
) -> AppResult<Json<serde_json::Value>> {
    // Pure-admin users have no time entries and therefore no weeks to reopen.
    if !requester.tracks_time {
        return Err(AppError::Forbidden);
    }
    assert_monday(body.week_start)?;
    let week_end = body.week_start + chrono::Duration::days(6);

    // Empty-week / nothing-to-reopen guard: only weeks with at least one
    // submitted, approved, or rejected entry are eligible.
    let reopenable_entry_count = app_state
        .db
        .reopen_requests
        .count_non_draft_entries(requester.id, body.week_start, week_end)
        .await?;
    if reopenable_entry_count == 0 {
        return Err(AppError::BadRequest(
            "Cannot request edit - this week has no submitted, approved, or rejected entries."
                .into(),
        ));
    }

    // Reject duplicate pending request (DB also has a unique partial index).
    let existing_pending_id = app_state
        .db
        .reopen_requests
        .find_pending_request_id(requester.id, body.week_start)
        .await?;
    if let Some(existing_request_id) = existing_pending_id {
        return Err(AppError::Conflict(format!(
            "A pending edit request already exists (id {existing_request_id})."
        )));
    }

    // Validate reason (required, max 2000 chars).
    let request_reason = body
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("Reason required.".into()))?;
    if request_reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }

    // Determine flow:
    //   * User has `allow_reopen_without_approval=TRUE` → auto_approved
    //   * Otherwise → pending, notify all approvers
    let should_auto_approve = requester.allow_reopen_without_approval;

    // Non-admin users who need a reviewer must have at least one active approver.
    // Auto-approve requests resolve immediately without any reviewer action, so
    // skip this check when the request will be auto-approved.
    if !should_auto_approve {
        crate::services::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
    }

    let initial_status = if should_auto_approve {
        "auto_approved"
    } else {
        "pending"
    };

    let (new_request_id, reopened_entries): (i64, Option<Vec<(i64, String)>>) =
        if should_auto_approve {
            let (new_id, affected) = app_state
                .db
                .reopen_requests
                .insert_auto_approved(requester.id, body.week_start, requester.id, request_reason)
                .await?;
            (new_id, Some(affected))
        } else {
            let (new_id, _created_at) = app_state
                .db
                .reopen_requests
                .insert_pending(requester.id, body.week_start, request_reason)
                .await?;
            (new_id, None)
        };

    let entries_reopened = reopened_entries
        .as_ref()
        .map(|entries| entries.len() as i64)
        .unwrap_or(0);

    if let Some(entries) = reopened_entries.as_ref() {
        audit_reopened_entries(&app_state.pool, requester.id, entries).await;
    }

    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "reopen_requests",
        new_request_id,
        None,
        Some(serde_json::json!({
            "week_start": body.week_start,
            "status": initial_status,
            "reason": request_reason,
        })),
    )
    .await;

    if should_auto_approve {
        // Silent by design (mirrors submission auto-approval): no one is
        // notified and no emails are sent, to either the requester or the
        // approvers.
        return Ok(Json(serde_json::json!({
            "ok": true,
            "id": new_request_id,
            "status": initial_status,
            "auto_approved": true,
            "entries_reopened": entries_reopened,
        })));
    }

    // Notify all approvers that a manual reopen request is pending.
    let approver_ids_for_notification = approver_ids_to_notify(&app_state.pool, &requester).await;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, body.week_start);
    let week_iso = body.week_start.format("%Y-%m-%d").to_string();
    let requester_full_name = requester.full_name();
    let frontend_body_created = serde_json::json!({
        "week": week_iso,
        "requester_name": requester_full_name,
    })
    .to_string();
    for approver_id in &approver_ids_for_notification {
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            *approver_id,
            "reopen_request_created",
            "reopen_request_created_title",
            "reopen_request_created_body",
            vec![
                ("requester_name", requester_full_name.clone()),
                ("week_label", week_label.clone()),
            ],
            &frontend_body_created,
            true,
            Some("reopen_request"),
            Some(new_request_id),
        )
        .await;
    }
    Ok(Json(serde_json::json!({
        "ok": true,
        "id": new_request_id,
        "status": initial_status,
        "auto_approved": false,
    })))
}

pub async fn list_mine(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
    // Pure-admin users have no reopen requests.
    if !requester.tracks_time {
        return Err(AppError::Forbidden);
    }
    let rrs = app_state.db.reopen_requests.list_mine(requester.id).await?;
    Ok(Json(rrs.into_iter().map(repo_rr_to_service).collect()))
}

pub async fn list_pending(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rrs = if requester.is_admin() {
        app_state.db.reopen_requests.list_pending_admin().await?
    } else {
        app_state
            .db
            .reopen_requests
            .list_pending_for_lead(requester.id)
            .await?
    };
    Ok(Json(rrs.into_iter().map(repo_rr_to_service).collect()))
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(request_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let (reopen_request_repo, reopened_entries) = app_state
        .db
        .reopen_requests
        .approve_with_access_check(request_id, requester.id, requester.is_admin())
        .await?;
    let reopen_request = repo_rr_to_service(reopen_request_repo);
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, reopen_request.week_start);
    let week_iso = reopen_request.week_start.format("%Y-%m-%d").to_string();
    audit_reopened_entries(&app_state.pool, requester.id, &reopened_entries).await;
    let entries_reopened = reopened_entries.len() as i64;
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "reopen_requests",
        request_id,
        serde_json::to_value(&reopen_request).ok(),
        Some(serde_json::json!({"status": "approved"})),
    )
    .await;
    // Notify the employee whose week was reopened (in-app only when self-approved).
    let frontend_body_approved = format!("{{\"week\":\"{}\"}}", week_iso);
    if reopen_request.user_id != requester.id {
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            reopen_request.user_id,
            "reopen_approved",
            "reopen_approved_title",
            "reopen_approved_body",
            vec![("week_label", week_label.clone())],
            &frontend_body_approved,
            true,
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    } else {
        // Self-approval by admin: in-app only, no email.
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            reopen_request.user_id,
            "reopen_approved",
            "reopen_approved_title",
            "reopen_approved_body",
            vec![("week_label", week_label.clone())],
            &frontend_body_approved,
            false,
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    }
    // If an admin acted, notify all other explicitly assigned approvers for
    // this user so they know the item left their pending queue.
    notify_assigned_approvers_if_admin_acted(
        &app_state,
        &language,
        &requester,
        reopen_request.user_id,
        request_id,
        "reopen_approved_by_admin",
        "reopen_approved_by_admin_title",
        "reopen_approved_by_admin_body",
        week_label,
        &week_iso,
        vec![],
    )
    .await;
    Ok(Json(
        serde_json::json!({ "ok": true, "entries_reopened": entries_reopened }),
    ))
}

pub async fn reject(
    State(app_state): State<AppState>,
    requester: User,
    Path(request_id): Path<i64>,
    Json(body): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rejection_reason = body.reason.trim();
    if rejection_reason.is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if rejection_reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    let before = app_state
        .db
        .reopen_requests
        .reject_with_access_check(
            request_id,
            requester.id,
            requester.is_admin(),
            rejection_reason,
        )
        .await?;
    let before = repo_rr_to_service(before);
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "reopen_requests",
        request_id,
        serde_json::to_value(&before).ok(),
        Some(serde_json::json!({ "status": "rejected", "reason": rejection_reason })),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, before.week_start);
    let week_iso = before.week_start.format("%Y-%m-%d").to_string();
    // Notify the employee whose reopen request was rejected (in-app only when self-rejected).
    let frontend_body_rejected = format!(
        "{{\"week\":\"{}\",\"reason\":{}}}",
        week_iso,
        serde_json::json!(rejection_reason),
    );
    if before.user_id != requester.id {
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            before.user_id,
            "reopen_rejected",
            "reopen_rejected_title",
            "reopen_rejected_body",
            vec![
                ("week_label", week_label.clone()),
                ("reason", rejection_reason.to_string()),
            ],
            &frontend_body_rejected,
            true,
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    } else {
        // Self-rejection by admin: in-app only, no email.
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            before.user_id,
            "reopen_rejected",
            "reopen_rejected_title",
            "reopen_rejected_body",
            vec![
                ("week_label", week_label.clone()),
                ("reason", rejection_reason.to_string()),
            ],
            &frontend_body_rejected,
            false,
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    }
    // Symmetric with approve: if an admin rejected a request, notify all other
    // explicitly assigned approvers for this user so they know the item left
    // their queue.
    notify_assigned_approvers_if_admin_acted(
        &app_state,
        &language,
        &requester,
        before.user_id,
        request_id,
        "reopen_rejected_by_admin",
        "reopen_rejected_by_admin_title",
        "reopen_rejected_by_admin_body",
        week_label,
        &week_iso,
        vec![("reason", rejection_reason.to_string())],
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}
