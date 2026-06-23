//! Hourly background task: sends throttled alert emails for system-error
//! notifications that were written directly to the DB by `backup.sh` (via
//! psql) without going through Rust.
//!
//! For failures that originate inside Rust (e.g. report PDF upload),
//! `services::notifications::notify_admins_system_error` handles the email
//! immediately.  This task acts as a catch-all for the bash side.
//!
//! Email throttle: at most one email per failure class per calendar day, stored
//! as `system_alert_email_{dedupe_key}` in `app_settings`.

use crate::services::{notifications, settings};
use crate::AppState;

pub async fn run_loop(state: AppState) {
    // Brief startup delay so DB migrations are guaranteed to have applied
    // before we issue any queries.
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;

    loop {
        send_pending_alert_emails(&state).await;
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

/// Check for unread pinned (system-error) notifications whose failure class
/// has not yet been emailed today and send throttled emails to all active admins.
async fn send_pending_alert_emails(state: &AppState) {
    let classes = match state.db.notifications.list_unread_system_error_classes().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(target: "zerf::system_alerts", "failed to query system error classes: {e}");
            return;
        }
    };

    if classes.is_empty() {
        return;
    }

    let today = settings::app_today(&state.pool)
        .await
        .format("%Y-%m-%d")
        .to_string();

    let all_users = match state.db.users.find_all_ordered().await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(target: "zerf::system_alerts", "failed to list users: {e}");
            return;
        }
    };
    let admins: Vec<_> = all_users
        .into_iter()
        .filter(|u| u.active && u.is_admin())
        .collect();

    let language = notifications::load_language(&state.pool).await;

    for (_kind, dedupe_key, title) in &classes {
        let email_key = format!("system_alert_email_{dedupe_key}");
        let last_sent = settings::load_setting(&state.pool, &email_key, "")
            .await
            .unwrap_or_default();
        if last_sent == today {
            continue;
        }

        for user in &admins {
            notifications::send_alert_email_to_user(state, &language, user.id, title).await;
        }
        if let Err(e) = state.db.settings.save_setting(&email_key, &today).await {
            tracing::warn!(
                target: "zerf::system_alerts",
                "failed to write email throttle key {email_key}: {e}"
            );
        }
        tracing::info!(
            target: "zerf::system_alerts",
            "sent system alert emails for class '{dedupe_key}'"
        );
    }
}
