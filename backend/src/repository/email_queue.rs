use crate::db::DatabasePool;
use crate::error::AppResult;
use sqlx::FromRow;

/// One outbound email awaiting delivery. Subject and body are already fully
/// rendered plain text (i18n, footer, etc. all resolved at enqueue time) —
/// nothing is re-rendered when the row is eventually sent.
#[derive(FromRow)]
pub struct EmailQueueEntry {
    pub id: i64,
    pub to_address: String,
    pub to_name: String,
    pub subject: String,
    pub body_text: String,
}

#[derive(Clone)]
pub struct EmailQueueDb {
    pool: DatabasePool,
}

impl EmailQueueDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Queue one email for delivery.
    pub async fn enqueue(
        &self,
        to_address: &str,
        to_name: &str,
        subject: &str,
        body_text: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO email_queue (to_address, to_name, subject, body_text) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(to_address)
        .bind(to_name)
        .bind(subject)
        .bind(body_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Batch of pending emails for the worker to process: never-yet-attempted
    /// rows first (oldest-created first among those, i.e. "der Reihe nach"),
    /// then previously-failed rows least-recently-attempted first.
    ///
    /// This is deliberately *not* a strict `ORDER BY id`: with the circuit
    /// breaker granting only one attempt per cooldown window while it's open,
    /// a strict creation-order queue would let a single permanently
    /// undeliverable message (e.g. a mistyped recipient address) sit at the
    /// head forever, since it is never deleted — it would keep consuming
    /// every future retry slot and starve every healthy message queued
    /// behind it. Sorting by `last_attempt_at` demotes a row the moment it
    /// fails, so fresh messages and other stuck messages get their turn.
    pub async fn list_pending(&self, limit: i64) -> AppResult<Vec<EmailQueueEntry>> {
        Ok(sqlx::query_as::<_, EmailQueueEntry>(
            "SELECT id, to_address, to_name, subject, body_text FROM email_queue \
             ORDER BY last_attempt_at ASC NULLS FIRST, id ASC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Remove an email once the SMTP server confirmed it accepted the message.
    pub async fn delete_entry(&self, id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM email_queue WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Record a failed delivery attempt and return the new attempt count.
    pub async fn record_failure(&self, id: i64, error: &str) -> AppResult<i32> {
        Ok(sqlx::query_scalar(
            "UPDATE email_queue SET attempts = attempts + 1, \
             last_attempt_at = CURRENT_TIMESTAMP, last_error = $2 \
             WHERE id = $1 RETURNING attempts",
        )
        .bind(id)
        .bind(error)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Number of queued rows, for tests and diagnostics.
    pub async fn count(&self) -> AppResult<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM email_queue")
            .fetch_one(&self.pool)
            .await?)
    }
}
