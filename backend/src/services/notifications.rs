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

/// Delivery channels for a notification. `InAppAndEmail` is the default and
/// what most notifications want; the other variants are the "switch".
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    /// Store an in-app notification and send an email sidecar.
    InAppAndEmail,
    /// In-app only (e.g. when the requester is also the recipient).
    InAppOnly,
    /// Email only, no stored notification (transactional auth mails).
    EmailOnly,
}

/// A single notification addressed to one user. Build with [`Outgoing::new`],
/// refine via the builder setters, then hand to [`deliver`] — the one entry
/// point every part of the app uses to notify a user.
///
/// `body` is stored verbatim in-app. `email_body`, when set, is the plain text
/// used for the email; otherwise `body` is reused. All user-facing copy is
/// rendered from the central templates in `i18n.rs` before constructing this
/// value. The rendering language travels with the message so its shared email
/// footer cannot switch language while an asynchronous delivery is pending.
pub struct Outgoing<'a> {
    user_id: i64,
    language: Language,
    kind: &'a str,
    title: &'a str,
    body: &'a str,
    email_body: Option<&'a str>,
    reference_type: Option<&'a str>,
    reference_id: Option<i64>,
    channels: Channels,
    dedupe_key: Option<&'a str>,
    pinned: bool,
    append_email_footer: bool,
}

impl<'a> Outgoing<'a> {
    pub fn new(
        user_id: i64,
        language: &Language,
        kind: &'a str,
        title: &'a str,
        body: &'a str,
    ) -> Self {
        Self {
            user_id,
            language: *language,
            kind,
            title,
            body,
            email_body: None,
            reference_type: None,
            reference_id: None,
            channels: Channels::InAppAndEmail,
            dedupe_key: None,
            pinned: false,
            append_email_footer: true,
        }
    }

    /// Select the delivery channels (the switch). Default: `InAppAndEmail`.
    pub fn channels(mut self, channels: Channels) -> Self {
        self.channels = channels;
        self
    }

    /// Distinct plain-text body for the email when a channel needs additional
    /// content such as a login URL or reminder instructions.
    pub fn email_body(mut self, email_body: &'a str) -> Self {
        self.email_body = Some(email_body);
        self
    }

    /// Link the notification to a domain item so it can be cleared later.
    pub fn reference(mut self, reference_type: &'a str, reference_id: Option<i64>) -> Self {
        self.reference_type = Some(reference_type);
        self.reference_id = reference_id;
        self
    }

    /// Deduplicate: idempotent insert (or pinned re-alert when `pinned`).
    pub fn dedupe_key(mut self, dedupe_key: &'a str) -> Self {
        self.dedupe_key = Some(dedupe_key);
        self
    }

    /// Mark as a pinned notification with re-alert semantics (system errors).
    /// Must be combined with [`Outgoing::dedupe_key`]: pinning is implemented
    /// via the deduplicating upsert, so without a key the write falls back to a
    /// plain (unpinned) insert.
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    /// Suppress the timestamp/app-URL email footer (transactional auth mails
    /// that build their own complete body).
    pub fn append_email_footer(mut self, append: bool) -> Self {
        self.append_email_footer = append;
        self
    }
}

/// Deliver one notification across its configured channels.
///
/// Returns whether a **new** in-app row was created/re-alerted (`false` when a
/// dedupe guard suppressed it), so idempotent callers (reminders) can tell
/// whether anything was actually sent. The email sidecar is sent only when a
/// new row was created (or the channel is `EmailOnly`).
pub async fn deliver(state: &AppState, msg: &Outgoing<'_>) -> bool {
    // In-app write (skipped for EmailOnly, which never stores a row).
    let created = if msg.channels == Channels::EmailOnly {
        true
    } else {
        write_in_app(state, msg).await
    };

    if msg.channels != Channels::InAppOnly && created {
        let email_body = msg.email_body.unwrap_or(msg.body);
        send_notification_email(
            state,
            &msg.language,
            msg.user_id,
            msg.title.to_string(),
            email_body,
            msg.append_email_footer,
        )
        .await;
    }

    created
}

/// Write the in-app row using the cheapest repository method for the request:
/// pinned+dedupe → re-alert upsert; dedupe only → idempotent insert; otherwise
/// a plain insert. Returns whether a new/re-alerted row resulted.
async fn write_in_app(state: &AppState, msg: &Outgoing<'_>) -> bool {
    let result = match (msg.pinned, msg.dedupe_key) {
        (true, Some(key)) => {
            state
                .db
                .notifications
                .upsert_system_error(msg.user_id, msg.kind, key, msg.title, msg.body)
                .await
        }
        (false, Some(key)) => state
            .db
            .notifications
            .insert_idempotent_with_dedupe_key(
                msg.user_id,
                msg.kind,
                msg.title,
                msg.body,
                msg.reference_type,
                msg.reference_id,
                Some(key),
            )
            .await
            .inspect(|&inserted| {
                // Idempotent insert does not broadcast; do it here on success.
                if inserted {
                    state.db.notifications.broadcast(msg.user_id);
                }
            }),
        _ => state
            .db
            .notifications
            .insert(
                msg.user_id,
                msg.kind,
                msg.title,
                msg.body,
                msg.reference_type,
                msg.reference_id,
            )
            .await
            .map(|()| true),
    };
    match result {
        Ok(created) => created,
        Err(e) => {
            tracing::warn!(target: "zerf::notifications", "insert failed for user {}: {e}", msg.user_id);
            false
        }
    }
}

