use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

#[derive(sqlx::FromRow, Serialize)]
pub struct ChangeRequest {
    pub id: i64,
    pub time_entry_id: i64,
    pub user_id: i64,
    pub new_date: Option<NaiveDate>,
    pub new_start_time: Option<String>,
    pub new_end_time: Option<String>,
    pub new_category_id: Option<i64>,
    pub new_comment: Option<String>,
    pub reason: String,
    pub status: String,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

const CR_SELECT: &str =
    "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, \
     new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, \
     rejection_reason, created_at FROM change_requests";

#[derive(Clone)]
pub struct ChangeRequestDb {
    pool: DatabasePool,
}

impl ChangeRequestDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    // ── Queries ────────────────────────────────────────────────────────────

    pub async fn list_for_user(&self, user_id: i64) -> AppResult<Vec<ChangeRequest>> {
        Ok(sqlx::query_as::<_, ChangeRequest>(&format!(
            "{CR_SELECT} WHERE user_id=$1 ORDER BY created_at DESC"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_open_all(&self) -> AppResult<Vec<ChangeRequest>> {
        Ok(sqlx::query_as::<_, ChangeRequest>(&format!(
            "{CR_SELECT} WHERE status='open' ORDER BY created_at"
        ))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_open_for_lead(&self, lead_id: i64) -> AppResult<Vec<ChangeRequest>> {
        Ok(sqlx::query_as::<_, ChangeRequest>(&format!(
            "{CR_SELECT} WHERE status='open' \
             AND user_id IN (\
               SELECT ua.user_id FROM user_approvers ua \
               JOIN users u ON u.id = ua.user_id \
               WHERE ua.approver_id=$1 AND u.role!='admin'\
             ) ORDER BY created_at"
        ))
        .bind(lead_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Load the relevant fields of a time entry for CR creation/approval checks.
    pub async fn get_entry_info(
        &self,
        time_entry_id: i64,
    ) -> AppResult<Option<(i64, String, NaiveDate, String, String, i64, Option<String>)>> {
        Ok(sqlx::query_as::<_, (i64, String, NaiveDate, String, String, i64, Option<String>)>(
            "SELECT user_id, status, entry_date, start_time, end_time, \
             category_id, comment FROM time_entries WHERE id=$1",
        )
        .bind(time_entry_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn get_entry_date(&self, time_entry_id: i64) -> AppResult<Option<NaiveDate>> {
        Ok(
            sqlx::query_scalar("SELECT entry_date FROM time_entries WHERE id=$1")
                .bind(time_entry_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn check_category_active(&self, category_id: i64) -> AppResult<Option<bool>> {
        Ok(sqlx::query_scalar("SELECT active FROM categories WHERE id=$1")
            .bind(category_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    // ── Mutations ──────────────────────────────────────────────────────────

    /// Insert a new change request, guarded by an advisory lock on the entry.
    /// Returns the new CR row.
    pub async fn create(
        &self,
        time_entry_id: i64,
        user_id: i64,
        new_date: Option<NaiveDate>,
        new_start_time: Option<&str>,
        new_end_time: Option<&str>,
        new_category_id: Option<i64>,
        new_comment: Option<&str>,
        reason: &str,
    ) -> AppResult<ChangeRequest> {
        let mut tx = self.pool.begin().await?;
        // Advisory lock in namespace 2 (separate from user-level locks).
        sqlx::query("SELECT pg_advisory_xact_lock(2, $1::int)")
            .bind(time_entry_id)
            .execute(&mut *tx)
            .await?;
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM change_requests WHERE time_entry_id=$1 AND status='open'",
        )
        .bind(time_entry_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = existing {
            return Err(AppError::Conflict(format!(
                "An open change request already exists for this entry (id {id})."
            )));
        }
        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO change_requests(time_entry_id, user_id, new_date, new_start_time, \
             new_end_time, new_category_id, new_comment, reason) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
        )
        .bind(time_entry_id)
        .bind(user_id)
        .bind(new_date)
        .bind(new_start_time)
        .bind(new_end_time)
        .bind(new_category_id)
        .bind(new_comment)
        .bind(reason)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(sqlx::query_as::<_, ChangeRequest>(&format!("{CR_SELECT} WHERE id=$1"))
            .bind(new_id)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Fetch an open CR with a row lock for approval/rejection.
    pub async fn fetch_open_for_update(
        tx: &mut sqlx::PgConnection,
        cr_id: i64,
    ) -> AppResult<ChangeRequest> {
        sqlx::query_as::<_, ChangeRequest>(&format!(
            "{CR_SELECT} WHERE id=$1 AND status='open' FOR UPDATE"
        ))
        .bind(cr_id)
        .fetch_optional(tx)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("Change request was already resolved by someone else.".into())
        })
    }

    pub async fn fetch_open(
        tx: &mut sqlx::PgConnection,
        cr_id: i64,
    ) -> AppResult<ChangeRequest> {
        sqlx::query_as::<_, ChangeRequest>(&format!(
            "{CR_SELECT} WHERE id=$1 AND status='open'"
        ))
        .bind(cr_id)
        .fetch_optional(tx)
        .await?
        .ok_or_else(|| AppError::BadRequest("Change request is not open.".into()))
    }

    /// Optimistically set CR to 'approved'. Returns rows affected.
    pub async fn set_approved_tx(
        tx: &mut sqlx::PgConnection,
        cr_id: i64,
        reviewer_id: i64,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE change_requests SET status='approved', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='open'",
        )
        .bind(reviewer_id)
        .bind(cr_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    /// Optimistically set CR to 'rejected'. Returns rows affected.
    pub async fn set_rejected_tx(
        tx: &mut sqlx::PgConnection,
        cr_id: i64,
        reviewer_id: i64,
        reason: &str,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE change_requests SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 \
             WHERE id=$3 AND status='open'",
        )
        .bind(reviewer_id)
        .bind(reason)
        .bind(cr_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    /// Check that `subject_id` is a non-admin direct report of `approver_id`.
    pub async fn is_direct_report_for_update(
        tx: &mut sqlx::PgConnection,
        subject_id: i64,
        approver_id: i64,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, Option<bool>>(
            "SELECT TRUE FROM users u \
             WHERE u.id=$1 AND u.role!='admin' \
             AND EXISTS (SELECT 1 FROM user_approvers ua WHERE ua.user_id=$1 AND ua.approver_id=$2) \
             FOR UPDATE",
        )
        .bind(subject_id)
        .bind(approver_id)
        .fetch_optional(tx)
        .await?
        .flatten()
        .is_some())
    }

    /// Acquire a per-user advisory lock (to serialize entry writes during CR approval).
    pub async fn lock_user_tx(tx: &mut sqlx::PgConnection, user_id: i64) -> AppResult<()> {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(user_id)
            .execute(tx)
            .await?;
        Ok(())
    }

    /// Begin a transaction on this pool.
    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }
}
