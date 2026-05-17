//! Weekly reopen-request workflow.
//!
//! An employee whose week is fully `submitted` or partially `approved` can
//! request to make the week editable again.  The approver (admin or the
//! configured team-lead) reviews the request.  When the **requester's own**
//! flag `allow_reopen_without_approval` is TRUE, the request is auto-approved
//! immediately and all explicitly assigned approvers receive an informational
//! notification.
//!
//! Approval / auto-approval reopens the week atomically: every submitted,
//! approved, or rejected entry for `[week_start, week_start+6 days]` is reset
//! to `'draft'` and audit-logged.  The week is treated atomically — individual
//! entries inside a submitted week cannot be edited, so the reopen workflow
//! is the only way to change submitted data after the fact.

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

const ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL: &str = "\
    SELECT TRUE \
    FROM user_approvers ua \
    JOIN users subject ON subject.id = ua.user_id \
    JOIN users approver ON approver.id = ua.approver_id \
    WHERE ua.user_id = $1 AND ua.approver_id = $2 \
    AND subject.active=TRUE AND subject.role != 'admin' \
    AND approver.active=TRUE AND approver.role IN ('team_lead','admin') \
    FOR UPDATE OF ua";

#[derive(FromRow, Serialize)]
pub struct ReopenRequest {
    pub id: i64,
    pub user_id: i64,
    pub week_start: NaiveDate,
    /// Set once the request is approved or rejected (NULL while pending).
    pub reviewed_by: Option<i64>,
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

/// Atomically reopen a week: reset every submitted, approved, or rejected
/// entry in `[week_start, week_start+6]` back to draft.  Caller is the
/// **acting** user (approver or self); `subject` is the user whose week is
/// being reopened.  Returns the affected entry ids and their previous status
/// so the caller can commit the whole state transition first and audit after.
async fn perform_reopen_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    subject_id: i64,
    week_start: NaiveDate,
) -> AppResult<Vec<(i64, String)>> {
    let week_end = week_start + chrono::Duration::days(6);

    // Advisory lock on subject_id serializes concurrent reopen attempts for the
    // same user, preventing two simultaneous transactions from both reading
    // 'submitted' entries and racing to reset them both to draft.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(subject_id)
        .execute(&mut **tx)
        .await?;

    let affected: Vec<(i64, String)> = sqlx::query_as(
        "SELECT te.id, te.status FROM time_entries te \
             WHERE te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
             AND te.status IN ('submitted','approved','rejected') \
             FOR UPDATE",
    )
    .bind(subject_id)
    .bind(week_start)
    .bind(week_end)
    .fetch_all(&mut **tx)
    .await?;

    if affected.is_empty() {
        return Err(AppError::BadRequest(
            "Cannot request edit - this week has no submitted, approved, or rejected entries.".into(),
        ));
    }

    let entry_ids: Vec<i64> = affected.iter().map(|(id, _)| *id).collect();

    crate::repository::time_entries::validate_entries_after_reopen(
        &mut **tx,
        subject_id,
        &entry_ids,
    )
    .await?;

    sqlx::query(
        "UPDATE time_entries SET status='draft', submitted_at=NULL, reviewed_by=NULL, \
         reviewed_at=NULL, rejection_reason=NULL, updated_at=CURRENT_TIMESTAMP \
         WHERE id = ANY($1)",
    )
    .bind(&entry_ids)
    .execute(&mut **tx)
    .await?;

    Ok(affected)
}

