use crate::db::DatabasePool;
use crate::error::AppResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub struct NotificationSignal {
    pub user_id: i64,
}

pub type NotificationBroadcaster = broadcast::Sender<NotificationSignal>;

pub fn new_broadcaster() -> NotificationBroadcaster {
    let (tx, _) = broadcast::channel(256);
    tx
}

#[derive(FromRow, Serialize)]
pub struct Notification {
    pub id: i64,
    pub user_id: i64,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub reference_type: Option<String>,
    pub reference_id: Option<i64>,
    pub is_read: bool,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct NotificationDb {
    pool: DatabasePool,
    broadcaster: NotificationBroadcaster,
}

impl NotificationDb {
    pub fn new(pool: DatabasePool, broadcaster: NotificationBroadcaster) -> Self {
        Self { pool, broadcaster }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NotificationSignal> {
        self.broadcaster.subscribe()
    }

    pub fn broadcast(&self, user_id: i64) {
        let _ = self.broadcaster.send(NotificationSignal { user_id });
    }

    /// Insert a notification and broadcast to the user's SSE stream.
    pub async fn insert(
        &self,
        user_id: i64,
        kind: &str,
        title: &str,
        body: &str,
        reference_type: Option<&str>,
        reference_id: Option<i64>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO notifications(user_id,kind,title,body,reference_type,reference_id) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(user_id)
        .bind(kind)
        .bind(title)
        .bind(body)
        .bind(reference_type)
        .bind(reference_id)
        .execute(&self.pool)
        .await?;
        self.broadcast(user_id);
        Ok(())
    }

    /// Insert with ON CONFLICT DO NOTHING; returns `true` when the row was
    /// actually inserted (idempotency guard for submission reminders).
    pub async fn insert_idempotent(
        &self,
        user_id: i64,
        kind: &str,
        title: &str,
        body: &str,
        reference_type: Option<&str>,
        reference_id: Option<i64>,
    ) -> AppResult<bool> {
        self.insert_idempotent_with_dedupe_key(
            user_id,
            kind,
            title,
            body,
            reference_type,
            reference_id,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_idempotent_with_dedupe_key(
        &self,
        user_id: i64,
        kind: &str,
        title: &str,
        body: &str,
        reference_type: Option<&str>,
        reference_id: Option<i64>,
        dedupe_key: Option<&str>,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            "INSERT INTO notifications(user_id,kind,title,body,reference_type,reference_id,dedupe_key) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(kind)
        .bind(title)
        .bind(body)
        .bind(reference_type)
        .bind(reference_id)
        .bind(dedupe_key)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_for_user(&self, user_id: i64) -> AppResult<Vec<Notification>> {
        Ok(sqlx::query_as::<_, Notification>(
            "SELECT id, user_id, kind, title, body, reference_type, reference_id, is_read, \
             pinned, created_at FROM notifications WHERE user_id=$1 \
             ORDER BY (pinned AND NOT is_read) DESC, created_at DESC LIMIT 100",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Upsert a pinned system-error notification for one user, deduplicated by
    /// (user_id, kind, dedupe_key).
    ///
    /// Behaviour:
    ///  - Not exists → INSERT (unread, pinned).
    ///  - Exists and is_read=FALSE (already unread) → DO NOTHING (no re-alert).
    ///  - Exists and is_read=TRUE (admin had dismissed it) → UPDATE: mark unread
    ///    again and refresh created_at so it floats back to the top.
    ///
    /// Returns `true` when a row was inserted or re-alerted (caller may want to
    /// send an email); `false` when the notification was already unread.
    pub async fn upsert_system_error(
        &self,
        user_id: i64,
        kind: &str,
        dedupe_key: &str,
        title: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            "INSERT INTO notifications \
               (user_id, kind, title, body, dedupe_key, pinned, is_read) \
             VALUES ($1, $2, $3, NULL, $4, TRUE, FALSE) \
             ON CONFLICT (user_id, kind, dedupe_key) \
             WHERE dedupe_key IS NOT NULL \
             DO UPDATE SET \
               title      = EXCLUDED.title, \
               pinned     = TRUE, \
               is_read    = FALSE, \
               created_at = NOW() \
             WHERE notifications.is_read = TRUE",
        )
        .bind(user_id)
        .bind(kind)
        .bind(title)
        .bind(dedupe_key)
        .execute(&self.pool)
        .await?;

        let changed = result.rows_affected() > 0;
        if changed {
            self.broadcast(user_id);
        }
        Ok(changed)
    }

    /// Return all distinct (kind, dedupe_key, representative title) for unread
    /// pinned notifications.  Used by the system-alerts background task to
    /// decide whether to send a throttled alert email.
    pub async fn list_unread_system_error_classes(
        &self,
    ) -> AppResult<Vec<(String, String, String)>> {
        Ok(sqlx::query_as::<_, (String, String, String)>(
            "SELECT kind, dedupe_key, MAX(title) \
             FROM notifications \
             WHERE pinned = TRUE AND is_read = FALSE AND dedupe_key IS NOT NULL \
             GROUP BY kind, dedupe_key",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn count_unread(&self, user_id: i64) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id=$1 AND is_read=FALSE",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Returns rows affected (0 if not found).
    pub async fn mark_read(&self, id: i64, user_id: i64) -> AppResult<u64> {
        Ok(
            sqlx::query("UPDATE notifications SET is_read=TRUE WHERE id=$1 AND user_id=$2")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    pub async fn mark_all_read(&self, user_id: i64) -> AppResult<u64> {
        Ok(
            sqlx::query("UPDATE notifications SET is_read=TRUE WHERE user_id=$1 AND is_read=FALSE")
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    pub async fn delete_all(&self, user_id: i64) -> AppResult<u64> {
        Ok(sqlx::query("DELETE FROM notifications WHERE user_id=$1")
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    /// Trim notifications older than 90 days (background cleanup).
    /// Pinned unread notifications are excluded — they represent active system
    /// alerts that must not disappear silently until an admin acknowledges them.
    pub async fn cleanup_old(&self) {
        let _ = sqlx::query(
            "DELETE FROM notifications \
             WHERE created_at < CURRENT_TIMESTAMP - INTERVAL '90 days' \
             AND NOT (pinned = TRUE AND is_read = FALSE)",
        )
        .execute(&self.pool)
        .await;
    }

    /// Fetch the email and display name of an active user (used to send
    /// notification emails). Returns `(email, first_name, last_name)`.
    pub async fn get_user_email(&self, user_id: i64) -> Option<(String, String, String)> {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT email, first_name, last_name FROM users WHERE id=$1 AND active=TRUE",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
    }
}
