use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::repository::time_entries::{
    validate_entries_after_reopen, TimeEntry, TimeEntryDb,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

#[derive(sqlx::FromRow, Serialize)]
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

#[derive(sqlx::FromRow)]
struct OpenChangeRequestForReopen {
    id: i64,
    time_entry_id: i64,
    new_date: Option<NaiveDate>,
    new_start_time: Option<String>,
    new_end_time: Option<String>,
    new_category_id: Option<i64>,
    new_comment: Option<String>,
}

const RR_SELECT: &str = "SELECT id, user_id, week_start, reviewed_by, status, \
     reviewed_at, rejection_reason, created_at FROM reopen_requests";

#[derive(Clone)]
pub struct ReopenRequestDb {
    pool: DatabasePool,
}

impl ReopenRequestDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    // ── Queries ────────────────────────────────────────────────────────────

    pub async fn find_by_id(&self, id: i64) -> AppResult<ReopenRequest> {
        Ok(
            sqlx::query_as::<_, ReopenRequest>(&format!("{RR_SELECT} WHERE id=$1"))
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn list_mine(&self, user_id: i64) -> AppResult<Vec<ReopenRequest>> {
        Ok(sqlx::query_as::<_, ReopenRequest>(&format!(
            "{RR_SELECT} WHERE user_id=$1 ORDER BY created_at DESC"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_pending_admin(&self) -> AppResult<Vec<ReopenRequest>> {
        Ok(sqlx::query_as::<_, ReopenRequest>(&format!(
            "{RR_SELECT} WHERE status='pending' ORDER BY created_at"
        ))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_pending_for_lead(&self, lead_id: i64) -> AppResult<Vec<ReopenRequest>> {
        Ok(sqlx::query_as::<_, ReopenRequest>(&format!(
            "{RR_SELECT} WHERE status='pending' \
             AND user_id IN (\
                 SELECT ua.user_id FROM user_approvers ua \
                 JOIN users u ON u.id = ua.user_id \
                 WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin'\
             ) ORDER BY created_at"
        ))
        .bind(lead_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn count_non_draft_entries(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        week_end: NaiveDate,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries \
             WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 \
             AND status IN ('submitted','approved','rejected')",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(week_end)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn find_pending_request_id(
        &self,
        user_id: i64,
        week_start: NaiveDate,
    ) -> AppResult<Option<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM reopen_requests \
             WHERE user_id=$1 AND week_start=$2 AND status='pending'",
        )
        .bind(user_id)
        .bind(week_start)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn get_user_full_name(&self, user_id: i64) -> AppResult<String> {
        let (first, last): (String, String) =
            sqlx::query_as("SELECT first_name, last_name FROM users WHERE id=$1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(format!("{first} {last}"))
    }

    // ── Mutations ──────────────────────────────────────────────────────────

    /// Insert a pending reopen request. Returns (id, created_at).
    /// `reviewed_by` is left NULL per the DB constraint (pending requests have no reviewer yet).
    pub async fn insert_pending(
        &self,
        user_id: i64,
        week_start: NaiveDate,
    ) -> AppResult<(i64, DateTime<Utc>)> {
        sqlx::query_as::<_, (i64, DateTime<Utc>)>(
            "INSERT INTO reopen_requests(user_id, week_start, status) \
             VALUES ($1,$2,'pending') RETURNING id, created_at",
        )
        .bind(user_id)
        .bind(week_start)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            tracing::warn!(target:"zerf::reopen", "insert_pending failed: {e}");
            AppError::Conflict("A pending request for this week already exists.".into())
        })
    }

    /// Insert a reopen request directly as 'auto_approved' and perform the
    /// actual reopen within the same transaction.
    /// Returns (request_id, vec of (entry_id, prev_status)).
    pub async fn insert_auto_approved(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        actor_id: i64,
    ) -> AppResult<(i64, Vec<(i64, String)>)> {
        let mut tx = self.pool.begin().await?;
        let req_id: i64 = sqlx::query_scalar(
            "INSERT INTO reopen_requests(user_id, week_start, status, reviewed_by, reviewed_at) \
             VALUES ($1,$2,'auto_approved',$3,CURRENT_TIMESTAMP) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(actor_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            tracing::warn!(target:"zerf::reopen", "insert_auto_approved failed: {e}");
            AppError::Conflict("A pending request for this week already exists.".into())
        })?;
        let affected = Self::perform_reopen(&mut tx, actor_id, user_id, week_start).await?;
        tx.commit().await?;
        Ok((req_id, affected))
    }

    /// Set a pending reopen to 'approved' and reopen the week atomically.
    /// Returns (updated request, vec of (entry_id, prev_status)).
    pub async fn approve(
        &self,
        request_id: i64,
        reviewer_id: i64,
    ) -> AppResult<(ReopenRequest, Vec<(i64, String)>)> {
        let mut tx = self.pool.begin().await?;
        let req: ReopenRequest = sqlx::query_as::<_, ReopenRequest>(&format!(
            "{RR_SELECT} WHERE id=$1 AND status='pending' FOR UPDATE"
        ))
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::Conflict("Reopen request is no longer pending.".into()))?;

        let affected =
            Self::perform_reopen(&mut tx, reviewer_id, req.user_id, req.week_start).await?;
        let rows = sqlx::query(
            "UPDATE reopen_requests SET status='approved', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP \
             WHERE id=$2 AND status='pending'",
        )
        .bind(reviewer_id)
        .bind(request_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows == 0 {
            return Err(AppError::Conflict(
                "Reopen request was already resolved by someone else.".into(),
            ));
        }
        tx.commit().await?;
        let updated = self.find_by_id(request_id).await?;
        Ok((updated, affected))
    }

    /// Reject a pending reopen request (optimistic locking).
    pub async fn reject(
        &self,
        request_id: i64,
        reviewer_id: i64,
        reason: &str,
    ) -> AppResult<ReopenRequest> {
        let before: ReopenRequest =
            sqlx::query_as::<_, ReopenRequest>(&format!("{RR_SELECT} WHERE id=$1"))
                .bind(request_id)
                .fetch_one(&self.pool)
                .await?;
        if before.status != "pending" {
            return Err(AppError::BadRequest(
                "Only pending reopen requests can be rejected.".into(),
            ));
        }
        let rows = sqlx::query(
            "UPDATE reopen_requests SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 \
             WHERE id=$3 AND status='pending'",
        )
        .bind(reviewer_id)
        .bind(reason)
        .bind(request_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if rows == 0 {
            return Err(AppError::Conflict(
                "Reopen request was already resolved by someone else.".into(),
            ));
        }
        Ok(before)
    }

    // ── Internal: perform the actual reopen within a transaction ──────────

    /// Apply open change requests for all submitted, approved, or rejected entries in
    /// `week_start..week_start+6`, then reset those entries to draft.
    /// Returns the list of (entry_id, previous_status) that were changed.
    pub async fn perform_reopen(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        actor_id: i64,
        subject_id: i64,
        week_start: NaiveDate,
    ) -> AppResult<Vec<(i64, String)>> {
        let week_end = week_start + chrono::Duration::days(6);
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(subject_id)
            .execute(&mut **tx)
            .await?;
        let affected: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, status FROM time_entries \
             WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 \
             AND status IN ('submitted','approved','rejected') \
             FOR UPDATE",
        )
        .bind(subject_id)
        .bind(week_start)
        .bind(week_end)
        .fetch_all(&mut **tx)
        .await?;
        if affected.is_empty() {
            return Err(AppError::BadRequest(
                "Cannot request edit - this week has no submitted, approved, or rejected entries."
                    .into(),
            ));
        }
        let entry_ids: Vec<i64> = affected.iter().map(|(id, _)| *id).collect();
        let open_change_requests: Vec<OpenChangeRequestForReopen> = sqlx::query_as(
            "SELECT id, time_entry_id, new_date, new_start_time, new_end_time, \
                    new_category_id, new_comment \
             FROM change_requests \
             WHERE status='open' AND time_entry_id = ANY($1) \
             ORDER BY id FOR UPDATE",
        )
        .bind(&entry_ids)
        .fetch_all(&mut **tx)
        .await?;
        let mut applied_change_request_ids = Vec::new();
        for change_request in open_change_requests {
            let current_entry: TimeEntry = sqlx::query_as(
                "SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, \
                        submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at \
                 FROM time_entries WHERE id=$1 FOR UPDATE",
            )
            .bind(change_request.time_entry_id)
            .fetch_one(&mut **tx)
            .await?;
            TimeEntryDb::apply_change_request_tx(
                tx,
                change_request.time_entry_id,
                &current_entry.status,
                change_request.new_date,
                change_request.new_start_time.as_deref(),
                change_request.new_end_time.as_deref(),
                change_request.new_category_id,
                change_request.new_comment.as_deref(),
            )
            .await?;
            applied_change_request_ids.push(change_request.id);
        }

        validate_entries_after_reopen(&mut **tx, subject_id, &entry_ids).await?;

        if !applied_change_request_ids.is_empty() {
            let rows = sqlx::query(
                "UPDATE change_requests \
                 SET status='approved', \
                     reviewed_by=$1, \
                     reviewed_at=CURRENT_TIMESTAMP, \
                     rejection_reason=NULL \
                 WHERE status='open' AND id = ANY($2)",
            )
            .bind(if actor_id == subject_id {
                None::<i64>
            } else {
                Some(actor_id)
            })
            .bind(&applied_change_request_ids)
            .execute(&mut **tx)
            .await?
            .rows_affected();
            if rows != applied_change_request_ids.len() as u64 {
                return Err(AppError::Conflict(
                    "Change request was already resolved by someone else.".into(),
                ));
            }
        }
        // Reset all affected entries to draft.  We filter by their original IDs
        // (not by date range) because the CR-apply step above may have moved some
        // entries to a date outside the original week; a date-range filter would
        // silently miss those and leave them in a non-draft status.
        sqlx::query(
            "UPDATE time_entries \
             SET status='draft', submitted_at=NULL, reviewed_by=NULL, \
                 reviewed_at=NULL, rejection_reason=NULL, updated_at=CURRENT_TIMESTAMP \
             WHERE id = ANY($1)",
        )
        .bind(&entry_ids)
        .execute(&mut **tx)
        .await?;
        Ok(affected)
    }

    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }
}