async fn audit_reopened_entries(
    pool: &crate::db::DatabasePool,
    actor_id: i64,
    affected: &[(i64, String)],
) {
    for (entry_id, prev_status) in affected {
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
}

/// Collect all user-ids that should be notified as "approver" for a reopen
/// request created by `requester`.  Rules:
///
/// | Requester role | Scenario                            | Notified set                          |
/// |----------------|-------------------------------------|---------------------------------------|
/// | employee       | any                                 | explicitly assigned approvers         |
/// | team_lead      | has designated approver             | explicitly assigned approvers         |
/// | admin          | has designated approver(s)          | those assigned approver(s) only       |
///
/// BTreeSet deduplicates ids.
/// Non-admin requesters are excluded from the result.
async fn approver_ids_to_notify(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    let mut ids: std::collections::BTreeSet<i64> = Default::default();
    ids.extend(crate::auth::approval_recipient_ids(pool, requester).await);
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
    crate::notifications::load_language(pool).await
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewReopen>,
) -> AppResult<Json<serde_json::Value>> {
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
            "Cannot request edit - this week has no submitted, approved, or rejected entries.".into(),
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

    // Determine flow:
    //   * User has `allow_reopen_without_approval=TRUE` → auto_approved
    //   * Otherwise → pending, notify all approvers
    let should_auto_approve = requester.allow_reopen_without_approval;

    // Non-admin users must have at least one explicit approver available.
    // Admin users may still create requests without assigned approvers. Any
    // admin can review the request, but admins are not auto-notified unless
    // explicitly assigned.
    let _approvers =
        crate::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;

    let initial_status = if should_auto_approve {
        "auto_approved"
    } else {
        "pending"
    };

    // Collect all users that should be notified as approvers (before the DB
    // insert, so we can reuse the result for both auto and pending paths).
    let approver_ids_for_notification = approver_ids_to_notify(&app_state.pool, &requester).await;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, body.week_start);
    let week_iso = body.week_start.format("%Y-%m-%d").to_string();

    let (new_request_id, reopened_entries): (i64, Option<Vec<(i64, String)>>) =
        if should_auto_approve {
            let mut transaction = app_state.pool.begin().await?;
            let new_id: i64 = sqlx::query_scalar(
                "INSERT INTO reopen_requests(user_id, week_start, status, reviewed_by, reviewed_at) \
                 VALUES ($1,$2,$3,$4, CURRENT_TIMESTAMP) \
                 RETURNING id",
            )
            .bind(requester.id)
            .bind(body.week_start)
            .bind(initial_status)
            .bind(requester.id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|e| {
                tracing::warn!(target:"zerf::reopen", "create reopen failed: {e}");
                AppError::Conflict("A pending request for this week already exists.".into())
            })?;
            let affected = perform_reopen_in_tx(&mut transaction, requester.id, body.week_start)
                .await?;
            transaction.commit().await?;
            (new_id, Some(affected))
        } else {
            let new_id: i64 = sqlx::query_scalar(
                "INSERT INTO reopen_requests(user_id, week_start, status) \
                 VALUES ($1,$2,$3) \
                 RETURNING id",
            )
            .bind(requester.id)
            .bind(body.week_start)
            .bind(initial_status)
            .fetch_one(&app_state.pool)
            .await
            .map_err(|e| {
                tracing::warn!(target:"zerf::reopen", "create reopen failed: {e}");
                AppError::Conflict("A pending request for this week already exists.".into())
            })?;
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
        })),
    )
    .await;

    let requester_full_name = requester.full_name();

    if should_auto_approve {
        // In-app only: the requester triggered the auto-approve themselves.
        let frontend_body_self = format!("{{\"week\":\"{}\"}}", week_iso);
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            requester.id,
            "reopen_auto_approved",
            "reopen_approved_title",
            "reopen_approved_body",
            vec![("week_label", week_label.clone())],
            &frontend_body_self,
            false,
            Some("reopen_request"),
            Some(new_request_id),
        )
        .await;
        // Notify each approver that the reopen was auto-approved (informational).
        let frontend_body_approver = format!(
            "{{\"week\":\"{}\",\"requester_name\":{}}}",
            week_iso,
            serde_json::json!(&requester_full_name),
        );
        for approver_id in &approver_ids_for_notification {
            notifications::create_with_frontend_body(
                &app_state,
                &language,
                *approver_id,
                "reopen_auto_approved_notice",
                "reopen_auto_approved_notice_title",
                "reopen_auto_approved_notice_body",
                vec![
                    ("requester_name", requester_full_name.clone()),
                    ("week_label", week_label.clone()),
                ],
                &frontend_body_approver,
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
            "auto_approved": true,
            "entries_reopened": entries_reopened,
        })))
    } else {
        // Notify all approvers that a manual reopen request is pending.
        let frontend_body_created = format!(
            "{{\"week\":\"{}\",\"requester_name\":{}}}",
            week_iso,
            serde_json::json!(&requester_full_name),
        );
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
}

fn repo_rr_to_service(r: crate::repository::ReopenRequest) -> ReopenRequest {
    ReopenRequest {
        id: r.id,
        user_id: r.user_id,
        week_start: r.week_start,
        reviewed_by: r.reviewed_by,
        status: r.status,
        reviewed_at: r.reviewed_at,
        rejection_reason: r.rejection_reason,
        created_at: r.created_at,
    }
}

