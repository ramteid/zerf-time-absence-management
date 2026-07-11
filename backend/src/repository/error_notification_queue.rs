use crate::db::DatabasePool;
use crate::error::AppResult;
use sqlx::FromRow;

/// One queued technical-error event awaiting fan-out to opted-in admins.
#[derive(FromRow)]
pub struct ErrorNotificationEntry {
    pub id: i64,
    pub dedupe_key: Option<String>,
    pub title: String,
    pub body: Option<String>,
}

#[derive(Clone)]
pub struct ErrorNotificationQueueDb {
    pool: DatabasePool,
}

impl ErrorNotificationQueueDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Enqueue one error event. `source` is 'app' or 'backup' (diagnostics only).
    ///
    /// Returns the raw `sqlx::Error` instead of `AppError` deliberately: the
    /// log-capture writer calls this for every ERROR-level event, and the
    /// `From<sqlx::Error> for AppError` conversion logs a capturable ERROR
    /// event (`zerf::db`) — routing through it would turn an enqueue failure
    /// into a new captured error and loop forever while the database is down.
    /// Callers log failures under excluded targets instead.
    pub async fn enqueue(
        &self,
        dedupe_key: Option<&str>,
        title: &str,
        body: Option<&str>,
        source: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO error_notification_queue (dedupe_key, title, body, source) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(dedupe_key)
        .bind(title)
        .bind(body)
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Oldest-first batch of pending events for the worker to process.
    pub async fn list_pending(&self, limit: i64) -> AppResult<Vec<ErrorNotificationEntry>> {
        Ok(sqlx::query_as::<_, ErrorNotificationEntry>(
            "SELECT id, dedupe_key, title, body FROM error_notification_queue \
             ORDER BY id LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Remove a processed event so it is never retried.
    pub async fn delete_entry(&self, id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM error_notification_queue WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
