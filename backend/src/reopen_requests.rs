//! Weekly reopen-request workflow.
//!
//! An employee whose week is fully `submitted` or partially `approved` can
//! request to make the week editable again.  The approver (admin or the
//! configured team-lead) reviews the request.  When the approver's policy
//! `allow_reopen_without_approval` is TRUE, the request is auto-approved
//! at creation time.  When the requester is themselves a lead/admin and
//! has no approver set, the same auto-approve path applies.
//!
//! Approval / auto-approval reopens the week atomically:
//!   * all non-draft entries for `[week_start, week_start+6 days]` are reset
//!     to `'draft'` (audit-logged per entry);
//!   * any open `change_requests` for those entries are auto-rejected with
//!     a system reason (also audit-logged).

use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
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
    if !entry_ids.is_empty() {
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
    }

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

/// Determine the approver to be recorded on a new request.  For employees
/// the column `approver_id` is mandatory (DB-level check); for leads/admins
/// who self-service, this returns `Ok(None)` indicating "auto-approve".
async fn resolve_approver(
    pool: &crate::db::DatabasePool,
    requester: &User,
) -> AppResult<Option<(i64, bool)>> {
    if let Some(aid) = requester.approver_id {
        let row: Option<(bool, String, bool)> = sqlx::query_as(
            "SELECT active, role, allow_reopen_without_approval FROM users WHERE id=$1",
        )
        .bind(aid)
        .fetch_optional(pool)
        .await?;
        match row {
            Some((true, role, policy)) if role == "team_lead" || role == "admin" => {
                Ok(Some((aid, policy)))
            }
            _ => Err(AppError::BadRequest(
                "Your approver is no longer available. Please contact an admin.".into(),
            )),
        }
    } else if requester.role == "team_lead" || requester.role == "admin" {
        // Self-service for leads/admins.
        Ok(None)
    } else {
        // Should never happen because of the DB CHECK, but keep a
        // friendly error rather than a 500 if it does.
        Err(AppError::BadRequest(
            "No approver assigned. Please contact an admin.".into(),
        ))
    }
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewReopen>,
) -> AppResult<Json<serde_json::Value>> {
    assert_monday(b.week_start)?;
    let week_end = b.week_start + chrono::Duration::days(6);

    // Empty-week / nothing-to-reopen guard: only weeks with at least one
    // non-draft entry are eligible.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM time_entries \
         WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 AND status<>'draft'",
    )
    .bind(u.id)
    .bind(b.week_start)
    .bind(week_end)
    .fetch_one(&s.pool)
    .await?;
    if n == 0 {
        return Err(AppError::BadRequest(
            "Nothing to reopen — this week has no submitted or approved entries.".into(),
        ));
    }

    // Reject duplicate pending request (DB also has a unique partial index).
    let pending: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM reopen_requests WHERE user_id=$1 AND week_start=$2 AND status='pending'",
    )
    .bind(u.id)
    .bind(b.week_start)
    .fetch_optional(&s.pool)
    .await?;
    if let Some(rid) = pending {
        return Err(AppError::Conflict(format!(
            "A pending reopen request already exists (id {rid})."
        )));
    }

    let approver = resolve_approver(&s.pool, &u).await?;

    // Determine flow:
    //   * Self-service lead/admin without approver_id  → auto_approved
    //   * Approver with policy `allow_reopen_without_approval=TRUE` → auto_approved
    //   * Otherwise → pending, notify approver
    let (status, recorded_approver) = match approver {
        None => ("auto_approved", u.id),
        Some((aid, true)) => ("auto_approved", aid),
        Some((aid, false)) => ("pending", aid),
    };

    // For the auto-approve flow we MUST reset the entries before persisting
    // the request row.  Otherwise a failure in `perform_reopen` (e.g. a
    // transient DB error) would leave an `auto_approved` row referencing a
    // week whose entries were never actually reopened — confusing the user
    // and bypassing the duplicate-pending guard for retries.
    let count = if status == "auto_approved" {
        perform_reopen(&s.pool, u.id, u.id, b.week_start).await?
    } else {
        0
    };

    let row: (i64, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO reopen_requests(user_id, week_start, approver_id, status, reviewed_at) \
         VALUES ($1,$2,$3,$4, CASE WHEN $4 IN ('auto_approved') THEN CURRENT_TIMESTAMP ELSE NULL END) \
         RETURNING id, created_at",
    )
    .bind(u.id)
    .bind(b.week_start)
    .bind(recorded_approver)
    .bind(status)
    .fetch_one(&s.pool)
    .await
    .map_err(|e| {
        tracing::warn!(target:"kitazeit::reopen", "create reopen failed: {e}");
        AppError::Conflict("A pending request for this week already exists.".into())
    })?;
    let req_id = row.0;

    audit::log(
        &s.pool,
        u.id,
        "created",
        "reopen_requests",
        req_id,
        None,
        Some(serde_json::json!({
            "week_start": b.week_start,
            "approver_id": recorded_approver,
            "status": status,
        })),
    )
    .await;

    if status == "auto_approved" {
        notifications::create(
            &s,
            u.id,
            "reopen_auto_approved",
            "Woche zur Bearbeitung freigegeben / Week reopened for editing",
            &format!(
                "Die Woche ab {} wurde wieder zur Bearbeitung freigegeben ({} Einträge).",
                b.week_start, count
            ),
            Some("reopen_request"),
            Some(req_id),
        )
        .await;
        Ok(Json(serde_json::json!({
            "ok": true,
            "id": req_id,
            "status": status,
            "auto_approved": true,
            "entries_reopened": count,
        })))
    } else {
        notifications::create(
            &s,
            recorded_approver,
            "reopen_request_created",
            "Neue Anfrage zur Wochenfreigabe / New week reopen request",
            &format!(
                "{} {} möchte die Woche ab {} wieder bearbeiten.",
                u.first_name, u.last_name, b.week_start
            ),
            Some("reopen_request"),
            Some(req_id),
        )
        .await;
        Ok(Json(serde_json::json!({
            "ok": true,
            "id": req_id,
            "status": status,
            "auto_approved": false,
        })))
    }
}

