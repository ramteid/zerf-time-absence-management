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
        let result = sqlx::query(
            "INSERT INTO notifications(user_id,kind,title,body,reference_type,reference_id) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(kind)
        .bind(title)
        .bind(body)
        .bind(reference_type)
        .bind(reference_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_for_user(&self, user_id: i64) -> AppResult<Vec<Notification>> {
        Ok(sqlx::query_as::<_, Notification>(
            "SELECT id, user_id, kind, title, body, reference_type, reference_id, is_read, \
             created_at FROM notifications WHERE user_id=$1 \
             ORDER BY created_at DESC LIMIT 100",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn count_unread(&self, user_id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM notifications WHERE user_id=$1 AND is_read=FALSE",
            )
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?,
        )
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
            sqlx::query(
                "UPDATE notifications SET is_read=TRUE WHERE user_id=$1 AND is_read=FALSE",
            )
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected(),
        )
    }

    pub async fn delete_all(&self, user_id: i64) -> AppResult<u64> {
        Ok(
            sqlx::query("DELETE FROM notifications WHERE user_id=$1")
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    /// Trim notifications older than 90 days (background cleanup).
    pub async fn cleanup_old(&self) {
        let _ = sqlx::query(
            "DELETE FROM notifications \
             WHERE created_at < CURRENT_TIMESTAMP - INTERVAL '90 days'",
        )
        .execute(&self.pool)
        .await;
    }

    /// Fetch the email of an active user (used to send notification emails).
    pub async fn get_user_email(&self, user_id: i64) -> Option<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT email FROM users WHERE id=$1 AND active=TRUE",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
    }
}