pub async fn list_mine(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
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

/// If an admin acted on a request, notify all other explicitly assigned
/// approvers for the request's user so they know the item left their pending
/// queue.
#[allow(clippy::too_many_arguments)]
async fn notify_assigned_approvers_if_admin_acted(
    app_state: &AppState,
    language: &i18n::Language,
    requester: &User,
    request_user_id: i64,
    request_id: i64,
    action_key: &str,
    action_title_key: &str,
    action_body_key: &str,
    week_label: String,
    week_iso: &str,
    extra_params: Vec<(&'static str, String)>,
) {
    if !requester.is_admin() {
        return;
    }
    let approver_ids: Vec<i64> = match app_state.db.users.get_approver_ids(request_user_id).await {
        Ok(ids) => ids
            .into_iter()
            .filter(|approver_id| *approver_id != requester.id)
            .collect(),
        Err(_) => return,
    };
    if approver_ids.is_empty() {
        return;
    }
    let employee_full_name: String =
        sqlx::query_scalar("SELECT first_name || ' ' || last_name FROM users WHERE id=$1")
            .bind(request_user_id)
            .fetch_optional(&app_state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("User {request_user_id}"));

    // Build frontend JSON with the employee's name (not the admin's).
    let reason = extra_params.iter().find(|(k, _)| *k == "reason").map(|(_, v)| v.as_str());
    let frontend_body = match reason {
        Some(r) => format!(
            "{{\"week\":\"{}\",\"requester_name\":{},\"reason\":{}}}",
            week_iso, serde_json::json!(&employee_full_name), serde_json::json!(r)
        ),
        None => format!(
            "{{\"week\":\"{}\",\"requester_name\":{}}}",
            week_iso, serde_json::json!(&employee_full_name)
        ),
    };

    let mut params = vec![
        ("requester_name", employee_full_name),
        ("week_label", week_label),
    ];
    params.extend(extra_params);
    for approver_id in approver_ids {
        notifications::create_with_frontend_body(
            app_state,
            language,
            approver_id,
            action_key,
            action_title_key,
            action_body_key,
            params.clone(),
            &frontend_body,
            true,
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    }
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(request_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut transaction = app_state.pool.begin().await?;
    let reopen_request: ReopenRequest = sqlx::query_as(
        "SELECT id, user_id, week_start, reviewed_by, status, reviewed_at, \
         rejection_reason, created_at \
         FROM reopen_requests WHERE id=$1 FOR UPDATE",
    )
    .bind(request_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if reopen_request.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    // Non-admin team leads cannot approve their own reopen request — only an
    // admin may self-approve (e.g. an admin correcting their own timesheet).
    if reopen_request.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin team leads must be explicitly assigned as approver for this
    // user; any admin may approve unconditionally.
    if !requester.is_admin() {
        let is_assigned_approver: Option<bool> =
            sqlx::query_scalar(ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL)
                .bind(reopen_request.user_id)
                .bind(requester.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if is_assigned_approver.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, reopen_request.week_start);
    let week_iso = reopen_request.week_start.format("%Y-%m-%d").to_string();
    let reopened_entries =
        perform_reopen_in_tx(&mut transaction, reopen_request.user_id, reopen_request.week_start)
            .await?;
    let rows_approved = sqlx::query(
        "UPDATE reopen_requests SET status='approved', reviewed_by=$2, reviewed_at=CURRENT_TIMESTAMP \
         WHERE id=$1 AND status='pending'",
    )
    .bind(request_id)
    .bind(requester.id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if rows_approved == 0 {
        return Err(AppError::Conflict(
            "Reopen request was already resolved by someone else.".into(),
        ));
    }
    transaction.commit().await?;
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
    // Use a transaction with row-level lock, mirroring the approve handler,
    // so that the authorization check and the status update are atomic.
    let mut transaction = app_state.pool.begin().await?;
    let reopen_request: ReopenRequest = sqlx::query_as(
        "SELECT id, user_id, week_start, reviewed_by, status, reviewed_at, \
         rejection_reason, created_at \
         FROM reopen_requests WHERE id=$1 FOR UPDATE",
    )
    .bind(request_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if reopen_request.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    // Non-admin team leads cannot reject their own reopen request.
    if reopen_request.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin team leads must be explicitly assigned as approver for this user.
    if !requester.is_admin() {
        let is_assigned_approver: Option<bool> =
            sqlx::query_scalar(ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL)
                .bind(reopen_request.user_id)
                .bind(requester.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if is_assigned_approver.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    let rows_claimed = sqlx::query(
        "UPDATE reopen_requests SET status='rejected', reviewed_by=$2, reviewed_at=CURRENT_TIMESTAMP, \
         rejection_reason=$3 WHERE id=$1 AND status='pending'",
    )
    .bind(request_id)
    .bind(requester.id)
    .bind(rejection_reason)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if rows_claimed == 0 {
        return Err(AppError::Conflict(
            "Request was already resolved by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "reopen_requests",
        request_id,
        serde_json::to_value(&reopen_request).ok(),
        Some(serde_json::json!({ "status": "rejected", "reason": rejection_reason })),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, reopen_request.week_start);
    let week_iso = reopen_request.week_start.format("%Y-%m-%d").to_string();
    // Notify the employee whose reopen request was rejected (in-app only when self-rejected).
    let frontend_body_rejected = format!(
        "{{\"week\":\"{}\",\"reason\":{}}}",
        week_iso,
        serde_json::json!(rejection_reason),
    );
    if reopen_request.user_id != requester.id {
        notifications::create_with_frontend_body(
            &app_state,
            &language,
            reopen_request.user_id,
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
            reopen_request.user_id,
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
        reopen_request.user_id,
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
