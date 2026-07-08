use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::repository::time_entries::{
    validate_entries_after_reopen, REOPENABLE_TIME_ENTRY_CONDITION,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};

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
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

const RR_SELECT: &str = "SELECT id, user_id, week_start, reviewed_by, status, \
     reviewed_at, rejection_reason, reason, created_at FROM reopen_requests";

/// Map an insert failure: the partial unique index on pending requests means a
/// unique violation is the expected "duplicate request" race; anything else
/// is a genuine database error and must surface as such, not be masked by a
/// misleading conflict message.
fn map_reopen_insert_error(context: &str, e: sqlx::Error) -> AppError {
    let is_unique_violation = e
        .as_database_error()
        .map(|db_err| db_err.kind() == sqlx::error::ErrorKind::UniqueViolation)
        .unwrap_or(false);
    if is_unique_violation {
        tracing::warn!(target:"zerf::reopen", "{context}: duplicate pending request: {e}");
        AppError::conflict("A pending request for this week already exists.")
    } else {
        tracing::warn!(target:"zerf::reopen", "{context} failed");
        // From<sqlx::Error> logs the details and hides them from the client.
        AppError::from(e)
    }
}

const ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL: &str = "\
    SELECT TRUE \
    FROM user_approvers ua \
    JOIN users subject ON subject.id = ua.user_id \
    JOIN users approver ON approver.id = ua.approver_id \
    WHERE ua.user_id = $1 AND ua.approver_id = $2 \
    AND subject.active=TRUE AND subject.role != 'admin' \
    AND approver.active=TRUE AND approver.role IN ('team_lead','admin') \
    FOR UPDATE OF ua";

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
            QueryBuilder::<Postgres>::new(format!("{RR_SELECT} WHERE id=$1"))
                .build_query_as::<ReopenRequest>()
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn list_mine(&self, user_id: i64) -> AppResult<Vec<ReopenRequest>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{RR_SELECT} WHERE user_id=$1 ORDER BY created_at DESC"
        ))
        .build_query_as::<ReopenRequest>()
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_pending_admin(&self) -> AppResult<Vec<ReopenRequest>> {
        // Exclude requests from users who have time tracking disabled or are
        // archived — their historical rows are kept immutably but must not
        // surface in any team or approval view.
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{RR_SELECT} WHERE status='pending' \
             AND user_id IN (SELECT id FROM users WHERE tracks_time=TRUE AND active=TRUE) \
             AND NOT EXISTS (\
                 SELECT 1 FROM time_entries te \
                 WHERE te.user_id = reopen_requests.user_id \
                 AND te.entry_date BETWEEN reopen_requests.week_start AND reopen_requests.week_start + 6 \
                 AND te.status = 'submitted'\
             ) \
             ORDER BY created_at"
        ))
        .build_query_as::<ReopenRequest>()
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_pending_for_lead(&self, lead_id: i64) -> AppResult<Vec<ReopenRequest>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{RR_SELECT} WHERE status='pending' \
             AND user_id IN (\
                 SELECT ua.user_id FROM user_approvers ua \
                 JOIN users u ON u.id = ua.user_id \
                 WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin' \
                 AND u.tracks_time=TRUE\
             ) \
             AND NOT EXISTS (\
                 SELECT 1 FROM time_entries te \
                 WHERE te.user_id = reopen_requests.user_id \
                 AND te.entry_date BETWEEN reopen_requests.week_start AND reopen_requests.week_start + 6 \
                 AND te.status = 'submitted'\
             ) \
             ORDER BY created_at"
        ))
        .build_query_as::<ReopenRequest>()
        .bind(lead_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn count_reopenable_entries(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        week_end: NaiveDate,
    ) -> AppResult<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM time_entries te \
             WHERE te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
             AND ({REOPENABLE_TIME_ENTRY_CONDITION})"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(week_start)
            .bind(week_end)
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn count_submitted_entries(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        week_end: NaiveDate,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries \
             WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 \
             AND status = 'submitted'",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(week_end)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Insert an auto-approved reopen only when every reopenable entry in the
    /// week is still submitted. The row locks close the race with approvers who
    /// may be approving the same week at the same time.
    pub async fn insert_auto_approved_if_all_reopenable_are_submitted(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        actor_id: i64,
        reason: &str,
    ) -> AppResult<Option<(i64, Vec<(i64, String)>)>> {
        let mut tx = self.pool.begin().await?;
        let week_end = week_start + chrono::Duration::days(6);
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        let sql = format!(
            "SELECT te.status FROM time_entries te \
             WHERE te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
             AND ({REOPENABLE_TIME_ENTRY_CONDITION}) \
             FOR UPDATE OF te"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        let statuses: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(week_start)
            .bind(week_end)
            .fetch_all(&mut *tx)
            .await?;
        if statuses.is_empty() {
            return Err(AppError::bad_request(
                "Cannot request edit - this week has no submitted, approved, or rejected entries.",
            ));
        }
        if statuses.iter().any(|status| status != "submitted") {
            tx.rollback().await?;
            return Ok(None);
        }

        let req_id: i64 = sqlx::query_scalar(
            "INSERT INTO reopen_requests(user_id, week_start, status, reviewed_by, reviewed_at, reason) \
             VALUES ($1,$2,'auto_approved',$3,CURRENT_TIMESTAMP,$4) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(actor_id)
        .bind(reason)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            map_reopen_insert_error("insert_auto_approved_if_all_reopenable_are_submitted", e)
        })?;
        let affected = Self::perform_reopen(&mut tx, user_id, week_start).await?;
        tx.commit().await?;
        Ok(Some((req_id, affected)))
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
        reason: &str,
    ) -> AppResult<(i64, DateTime<Utc>)> {
        sqlx::query_as::<_, (i64, DateTime<Utc>)>(
            "INSERT INTO reopen_requests(user_id, week_start, status, reason) \
             VALUES ($1,$2,'pending',$3) RETURNING id, created_at",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(reason)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| map_reopen_insert_error("insert_pending", e))
    }

    /// Insert a reopen request directly as 'auto_approved' and perform the
    /// actual reopen within the same transaction.
    /// Returns (request_id, vec of (entry_id, prev_status)).
    pub async fn insert_auto_approved(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        actor_id: i64,
        reason: &str,
    ) -> AppResult<(i64, Vec<(i64, String)>)> {
        let mut tx = self.pool.begin().await?;
        let req_id: i64 = sqlx::query_scalar(
            "INSERT INTO reopen_requests(user_id, week_start, status, reviewed_by, reviewed_at, reason) \
             VALUES ($1,$2,'auto_approved',$3,CURRENT_TIMESTAMP,$4) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(actor_id)
        .bind(reason)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| map_reopen_insert_error("insert_auto_approved", e))?;
        let affected = Self::perform_reopen(&mut tx, user_id, week_start).await?;
        tx.commit().await?;
        Ok((req_id, affected))
    }

    /// Set a pending reopen to 'approved' and reopen the week atomically.
    /// Returns (updated request, vec of (entry_id, prev_status)).
    pub async fn approve_with_access_check(
        &self,
        request_id: i64,
        reviewer_id: i64,
        reviewer_is_admin: bool,
    ) -> AppResult<(ReopenRequest, Vec<(i64, String)>)> {
        let mut tx = self.pool.begin().await?;
        let req: ReopenRequest =
            QueryBuilder::<Postgres>::new(format!("{RR_SELECT} WHERE id=$1 FOR UPDATE"))
                .build_query_as::<ReopenRequest>()
                .bind(request_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(AppError::NotFound)?;
        if req.status != "pending" {
            return Err(AppError::bad_request("Request is not pending."));
        }

        if !reviewer_is_admin {
            if req.user_id == reviewer_id {
                return Err(AppError::forbidden());
            }
            let is_assigned_approver: Option<bool> =
                sqlx::query_scalar(ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL)
                    .bind(req.user_id)
                    .bind(reviewer_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if is_assigned_approver.is_none() {
                return Err(AppError::forbidden());
            }
        }

        let affected = Self::perform_reopen(&mut tx, req.user_id, req.week_start).await?;
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
            return Err(AppError::conflict(
                "Reopen request was already resolved by someone else.",
            ));
        }
        tx.commit().await?;
        Ok((req, affected))
    }

    /// Reject a pending reopen request atomically with access checks.
    pub async fn reject_with_access_check(
        &self,
        request_id: i64,
        reviewer_id: i64,
        reviewer_is_admin: bool,
        reason: &str,
    ) -> AppResult<ReopenRequest> {
        let mut tx = self.pool.begin().await?;
        let before: ReopenRequest =
            QueryBuilder::<Postgres>::new(format!("{RR_SELECT} WHERE id=$1 FOR UPDATE"))
                .build_query_as::<ReopenRequest>()
                .bind(request_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(AppError::NotFound)?;
        if before.status != "pending" {
            return Err(AppError::bad_request("Request is not pending."));
        }

        if !reviewer_is_admin {
            if before.user_id == reviewer_id {
                return Err(AppError::forbidden());
            }
            let is_assigned_approver: Option<bool> =
                sqlx::query_scalar(ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL)
                    .bind(before.user_id)
                    .bind(reviewer_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if is_assigned_approver.is_none() {
                return Err(AppError::forbidden());
            }
        }
        let rows = sqlx::query(
            "UPDATE reopen_requests SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 \
             WHERE id=$3 AND status='pending'",
        )
        .bind(reviewer_id)
        .bind(reason)
        .bind(request_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows == 0 {
            return Err(AppError::conflict(
                "Request was already resolved by someone else.",
            ));
        }
        tx.commit().await?;
        Ok(before)
    }

    // ── Internal: perform the actual reopen within a transaction ──────────

    /// Reset every submitted, approved, or rejected entry in
    /// `week_start..week_start+6` back to draft.  Returns the list of
    /// (entry_id, previous_status) that were changed.
    pub async fn perform_reopen(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        subject_id: i64,
        week_start: NaiveDate,
    ) -> AppResult<Vec<(i64, String)>> {
        let week_end = week_start + chrono::Duration::days(6);
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(subject_id)
            .execute(&mut **tx)
            .await?;
        let sql = format!(
            "SELECT te.id, te.status FROM time_entries te \
             WHERE te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
             AND ({REOPENABLE_TIME_ENTRY_CONDITION}) \
             FOR UPDATE OF te"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        let affected: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(subject_id)
            .bind(week_start)
            .bind(week_end)
            .fetch_all(&mut **tx)
            .await?;
        if affected.is_empty() {
            return Err(AppError::bad_request(
                "Cannot request edit - this week has no submitted, approved, or rejected entries.",
            ));
        }
        let entry_ids: Vec<i64> = affected.iter().map(|(id, _)| *id).collect();

        validate_entries_after_reopen(&mut *tx, subject_id, &entry_ids).await?;

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

    /// Reject any pending reopen request for `user_id` on the given `week_start`.
    /// Called after a successful week submission: a pending reopen request on a
    /// week that now has submitted entries can no longer be decided by approvers
    /// (they filter it out via the NOT EXISTS submitted-entries guard), so it
    /// becomes a "zombie" that permanently blocks new reopen requests. Rejecting
    /// it here closes the state machine cleanly. Returns the id of the rejected
    /// request, or `None` if no pending request existed for that week.
    pub async fn reject_pending_for_week(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        reason: &str,
    ) -> AppResult<Option<i64>> {
        // Use user_id as the nominal reviewer so the audit trail is self-contained.
        // There is no "system" reviewer id; the employee's own id is the closest
        // proxy (the request originated from them and the submission superseded it).
        let id: Option<i64> = sqlx::query_scalar(
            "UPDATE reopen_requests \
             SET status='rejected', reviewed_by=$1, \
                 reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 \
             WHERE user_id=$1 AND week_start=$3 AND status='pending' \
             RETURNING id",
        )
        .bind(user_id)
        .bind(reason)
        .bind(week_start)
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    /// Reject all pending reopen requests owned by `user_id` within an existing
    /// transaction. Used during archiving to auto-reject the user's open requests.
    /// Returns the count of rejected requests.
    pub async fn reject_pending_for_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        reviewer_id: i64,
        reason: &str,
    ) -> AppResult<u64> {
        let rows = sqlx::query(
            "UPDATE reopen_requests SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 \
             WHERE user_id=$3 AND status='pending'",
        )
        .bind(reviewer_id)
        .bind(reason)
        .bind(user_id)
        .execute(tx)
        .await?
        .rows_affected();
        Ok(rows)
    }

    /// Prune resolved reopen requests older than `retention_days` days.
    /// Pending requests are never deleted — they are still actionable.
    pub async fn cleanup_old(&self, retention_days: i64) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days);
        let _ = sqlx::query(
            "DELETE FROM reopen_requests \
             WHERE status IN ('approved', 'rejected', 'auto_approved') \
             AND created_at < $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await;
    }

    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }
}
