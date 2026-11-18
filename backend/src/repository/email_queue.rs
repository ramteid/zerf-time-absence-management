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
    /// Notification kind that produced this mail (empty for rows queued
    /// before the column existed). The worker needs it to tell an admin
    /// alert apart from ordinary mail when a delivery fails.
    pub kind: String,
    pub subject: String,
    pub body_text: String,
    /// Where an answer to this mail should go (empty = no Reply-To header).
    pub reply_to_address: String,
    pub reply_to_name: String,
}

/// What one failed delivery attempt did to its row.
pub struct FailedAttempt {
    /// Failed attempts of this mail so far, the one just recorded included.
    pub attempts: i32,
    /// Whether this was the last attempt the mail gets. The row is then marked
    /// `given_up_at` and never offered for delivery again.
    pub given_up: bool,
}

/// A person as shown in a mail header: address plus display name (the name may
/// be empty).
pub struct EmailContact {
    pub address: String,
    pub name: String,
}

#[derive(Clone)]
pub struct EmailQueueDb {
    pool: DatabasePool,
}

impl EmailQueueDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Queue one email for delivery. `kind` is the notification kind that
    /// produced it (see `services::notifications::Outgoing`). `reply_to` is the
    /// person answers should reach instead of the system sender, if any.
    pub async fn enqueue(
        &self,
        to_address: &str,
        to_name: &str,
        kind: &str,
        subject: &str,
        body_text: &str,
        reply_to: Option<&EmailContact>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO email_queue \
             (to_address, to_name, kind, subject, body_text, reply_to_address, reply_to_name) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(to_address)
        .bind(to_name)
        .bind(kind)
        .bind(subject)
        .bind(body_text)
        .bind(reply_to.map_or("", |contact| contact.address.as_str()))
        .bind(reply_to.map_or("", |contact| contact.name.as_str()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Batch of emails the worker may attempt right now: never-yet-attempted
    /// rows first (oldest-created first among those, i.e. "der Reihe nach"),
    /// then previously-failed rows least-recently-attempted first.
    ///
    /// A row that failed is not offered again before its waiting time has
    /// passed. `retry_delays_seconds[n - 1]` is the wait after the n-th failed
    /// attempt; a row with more failures than the list has entries keeps the
    /// last (longest) wait, so a row that failed more often than the current
    /// schedule expects (queued under an older one) still gets its next
    /// attempt instead of being stuck. Rows marked `given_up_at` are never
    /// offered. The schedule itself is the worker's decision and is passed in.
    ///
    /// The order is deliberately *not* a strict `ORDER BY id`: with the
    /// circuit breaker granting only one attempt per cooldown window while
    /// it's open, a strict creation-order queue would let a single
    /// undeliverable message (e.g. a mistyped recipient address) sit at the
    /// head and consume every retry slot, starving every healthy message
    /// queued behind it. Sorting by `last_attempt_at` demotes a row the moment
    /// it fails, so fresh messages and other stuck messages get their turn.
    pub async fn list_due(
        &self,
        limit: i64,
        retry_delays_seconds: &[i64],
    ) -> AppResult<Vec<EmailQueueEntry>> {
        Ok(sqlx::query_as::<_, EmailQueueEntry>(
            "SELECT id, to_address, to_name, kind, subject, body_text, \
             reply_to_address, reply_to_name FROM email_queue \
             WHERE given_up_at IS NULL \
               AND (last_attempt_at IS NULL \
                    OR last_attempt_at \
                       + ($2::bigint[])[LEAST(attempts, cardinality($2::bigint[]))] \
                         * INTERVAL '1 second' <= CURRENT_TIMESTAMP) \
             ORDER BY last_attempt_at ASC NULLS FIRST, id ASC LIMIT $1",
        )
        .bind(limit)
        .bind(retry_delays_seconds)
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

    /// Record a failed delivery attempt. When it was the `max_attempts`-th
    /// (or a later one), the same statement marks the row `given_up_at`, so the
    /// failure and the decision to stop can never end up recorded apart.
    pub async fn record_failure(
        &self,
        id: i64,
        error: &str,
        max_attempts: i32,
    ) -> AppResult<FailedAttempt> {
        let (attempts, given_up): (i32, bool) = sqlx::query_as(
            "UPDATE email_queue SET attempts = attempts + 1, \
             last_attempt_at = CURRENT_TIMESTAMP, last_error = $2, \
             given_up_at = CASE WHEN attempts + 1 >= $3 THEN CURRENT_TIMESTAMP END \
             WHERE id = $1 RETURNING attempts, given_up_at IS NOT NULL",
        )
        .bind(id)
        .bind(error)
        .bind(max_attempts)
        .fetch_one(&self.pool)
        .await?;
        Ok(FailedAttempt { attempts, given_up })
    }

    /// Delete rows that were given up more than `days` days ago (background
    /// cleanup). A given-up mail is kept for a while so it can be looked up,
    /// but its text can hold a temporary password or a reset link, so it must
    /// not stay forever. Rows still waiting for delivery are never touched.
    pub async fn delete_given_up_older_than(&self, days: i32) -> AppResult<u64> {
        Ok(sqlx::query(
            "DELETE FROM email_queue \
             WHERE given_up_at < CURRENT_TIMESTAMP - make_interval(days => $1)",
        )
        .bind(days)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    /// Number of rows in the queue, given-up ones included, for tests and
    /// diagnostics.
    pub async fn count(&self) -> AppResult<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM email_queue")
            .fetch_one(&self.pool)
            .await?)
    }
}
