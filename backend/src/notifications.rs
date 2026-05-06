//! Persistent in-app notification center plus best-effort email sidecar.
//!
//! Notifications are immutable once created (only `is_read` flips).
//! Cleanup beyond 90 days happens in the background loop in `lib.rs`.

use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n::Language;
use crate::AppState;
use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

#[derive(Clone, Debug)]
pub struct NotificationSignal {
    pub user_id: i64,
}

pub type NotificationBroadcaster = broadcast::Sender<NotificationSignal>;

pub fn broadcaster() -> NotificationBroadcaster {
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

/// Insert a notification row. `email` is sent best-effort via SMTP if
/// configured. Both operations are non-fatal: failures are logged but not
/// propagated.
pub async fn create(
    state: &AppState,
    user_id: i64,
    kind: &str,
    title: &str,
    body: &str,
    reference_type: Option<&str>,
    reference_id: Option<i64>,
) {
    if let Err(e) = sqlx::query(
        "INSERT INTO notifications(user_id,kind,title,body,reference_type,reference_id) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(user_id)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(reference_type)
    .bind(reference_id)
    .execute(&state.pool)
    .await
    {
        tracing::warn!(target:"zerf::notifications", "insert failed: {e}");
        return;
    }
    let _ = state.notifications.send(NotificationSignal { user_id });
    // Resolve recipient email and dispatch SMTP best-effort.
    if let Ok(Some(email)) =
        sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id=$1 AND active=TRUE")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await
    {
        let smtp = crate::settings::load_smtp_config(&state.pool)
            .await
            .map(std::sync::Arc::new);
        crate::email::send_async(smtp, email, title.to_string(), body.to_string());
    }
}

pub async fn create_translated(
    state: &AppState,
    language: &Language,
    user_id: i64,
    kind: &str,
    title_key: &str,
    body_key: &str,
    params: Vec<(&str, String)>,
    reference_type: Option<&str>,
    reference_id: Option<i64>,
) {
    let title = crate::i18n::translate(language, title_key, &params);
    let body = crate::i18n::translate(language, body_key, &params);
    create(
        state,
        user_id,
        kind,
        &title,
        &body,
        reference_type,
        reference_id,
    )
    .await;
}

pub async fn list(State(app_state): State<AppState>, requester: User) -> AppResult<Json<Vec<Notification>>> {
    Ok(Json(
        sqlx::query_as::<_, Notification>(
            "SELECT id, user_id, kind, title, body, reference_type, reference_id, is_read, created_at FROM notifications WHERE user_id=$1 \
             ORDER BY created_at DESC LIMIT 100",
        )
        .bind(requester.id)
        .fetch_all(&app_state.pool)
        .await?,
    ))
}

pub async fn unread_count(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let unread_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id=$1 AND is_read=FALSE")
            .bind(requester.id)
            .fetch_one(&app_state.pool)
            .await?;
    Ok(Json(serde_json::json!({ "count": unread_count })))
}

pub async fn stream(
    State(app_state): State<AppState>,
    requester: User,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let requester_id = requester.id;
    let stream = BroadcastStream::new(app_state.notifications.subscribe()).filter_map(move |msg| {
        let should_refresh = match msg {
            Ok(signal) => signal.user_id == requester_id,
            Err(_) => true, // lagged — refresh to catch up
        };
        should_refresh.then_some(Ok(Event::default().event("notification").data(r#"{"type":"refresh"}"#)))
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    )
}

pub async fn mark_read(
    State(app_state): State<AppState>,
    requester: User,
    Path(notification_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let rows_updated = sqlx::query("UPDATE notifications SET is_read=TRUE WHERE id=$1 AND user_id=$2")
        .bind(notification_id)
        .bind(requester.id)
        .execute(&app_state.pool)
        .await?
        .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn mark_all_read(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let rows_updated =
        sqlx::query("UPDATE notifications SET is_read=TRUE WHERE user_id=$1 AND is_read=FALSE")
            .bind(requester.id)
            .execute(&app_state.pool)
            .await?
            .rows_affected();
    Ok(Json(serde_json::json!({ "ok": true, "count": rows_updated })))
}

pub async fn delete_all(State(app_state): State<AppState>, requester: User) -> AppResult<Json<serde_json::Value>> {
    let rows_deleted = sqlx::query("DELETE FROM notifications WHERE user_id=$1")
        .bind(requester.id)
        .execute(&app_state.pool)
        .await?
        .rows_affected();
    Ok(Json(serde_json::json!({ "ok": true, "count": rows_deleted })))
}

/// Trim notifications older than 90 days; called from the background loop.
pub async fn cleanup_old(pool: &crate::db::DatabasePool) {
    let _ = sqlx::query(
        "DELETE FROM notifications WHERE created_at < CURRENT_TIMESTAMP - INTERVAL '90 days'",
    )
    .execute(pool)
    .await;
}