pub async fn list_mine(State(s): State<AppState>, u: User) -> AppResult<Json<Vec<ReopenRequest>>> {
    Ok(Json(
        sqlx::query_as::<_, ReopenRequest>(
            "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
             rejection_reason, created_at \
             FROM reopen_requests WHERE user_id=$1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(u.id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

pub async fn list_pending(
    State(s): State<AppState>,
    u: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    // Admins see all pending, leads see only their own queue.
    let rows: Vec<ReopenRequest> = if u.is_admin() {
        sqlx::query_as(
            "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
             rejection_reason, created_at \
             FROM reopen_requests WHERE status='pending' ORDER BY created_at",
        )
        .fetch_all(&s.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, user_id, week_start, approver_id, status, reviewed_at, \
             rejection_reason, created_at \
             FROM reopen_requests WHERE status='pending' AND approver_id=$1 ORDER BY created_at",
        )
        .bind(u.id)
        .fetch_all(&s.pool)
        .await?
    };
    Ok(Json(rows))
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

pub async fn approve(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let r = load_pending(&s.pool, id).await?;
    if r.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    if !u.is_admin() && r.approver_id != u.id {
        return Err(AppError::Forbidden);
    }
    // Atomically claim the request before doing the (idempotent-ish) reopen.
    // The status guard prevents a TOCTOU race where two approvers click at
    // the same time and both run perform_reopen.
    let claimed = sqlx::query(
        "UPDATE reopen_requests SET status='approved', reviewed_at=CURRENT_TIMESTAMP \
         WHERE id=$1 AND status='pending'",
    )
    .bind(id)
    .execute(&s.pool)
    .await?
    .rows_affected();
    if claimed == 0 {
        return Err(AppError::Conflict(
            "Request was already resolved by someone else.".into(),
        ));
    }
    let count = match perform_reopen(&s.pool, u.id, r.user_id, r.week_start).await {
        Ok(c) => c,
        Err(e) => {
            // Revert the approval claim so the request can be retried.
            let _ = sqlx::query(
                "UPDATE reopen_requests SET status='pending', reviewed_at=NULL WHERE id=$1",
            )
            .bind(id)
            .execute(&s.pool)
            .await;
            return Err(e);
        }
    };
    audit::log(
        &s.pool,
        u.id,
        "approved",
        "reopen_requests",
        id,
        Some(serde_json::to_value(&r).unwrap()),
        Some(serde_json::json!({"status": "approved"})),
    )
    .await;
    notifications::create(
        &s,
        r.user_id,
        "reopen_approved",
        "Wochenfreigabe genehmigt / Week reopen approved",
        &format!(
            "Ihre Woche ab {} wurde zur Bearbeitung freigegeben.",
            r.week_start
        ),
        Some("reopen_request"),
        Some(id),
    )
    .await;
    Ok(Json(
        serde_json::json!({ "ok": true, "entries_reopened": count }),
    ))
}

pub async fn reject(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let reason = b.reason.trim();
    if reason.is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    let r = load_pending(&s.pool, id).await?;
    if r.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    if !u.is_admin() && r.approver_id != u.id {
        return Err(AppError::Forbidden);
    }
    let claimed = sqlx::query(
        "UPDATE reopen_requests SET status='rejected', reviewed_at=CURRENT_TIMESTAMP, \
         rejection_reason=$2 WHERE id=$1 AND status='pending'",
    )
    .bind(id)
    .bind(reason)
    .execute(&s.pool)
    .await?
    .rows_affected();
    if claimed == 0 {
        return Err(AppError::Conflict(
            "Request was already resolved by someone else.".into(),
        ));
    }
    audit::log(
        &s.pool,
        u.id,
        "rejected",
        "reopen_requests",
        id,
        Some(serde_json::to_value(&r).unwrap()),
        Some(serde_json::json!({ "status": "rejected", "reason": reason })),
    )
    .await;
    notifications::create(
        &s,
        r.user_id,
        "reopen_rejected",
        "Wochenfreigabe abgelehnt / Week reopen rejected",
        &format!(
            "Ihre Anfrage zur Bearbeitung der Woche ab {} wurde abgelehnt: {}",
            r.week_start, reason
        ),
        Some("reopen_request"),
        Some(id),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}
