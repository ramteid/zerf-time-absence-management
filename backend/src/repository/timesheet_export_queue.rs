use crate::db::DatabasePool;
use crate::error::AppResult;

/// A single pending export queue entry: one employee, one month.
#[derive(sqlx::FromRow)]
pub struct ExportQueueEntry {
    pub user_id: i64,
    pub period: String, // "YYYY-MM"
    pub requires_start_date_review: bool,
}

#[derive(Clone)]
pub struct TimesheetExportQueueDb {
    pool: DatabasePool,
}

impl TimesheetExportQueueDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Insert one queue entry per user ID for the given period.
    /// Idempotent: duplicate (user_id, period) pairs keep any existing review flag.
    /// Uses a single UNNEST bulk INSERT to avoid N+1 round-trips.
    pub async fn populate(&self, period: &str, user_ids: &[i64]) -> AppResult<()> {
        self.populate_with_start_date_review(period, user_ids, false)
            .await
    }

    /// Insert queue entries and mark them as requiring start-date review.
    /// Existing rows keep the flag once it has been raised.
    pub async fn populate_requiring_start_date_review(
        &self,
        period: &str,
        user_ids: &[i64],
    ) -> AppResult<()> {
        self.populate_with_start_date_review(period, user_ids, true)
            .await
    }

    async fn populate_with_start_date_review(
        &self,
        period: &str,
        user_ids: &[i64],
        requires_start_date_review: bool,
    ) -> AppResult<()> {
        if user_ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO timesheet_export_queue (user_id, period, requires_start_date_review) \
             SELECT uid, $1, $3 FROM UNNEST($2::BIGINT[]) AS t(uid) \
             ON CONFLICT (user_id, period) DO UPDATE \
             SET requires_start_date_review = \
                 timesheet_export_queue.requires_start_date_review \
                 OR EXCLUDED.requires_start_date_review",
        )
        .bind(period)
        .bind(user_ids)
        .bind(requires_start_date_review)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return all pending entries ordered by period then user_id.
    pub async fn list_pending(&self) -> AppResult<Vec<ExportQueueEntry>> {
        Ok(sqlx::query_as(
            "SELECT user_id, period, requires_start_date_review \
             FROM timesheet_export_queue \
             ORDER BY period, user_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Remove a single queue entry (called after a successful upload).
    pub async fn delete_entry(&self, user_id: i64, period: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM timesheet_export_queue WHERE user_id=$1 AND period=$2")
            .bind(user_id)
            .bind(period)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