/// Queue a notification email for delivery (non-fatal: enqueue failures are
/// only logged). When `append_footer` is true the configured timestamp and
/// public app URL are appended. No-op when SMTP is not enabled/configured
/// (the whole email feature is opt-in) — nothing is queued in that case.
async fn send_notification_email(
    state: &AppState,
    language: &Language,
    user_id: i64,
    subject: String,
    body: &str,
    append_footer: bool,
) {
    if let Some((email, first_name, last_name)) =
        state.db.notifications.get_user_email(user_id).await
    {
        let recipient_name = format!("{} {}", first_name, last_name);
        let smtp_configured = state.db.settings.load_smtp_config().await.is_some();
        let email_body = if append_footer {
            let timezone = crate::services::settings::load_setting(
                &state.pool,
                crate::services::settings::TIMEZONE_KEY,
                crate::services::settings::DEFAULT_TIMEZONE,
            )
            .await
            .unwrap_or_else(|_| crate::services::settings::DEFAULT_TIMEZONE.to_string());
            let timestamp =
                crate::i18n::format_datetime_in_timezone(language, chrono::Utc::now(), &timezone);
            crate::i18n::email_with_footer(
                language,
                body,
                &timestamp,
                state.cfg.public_url.as_deref(),
            )
        } else {
            body.to_string()
        };
        crate::email::queue_email(
            &state.db.email_queue,
            smtp_configured,
            &email,
            &recipient_name,
            &subject,
            &email_body,
        )
        .await;
    }
}

/// Clear pending approval notifications for an item once it has been decided
/// (approved, rejected, revoked, etc.). All recipients keep the row in their
/// history but the in-app badge and dashboard "open requests" view will no
/// longer surface it. Failures are non-fatal — the underlying transition has
/// already committed.
pub async fn clear_pending_for_reference(
    state: &AppState,
    reference_type: &str,
    reference_id: i64,
) {
    if let Err(e) = state
        .db
        .notifications
        .mark_read_by_reference(reference_type, reference_id)
        .await
    {
        tracing::warn!(
            target: "zerf::notifications",
            "mark_read_by_reference({reference_type}, {reference_id}) failed: {e}"
        );
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

/// Notification kind for technical system-error alerts (pinned, deduped).
pub const SYSTEM_ERROR_KIND: &str = "system_error";

/// Enqueue a technical-error event for asynchronous fan-out to opted-in admins.
///
/// Producer side: returns immediately after writing one queue row. A background
/// worker ([`deliver_error_to_opted_in_admins`]) drains the queue and delivers
/// the in-app + email notifications. The backup container enqueues the same way
/// via `psql`.
///
/// `dedupe_key` identifies the failure class (deduplicates repeat alerts);
/// `title` is a short summary; `body` holds the failure-specific detail.
pub async fn enqueue_error(
    state: &AppState,
    language: &Language,
    dedupe_key: &str,
    title: &str,
    body: &str,
) {
    let source = format!("app:{}", language.code());
    if let Err(e) = state
        .db
        .error_queue
        .enqueue(Some(dedupe_key), title, Some(body), &source)
        .await
    {
        tracing::warn!(target: "zerf::notifications", "enqueue_error failed: {e}");
    }
}

/// Fan one queued error out to every active admin who opted in to technical
/// error notifications: a pinned in-app notice plus an email, both through
/// [`deliver`]. When SMTP is unconfigured the in-app notices are still created;
/// a single warning is logged and no email is sent.
///
/// Returns whether the event was handled. `true` covers every intentional
/// outcome — delivered, no opted-in admins, missing SMTP — and tells the worker
/// to delete the queue row so nothing is retried endlessly. `false` means an
/// infrastructure failure (recipients query) prevented any handling; the row
/// stays queued for the next poll.
pub async fn deliver_error_to_opted_in_admins(
    state: &AppState,
    language: &Language,
    dedupe_key: Option<&str>,
    title: &str,
    body: &str,
) -> bool {
    let recipient_ids = match state.db.users.error_notification_recipient_ids().await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(target: "zerf::notifications", "error recipients query failed: {e}");
            return false;
        }
    };
    if recipient_ids.is_empty() {
        return true;
    }
    // Surface a missing email configuration once per event: opted-in admins want
    // emails, but none can be sent. In-app notices are still posted below.
    if state.db.settings.load_smtp_config().await.is_none() {
        tracing::warn!(
            target: "zerf::notifications",
            "SMTP not configured; error notification delivered in-app only: {title}"
        );
    }
    // Pinned + dedupe → re-alert upsert semantics so a recurring failure floats
    // back to the top without piling up duplicate rows.
    let dedupe = dedupe_key.unwrap_or(title);
    let email_body = crate::i18n::technical_error_email_body(language, title, body);
    for user_id in recipient_ids {
        deliver(
            state,
            &Outgoing::new(user_id, language, SYSTEM_ERROR_KIND, title, body)
                .email_body(&email_body)
                .dedupe_key(dedupe)
                .pinned(),
        )
        .await;
    }
    true
}

/// Trim notifications older than 90 days; called from the background loop.
pub async fn cleanup_old(db: &crate::repository::Db) {
    db.notifications.cleanup_old().await;
}

pub async fn list_for_user(state: &AppState, user_id: i64) -> AppResult<Vec<Notification>> {
    let language = load_language(&state.pool).await;
    let mut notifications = state.db.notifications.list_for_user(user_id).await?;
    for notification in &mut notifications {
        let Some(body) = notification.body.as_deref() else {
            continue;
        };
        let Some(text) = crate::i18n::legacy_notification_text(&language, &notification.kind, body)
        else {
            continue;
        };
        notification.title = text.title;
        notification.body = Some(text.body);
    }
    Ok(notifications)
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
