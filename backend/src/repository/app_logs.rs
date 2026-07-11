use crate::db::DatabasePool;
use crate::error::AppResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;

/// Maximum number of rows kept in `app_logs`; older rows are pruned.
pub const APP_LOG_MAX_ROWS: i64 = 1000;
/// Rows older than this many days expire regardless of the row cap.
pub const APP_LOG_MAX_AGE_DAYS: i64 = 365;

#[derive(FromRow, Serialize)]
pub struct AppLogEntry {
    pub id: i64,
    pub level: String,
    pub message: String,
    pub target: String,
    pub fields: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AppLogDb {
    pool: DatabasePool,
}

impl AppLogDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Insert one captured warn/error log record.
    ///
    /// Returns the raw `sqlx::Error` instead of `AppError` deliberately: this
    /// method runs inside the log-capture writer task, and the
    /// `From<sqlx::Error> for AppError` conversion logs a capturable ERROR
    /// event (`zerf::db`) — routing through it would feed the writer's own
    /// failures back into the capture channel and loop forever while the
    /// database is down. The caller logs failures under the excluded
    /// `WRITER_TARGET` instead.
    pub async fn insert(
        &self,
        level: &str,
        message: &str,
        target: &str,
        fields: Option<serde_json::Value>,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO app_logs(level, message, target, fields, occurred_at) \
             VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(level)
        .bind(message)
        .bind(target)
        .bind(fields)
        .bind(occurred_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Enforce both storage bounds: keep at most `APP_LOG_MAX_ROWS` rows and
    /// expire rows older than `APP_LOG_MAX_AGE_DAYS` days.
    pub async fn prune(&self) -> AppResult<()> {
        sqlx::query(
            "DELETE FROM app_logs \
             WHERE occurred_at < CURRENT_TIMESTAMP - make_interval(days => $1::int) \
                OR id NOT IN (SELECT id FROM app_logs ORDER BY id DESC LIMIT $2)",
        )
        .bind(APP_LOG_MAX_AGE_DAYS)
        .bind(APP_LOG_MAX_ROWS)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// One page of log entries (newest first) plus the total row count, so the
    /// UI can render pagination without fetching everything.
    pub async fn list_page(&self, limit: i64, offset: i64) -> AppResult<(Vec<AppLogEntry>, i64)> {
        let entries = sqlx::query_as::<_, AppLogEntry>(
            "SELECT id, level, message, target, fields, occurred_at \
             FROM app_logs ORDER BY occurred_at DESC, id DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_logs")
            .fetch_one(&self.pool)
            .await?;
        Ok((entries, total))
    }
}
