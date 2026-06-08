//! HTTP handlers for the in-app notification center (SSE stream + CRUD).

use crate::error::AppResult;
use crate::middleware::auth::User;
use crate::services::notifications::{self, Notification};
use crate::AppState;
use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use std::{convert::Infallible, time::Duration};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<Notification>>> {
    Ok(Json(
        notifications::list_for_user(&app_state, requester.id).await?,
    ))
}

pub async fn unread_count(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let count = notifications::unread_count(&app_state, requester.id).await?;
    Ok(Json(serde_json::json!({ "count": count })))
}

pub async fn stream(
    State(app_state): State<AppState>,
    requester: User,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let requester_id = requester.id;
    let stream =
        BroadcastStream::new(app_state.db.notifications.subscribe()).filter_map(move |msg| {
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
    notifications::mark_read(&app_state, requester.id, notification_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn mark_all_read(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let rows_updated = notifications::mark_all_read(&app_state, requester.id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "count": rows_updated }),
    ))
}

pub async fn delete_all(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<serde_json::Value>> {
    let rows_deleted = notifications::delete_all(&app_state, requester.id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "count": rows_deleted }),
    ))
}
