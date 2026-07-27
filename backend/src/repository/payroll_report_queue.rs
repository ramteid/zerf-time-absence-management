use crate::db::DatabasePool;
use crate::error::AppResult;

/// Queue of months whose payroll report PDF still has to be emailed.
///
/// Unlike the timesheet export queue this is keyed by period only: the payroll
/// report is one document per month covering every employee, so there is
/// nothing to track per user.
#[derive(Clone)]
pub struct PayrollReportQueueDb {
    pool: DatabasePool,
}

impl PayrollReportQueueDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Queue one period. Idempotent: re-queueing an already pending period is
    /// a no-op, so repeated runs never create duplicates.
    pub async fn enqueue(&self, period: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO payroll_report_queue (period) VALUES ($1) ON CONFLICT (period) DO NOTHING",
        )
        .bind(period)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// All queued periods, oldest first.
    pub async fn list_pending(&self) -> AppResult<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT period FROM payroll_report_queue ORDER BY period")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Remove a period after its report was sent successfully.
    pub async fn delete_entry(&self, period: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM payroll_report_queue WHERE period=$1")
            .bind(period)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
