//! Notification service: create in-app notifications with optional email sidecars,
//! load UI language, clean up old records.
//!
//! Notifications are immutable once created (only `is_read` flips).
//! Cleanup beyond 90 days happens in the background loop in `main.rs`.

use crate::error::{AppError, AppResult};
use crate::i18n::Language;
use crate::AppState;

// Re-export canonical types from the repository layer so callers only need
// to import from this module.
pub use crate::repository::notifications::{
    Notification, NotificationBroadcaster, NotificationSignal,
};

pub fn broadcaster() -> NotificationBroadcaster {
    crate::repository::notifications::new_broadcaster()
}

/// Send notification email best-effort (non-fatal on failure).
async fn send_notification_email(
    state: &AppState,
    language: &Language,
    user_id: i64,
    subject: String,
    body: &str,
) {
    if let Some((email, first_name, last_name)) =
        state.db.notifications.get_user_email(user_id).await
    {
        let recipient_name = format!("{} {}", first_name, last_name);
        let smtp = state
            .db
            .settings
            .load_smtp_config()
            .await
            .map(std::sync::Arc::new);
        let timezone = crate::services::settings::load_setting(
            &state.pool,
            crate::services::settings::TIMEZONE_KEY,
            crate::services::settings::DEFAULT_TIMEZONE,
        )
        .await
        .unwrap_or_else(|_| crate::services::settings::DEFAULT_TIMEZONE.to_string());
        let timestamp =
            crate::i18n::format_datetime_in_timezone(language, chrono::Utc::now(), &timezone);
        let email_body = match &state.cfg.public_url {
            Some(url) => format!("{body}\n\n{timestamp}\n\n{url}"),
            None => format!("{body}\n\n{timestamp}"),
        };
        crate::email::send_async(smtp, email, recipient_name, subject, email_body);
    }
}

/// Insert a notification row. `email` is sent best-effort via SMTP if
/// configured. Both operations are non-fatal: failures are logged but not
/// propagated.
///
/// The in-app notification stores `body` verbatim. The outgoing email appends
/// the public app URL so recipients can navigate directly to the application.
pub async fn create(
    state: &AppState,
    user_id: i64,
    kind: &str,
    title: &str,
    body: &str,
    reference_type: Option<&str>,
    reference_id: Option<i64>,
) {
    if let Err(e) = state
        .db
        .notifications
        .insert(user_id, kind, title, body, reference_type, reference_id)
        .await
    {
        tracing::warn!(target:"zerf::notifications", "insert failed: {e}");
        return;
    }
    let language = crate::i18n::load_ui_language(&state.pool)
        .await
        .unwrap_or_default();
    send_notification_email(state, &language, user_id, title.to_string(), body).await;
}

/// Insert an in-app-only notification row, skipping the email sidecar.
/// Used when the requester is also the recipient (e.g. an admin approving
/// or rejecting their own submission) to avoid self-addressed emails.
pub async fn create_inapp_only(
    state: &AppState,
    user_id: i64,
    kind: &str,
    title: &str,
    body: &str,
    reference_type: Option<&str>,
    reference_id: Option<i64>,
) {
    if let Err(e) = state
        .db
        .notifications
        .insert(user_id, kind, title, body, reference_type, reference_id)
        .await
    {
        tracing::warn!(target:"zerf::notifications", "insert failed: {e}");
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn create_translated_inapp_only(
    state: &AppState,
    language: &Language,
    user_id: i64,
    kind: &str,
    title_key: &str,
    body_key: &str,
    params: Vec<(&'static str, String)>,
    reference_type: Option<&str>,
    reference_id: Option<i64>,
) {
    let title = crate::i18n::translate(language, title_key, &params);
    let body = crate::i18n::translate(language, body_key, &params);
    create_inapp_only(
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

#[allow(clippy::too_many_arguments)]
pub async fn create_translated(
    state: &AppState,
    language: &Language,
    user_id: i64,
    kind: &str,
    title_key: &str,
    body_key: &str,
    params: Vec<(&'static str, String)>,
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

/// Create a notification storing `frontend_body` in the DB (for frontend
/// rendering from structured data) while sending the email with the
/// i18n-rendered body. When `send_email` is false, no email is sent
/// (used for self-notifications).
#[allow(clippy::too_many_arguments)]
pub async fn create_with_frontend_body(
    state: &AppState,
    language: &Language,
    user_id: i64,
    kind: &str,
    title_key: &str,
    email_body_key: &str,
    params: Vec<(&'static str, String)>,
    frontend_body: &str,
    send_email: bool,
    reference_type: Option<&str>,
    reference_id: Option<i64>,
) {
    let title = crate::i18n::translate(language, title_key, &params);
    if let Err(e) = state
        .db
        .notifications
        .insert(
            user_id,
            kind,
            &title,
            frontend_body,
            reference_type,
            reference_id,
        )
        .await
    {
        tracing::warn!(target:"zerf::notifications", "insert failed: {e}");
        return;
    }
    if send_email {
        let email_body = crate::i18n::translate(language, email_body_key, &params);
        send_notification_email(state, language, user_id, title, &email_body).await;
    }
}

/// Load the configured UI language, falling back to the default on error.
/// Used by notification senders across all modules.
pub async fn load_language(pool: &crate::db::DatabasePool) -> crate::i18n::Language {
    match crate::i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::warn!(target: "zerf::notifications", "load notification language failed: {e}");
            crate::i18n::Language::default()
        }
    }
}

/// Trim notifications older than 90 days; called from the background loop.
pub async fn cleanup_old(db: &crate::repository::Db) {
    db.notifications.cleanup_old().await;
}

pub async fn list_for_user(state: &AppState, user_id: i64) -> AppResult<Vec<Notification>> {
    state.db.notifications.list_for_user(user_id).await
}

pub async fn unread_count(state: &AppState, user_id: i64) -> AppResult<i64> {
    state.db.notifications.count_unread(user_id).await
}

pub async fn mark_read(state: &AppState, user_id: i64, notification_id: i64) -> AppResult<()> {
    let rows_updated = state
        .db
        .notifications
        .mark_read(notification_id, user_id)
        .await?;
    if rows_updated == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn mark_all_read(state: &AppState, user_id: i64) -> AppResult<u64> {
    state.db.notifications.mark_all_read(user_id).await
}

pub async fn delete_all(state: &AppState, user_id: i64) -> AppResult<u64> {
    state.db.notifications.delete_all(user_id).await
}
