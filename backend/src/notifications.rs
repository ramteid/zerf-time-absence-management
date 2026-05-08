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
use std::{convert::Infallible, time::Duration};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

// Re-export the canonical types from the repository layer.
pub use crate::repository::notifications::{NotificationBroadcaster, NotificationSignal};

pub fn broadcaster() -> NotificationBroadcaster {
    crate::repository::notifications::new_broadcaster()
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
    if let Err(e) = state.db.notifications.insert(
        user_id, kind, title, body, reference_type, reference_id,
    ).await {
        tracing::warn!(target:"zerf::notifications", "insert failed: {e}");
        return;
    }
    // Resolve recipient email and dispatch SMTP best-effort.
    if let Some(email) = state.db.notifications.get_user_email(user_id).await {
        let smtp = state.db.settings.load_smtp_config().await.map(std::sync::Arc::new);
        crate::email::send_async(smtp, email, title.to_string(), body.to_string());
    }
}

#[allow(clippy::too_many_arguments)]
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

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<crate::repository::notifications::Notification>>> {
    Ok(Json(app_state.db.notifications.list_for_user(requester.id).await?))
}

pub async fn unread_count(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let count = app_state.db.notifications.count_unread(requester.id).await?;
    Ok(Json(serde_json::json!({ "count": count })))
}

pub async fn stream(
    State(app_state): State<AppState>,
    requester: User,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let requester_id = requester.id;
    let stream = BroadcastStream::new(app_state.db.notifications.subscribe()).filter_map(move |msg| {
        let should_refresh = match msg {
            Ok(signal) => signal.user_id == requester_id,
            Err(_) => true, // lagged — refresh to catch up
        };
        should_refresh.then_some(Ok(Event::default()
            .event("notification")
            .data(r#"{"type":"refresh"}"#)))
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
    let rows_updated = app_state.db.notifications.mark_read(notification_id, requester.id).await?;
    if rows_updated == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn mark_all_read(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let rows_updated = app_state.db.notifications.mark_all_read(requester.id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "count": rows_updated }),
    ))
}

pub async fn delete_all(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let rows_deleted = app_state.db.notifications.delete_all(requester.id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "count": rows_deleted }),
    ))
}

/// Trim notifications older than 90 days; called from the background loop.
pub async fn cleanup_old(db: &crate::repository::Db) {
    db.notifications.cleanup_old().await;
}
