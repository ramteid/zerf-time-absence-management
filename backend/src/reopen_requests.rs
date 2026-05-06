//! Weekly reopen-request workflow.
//!
//! An employee whose week is fully `submitted` or partially `approved` can
//! request to make the week editable again.  The approver (admin or the
//! configured team-lead) reviews the request.  When the **requester's own**
//! flag `allow_reopen_without_approval` is TRUE, the request is auto-approved
//! immediately and all relevant approvers (designated approver + all admins)
//! receive an informational notification.
//!
//! Approval / auto-approval reopens the week atomically:
//!   * all non-draft entries for `[week_start, week_start+6 days]` are reset
//!     to `'draft'` (audit-logged per entry);
//!   * any open `change_requests` for those entries are auto-rejected with
//!     a system reason (also audit-logged).

use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::notifications;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(FromRow, Serialize)]
pub struct ReopenRequest {
    pub id: i64,
    pub user_id: i64,
    pub week_start: NaiveDate,
    pub approver_id: i64,
    pub status: String,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct NewReopen {
    pub week_start: NaiveDate,
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

fn assert_monday(d: NaiveDate) -> AppResult<()> {
    if d.weekday() != chrono::Weekday::Mon {
        return Err(AppError::BadRequest(
            "week_start must be a Monday (ISO).".into(),
        ));
    }
    Ok(())
}

/// Atomically reopen a week: reset every non-draft entry to draft and
/// auto-reject open change_requests for those entries. Caller is the
/// **acting** user (approver or self); `subject` is the user whose week
/// is being reopened.  Counts how many entries were affected.
async fn perform_reopen(
    pool: &crate::db::DatabasePool,
    actor_id: i64,
    subject_id: i64,
    week_start: NaiveDate,
) -> AppResult<i64> {
    let week_end = week_start + chrono::Duration::days(6);
    let mut tx = pool.begin().await?;

    let affected: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, status FROM time_entries \
         WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 AND status<>'draft' FOR UPDATE",
    )
    .bind(subject_id)
    .bind(week_start)
    .bind(week_end)
    .fetch_all(&mut *tx)
    .await?;

    if affected.is_empty() {
        // Nothing to reopen — caller should have validated, but be defensive.
        tx.rollback().await?;
        return Ok(0);
    }

    sqlx::query(
        "UPDATE time_entries SET status='draft', submitted_at=NULL, reviewed_by=NULL, \
         reviewed_at=NULL, rejection_reason=NULL, updated_at=CURRENT_TIMESTAMP \
         WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 AND status<>'draft'",
    )
    .bind(subject_id)
    .bind(week_start)
    .bind(week_end)
    .execute(&mut *tx)
    .await?;

    // Auto-reject open change_requests for these entries.
    let entry_ids: Vec<i64> = affected.iter().map(|(id, _)| *id).collect();
    sqlx::query(
        "UPDATE change_requests \
         SET status='rejected', \
             reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, \
             rejection_reason='Auto-cancelled: week was reopened for editing' \
         WHERE status='open' AND time_entry_id = ANY($2)",
    )
    .bind(actor_id)
    .bind(&entry_ids)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Audit-log per entry (after commit; best-effort).
    for (entry_id, prev_status) in &affected {
        audit::log(
            pool,
            actor_id,
            "reopened",
            "time_entries",
            *entry_id,
            Some(serde_json::json!({"status": prev_status})),
            Some(serde_json::json!({"status":"draft"})),
        )
        .await;
    }
    Ok(affected.len() as i64)
}

/// Collect all user-ids that should be notified as "approver" for a reopen
/// request created by `requester`.  Rules:
///
/// | Requester role | Scenario                            | Notified set                          |
/// |----------------|-------------------------------------|---------------------------------------|
/// | employee       | any                                 | designated approver + all admins      |
/// | team_lead      | has designated approver             | that approver + all admins            |
/// | admin          | any                                 | all other admins                      |
///
/// BTreeSet deduplicates (e.g. when the designated approver IS an admin).
/// Non-admin requesters are excluded from the result.
async fn approver_ids_to_notify(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    let mut ids: std::collections::BTreeSet<i64> = Default::default();
    if let Some(approver_id) = requester.approver_id {
        // Only include the designated approver if they are still active;
        // a deactivated approver cannot log in to review the request.
        let is_active: bool = sqlx::query_scalar("SELECT active FROM users WHERE id=$1")
            .bind(approver_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
        if is_active {
            ids.insert(approver_id);
        }
    }
    if let Ok(admin_ids) =
        sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE active=TRUE AND role='admin'")
            .fetch_all(pool)
            .await
    {
        ids.extend(admin_ids);
    }
    // Only exclude the requester when they are NOT an admin.  An admin who
    // requests a reopen for their own week still needs a notification so
    // they can approve it from the dashboard (especially when they are the
    // only admin).
    if !requester.is_admin() {
        ids.remove(&requester.id);
    }
    ids.into_iter().collect()
}

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::warn!(target:"zerf::reopen", "load notification language failed: {e}");
            i18n::Language::default()
        }
    }
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewReopen>,
) -> AppResult<Json<serde_json::Value>> {
    assert_monday(body.week_start)?;
    let week_end = body.week_start + chrono::Duration::days(6);

    // Empty-week / nothing-to-reopen guard: only weeks with at least one
    // non-draft entry are eligible.
    let non_draft_entry_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM time_entries \
         WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 AND status<>'draft'",
    )
    .bind(requester.id)
    .bind(body.week_start)
    .bind(week_end)
    .fetch_one(&app_state.pool)
    .await?;
    if non_draft_entry_count == 0 {
        return Err(AppError::BadRequest(
            "Nothing to reopen — this week has no submitted or approved entries.".into(),
        ));
    }

    // Reject duplicate pending request (DB also has a unique partial index).
    let existing_pending_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM reopen_requests WHERE user_id=$1 AND week_start=$2 AND status='pending'",
    )
    .bind(requester.id)
    .bind(body.week_start)
    .fetch_optional(&app_state.pool)
    .await?;
    if let Some(existing_request_id) = existing_pending_id {
        return Err(AppError::Conflict(format!(
            "A pending reopen request already exists (id {existing_request_id})."
        )));
    }

    // Determine flow:
    //   * User has `allow_reopen_without_approval=TRUE` → auto_approved
    //   * Otherwise → pending, notify all approvers
    let should_auto_approve = requester.allow_reopen_without_approval;

    // `recorded_approver` is stored as the primary approver on the request row.
    // Admins do not require a designated approver, so their own id is recorded.
    let recorded_approver_id: i64 = requester.approver_id.unwrap_or(requester.id);

    let initial_status = if should_auto_approve {
        "auto_approved"
    } else {
        "pending"
    };

    // Collect all users that should be notified as approvers (before the DB
    // insert, so we can reuse the result for both auto and pending paths).
    let approver_ids_for_notification = approver_ids_to_notify(&app_state.pool, &requester).await;

    // Insert the request row FIRST so that if perform_reopen fails, the row
    // can be cleaned up — but if only the INSERT succeeded, we won't have
    // entries silently reopened without a record.
    let insert_result: (i64, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO reopen_requests(user_id, week_start, approver_id, status, reviewed_at) \
         VALUES ($1,$2,$3,$4, CASE WHEN $4 IN ('auto_approved') THEN CURRENT_TIMESTAMP ELSE NULL END) \
         RETURNING id, created_at",
    )
    .bind(requester.id)
    .bind(body.week_start)
    .bind(recorded_approver_id)
    .bind(initial_status)
    .fetch_one(&app_state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(target:"zerf::reopen", "create reopen failed: {e}");
        AppError::Conflict("A pending request for this week already exists.".into())
    })?;
    let new_request_id = insert_result.0;

    // For the auto-approve flow, now perform the actual reopen. If it fails,
    // delete the request row so the user can retry cleanly.
    let entries_reopened = if should_auto_approve {
        match perform_reopen(&app_state.pool, requester.id, requester.id, body.week_start).await {
            Ok(count) => count,
            Err(e) => {
                let _ = sqlx::query("DELETE FROM reopen_requests WHERE id=$1")
                    .bind(new_request_id)
                    .execute(&app_state.pool)
                    .await;
                return Err(e);
            }
        }
    } else {
        0
    };

    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "reopen_requests",
        new_request_id,
        None,
        Some(serde_json::json!({
            "week_start": body.week_start,
            "approver_id": recorded_approver_id,
            "status": initial_status,
        })),
    )
    .await;

    let language = notification_language(&app_state.pool).await;
    let requester_full_name = format!("{} {}", requester.first_name, requester.last_name);

    if should_auto_approve {
        // Notify the requester that their week was reopened automatically.
        notifications::create_translated(
            &app_state,
            &language,
            requester.id,
            "reopen_auto_approved",
            "reopen_auto_approved_title",
            "reopen_auto_approved_body",
            vec![
                ("week_start", i18n::format_date(&language, body.week_start)),
                ("entry_count", i18n::entry_count(&language, entries_reopened)),
            ],
            Some("reopen_request"),
            Some(new_request_id),
        )
        .await;
        // Notify each approver that the reopen was auto-approved (informational).
        for approver_id in &approver_ids_for_notification {
            notifications::create_translated(
                &app_state,
                &language,
                *approver_id,
                "reopen_auto_approved_notice",
                "reopen_auto_approved_notice_title",
                "reopen_auto_approved_notice_body",
                vec![
                    ("requester_name", requester_full_name.clone()),
                    ("week_start", i18n::format_date(&language, body.week_start)),
                    ("entry_count", i18n::entry_count(&language, entries_reopened)),
                ],
                Some("reopen_request"),
                Some(new_request_id),
            )
            .await;
        }
        Ok(Json(serde_json::json!({
            "ok": true,
            "id": new_request_id,
            "status": initial_status,
            "auto_approved": true,
            "entries_reopened": entries_reopened,
        })))
    } else {
        // Notify all approvers that a manual reopen request is pending.
        for approver_id in &approver_ids_for_notification {
            notifications::create_translated(
                &app_state,
                &language,
                *approver_id,
                "reopen_request_created",
                "reopen_request_created_title",
                "reopen_request_created_body",
                vec![
                    ("requester_name", requester_full_name.clone()),
                    ("week_start", i18n::format_date(&language, body.week_start)),
                ],
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
}

pub async fn list_mine(State(app_state): State<AppState>, requester: User) -> AppResult<Json<Vec<ReopenRequest>>> {
    Ok(Json(
        sqlx::query_as::<_, ReopenRequest>(
            "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
             rejection_reason, created_at \
             FROM reopen_requests WHERE user_id=$1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(requester.id)
        .fetch_all(&app_state.pool)
        .await?,
    ))
}

pub async fn list_pending(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    // Admins see all pending requests; leads see only requests in their own approval queue.
    let pending_requests: Vec<ReopenRequest> = if requester.is_admin() {
        sqlx::query_as(
            "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
             rejection_reason, created_at \
             FROM reopen_requests WHERE status='pending' ORDER BY created_at",
        )
        .fetch_all(&app_state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
             rejection_reason, created_at \
             FROM reopen_requests WHERE status='pending' AND approver_id=$1 AND user_id NOT IN (SELECT id FROM users WHERE role='admin') ORDER BY created_at",
        )
        .bind(requester.id)
        .fetch_all(&app_state.pool)
        .await?
    };
    Ok(Json(pending_requests))
}

async fn load_pending(pool: &crate::db::DatabasePool, id: i64) -> AppResult<ReopenRequest> {
    sqlx::query_as::<_, ReopenRequest>(
        "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
         rejection_reason, created_at \
         FROM reopen_requests WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// If an admin acted on a request that was in a team lead's queue, notify
/// that lead so they know the item left their queue.
async fn notify_lead_if_admin_acted(
    app_state: &AppState,
    language: &i18n::Language,
    requester: &User,
    reopen_request: &ReopenRequest,
    request_id: i64,
    action_key: &str,
    action_title_key: &str,
    action_body_key: &str,
    extra_params: Vec<(&'static str, String)>,
) {
    if !requester.is_admin() || reopen_request.approver_id == requester.id {
        return;
    }
    let original_approver_is_lead: Option<bool> = sqlx::query_scalar(
        "SELECT TRUE FROM users WHERE id=$1 AND active=TRUE AND role='team_lead'",
    )
    .bind(reopen_request.approver_id)
    .fetch_optional(&app_state.pool)
    .await
    .ok()
    .flatten();
    if original_approver_is_lead.is_none() {
        return;
    }
    let employee_full_name: String =
        sqlx::query_scalar("SELECT first_name || ' ' || last_name FROM users WHERE id=$1")
            .bind(reopen_request.user_id)
            .fetch_optional(&app_state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("User {}", reopen_request.user_id));
    let mut params = vec![
        ("requester_name", employee_full_name),
        ("week_start", i18n::format_date(language, reopen_request.week_start)),
    ];
    params.extend(extra_params);
    notifications::create_translated(
        app_state,
        language,
        reopen_request.approver_id,
        action_key,
        action_title_key,
        action_body_key,
        params,
        Some("reopen_request"),
        Some(request_id),
    )
    .await;
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(request_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let reopen_request = load_pending(&app_state.pool, request_id).await?;
    if reopen_request.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    if !requester.is_admin() && reopen_request.approver_id != requester.id {
        return Err(AppError::Forbidden);
    }
    // Team leads must not act on reopen requests from admin users.
    if !requester.is_admin() {
        let is_admin_user: Option<bool> =
            sqlx::query_scalar("SELECT TRUE FROM users WHERE id = $1 AND role = 'admin'")
                .bind(reopen_request.user_id)
                .fetch_optional(&app_state.pool)
                .await?;
        if is_admin_user.is_some() {
            return Err(AppError::Forbidden);
        }
    }
    // Atomically claim the request before doing the (idempotent-ish) reopen.
    // The status guard prevents a TOCTOU race where two approvers click at
    // the same time and both run perform_reopen.
    let rows_claimed = sqlx::query(
        "UPDATE reopen_requests SET status='approved', reviewed_at=CURRENT_TIMESTAMP \
         WHERE id=$1 AND status='pending'",
    )
    .bind(request_id)
    .execute(&app_state.pool)
    .await?
    .rows_affected();
    if rows_claimed == 0 {
        return Err(AppError::Conflict(
            "Request was already resolved by someone else.".into(),
        ));
    }
    let entries_reopened = match perform_reopen(&app_state.pool, requester.id, reopen_request.user_id, reopen_request.week_start).await {
        Ok(count) => count,
        Err(e) => {
            // Revert the approval claim so the request can be retried.
            let _ = sqlx::query(
                "UPDATE reopen_requests SET status='pending', reviewed_at=NULL WHERE id=$1",
            )
            .bind(request_id)
            .execute(&app_state.pool)
            .await;
            return Err(e);
        }
    };
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "reopen_requests",
        request_id,
        Some(serde_json::to_value(&reopen_request).unwrap()),
        Some(serde_json::json!({"status": "approved"})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    // Notify the employee whose week was reopened.
    notifications::create_translated(
        &app_state,
        &language,
        reopen_request.user_id,
        "reopen_approved",
        "reopen_approved_title",
        "reopen_approved_body",
        vec![("week_start", i18n::format_date(&language, reopen_request.week_start))],
        Some("reopen_request"),
        Some(request_id),
    )
    .await;
    // If the approving user is an admin but NOT the recorded approver on the
    // request (i.e. a different admin stepped in), notify the original approver
    // so they know the item left their queue.
    notify_lead_if_admin_acted(
        &app_state,
        &language,
        &requester,
        &reopen_request,
        request_id,
        "reopen_approved_by_admin",
        "reopen_approved_by_admin_title",
        "reopen_approved_by_admin_body",
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
    let reopen_request = load_pending(&app_state.pool, request_id).await?;
    if reopen_request.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    if !requester.is_admin() && reopen_request.approver_id != requester.id {
        return Err(AppError::Forbidden);
    }
    // Team leads must not act on reopen requests from admin users.
    if !requester.is_admin() {
        let is_admin_user: Option<bool> =
            sqlx::query_scalar("SELECT TRUE FROM users WHERE id = $1 AND role = 'admin'")
                .bind(reopen_request.user_id)
                .fetch_optional(&app_state.pool)
                .await?;
        if is_admin_user.is_some() {
            return Err(AppError::Forbidden);
        }
    }
    // Use optimistic locking: only proceed if status is still 'pending'.
    let rows_claimed = sqlx::query(
        "UPDATE reopen_requests SET status='rejected', reviewed_at=CURRENT_TIMESTAMP, \
         rejection_reason=$2 WHERE id=$1 AND status='pending'",
    )
    .bind(request_id)
    .bind(rejection_reason)
    .execute(&app_state.pool)
    .await?
    .rows_affected();
    if rows_claimed == 0 {
        return Err(AppError::Conflict(
            "Request was already resolved by someone else.".into(),
        ));
    }
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "reopen_requests",
        request_id,
        Some(serde_json::to_value(&reopen_request).unwrap()),
        Some(serde_json::json!({ "status": "rejected", "reason": rejection_reason })),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    // Notify the employee whose reopen request was rejected.
    notifications::create_translated(
        &app_state,
        &language,
        reopen_request.user_id,
        "reopen_rejected",
        "reopen_rejected_title",
        "reopen_rejected_body",
        vec![
            ("week_start", i18n::format_date(&language, reopen_request.week_start)),
            ("reason", rejection_reason.to_string()),
        ],
        Some("reopen_request"),
        Some(request_id),
    )
    .await;
    // Symmetric with approve: if an admin rejected a request from a team lead's
    // queue, notify that team lead so they know the item left their queue.
    notify_lead_if_admin_acted(
        &app_state,
        &language,
        &requester,
        &reopen_request,
        request_id,
        "reopen_rejected_by_admin",
        "reopen_rejected_by_admin_title",
        "reopen_rejected_by_admin_body",
        vec![("reason", rejection_reason.to_string())],
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}
