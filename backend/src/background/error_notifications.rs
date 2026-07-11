//! Background worker: drains the `error_notification_queue` and fans each
//! queued technical-error event out to opted-in admins (in-app + email) through
//! the notification facade, then deletes the row.
//!
//! Producers are decoupled from delivery: the backend's log-capture writer and
//! curated call sites enqueue via `services::notifications::enqueue_error`, and
//! the backup container enqueues the same rows via `psql`. This worker is the
//! single consumer. Each row is processed exactly once and deleted afterwards,
//! so a missing SMTP configuration (email skipped, in-app still posted) can
//! never cause endless retries.

use crate::AppState;
use std::time::Duration;

/// Rows processed per wake-up. Bounds the work per tick during an error burst.
const BATCH_LIMIT: i64 = 50;

/// Poll cadence. Short enough to be "prompt" for both backend-enqueued rows and
/// backup-container rows, cheap enough to run continuously against an empty,
/// indexed queue.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub async fn run_loop(state: AppState) {
    loop {
        process_pending(&state).await;
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Drain and process one batch of pending error events. Public so integration
/// tests can drive a single deterministic pass without the polling loop.
pub async fn process_pending(state: &AppState) {
    let entries = match state.db.error_queue.list_pending(BATCH_LIMIT).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!(target: "zerf::error_notify", "list pending failed: {e}");
            return;
        }
    };
    for entry in entries {
        let handled = crate::services::notifications::deliver_error_to_opted_in_admins(
            state,
            entry.dedupe_key.as_deref(),
            &entry.title,
            entry.body.as_deref().unwrap_or(""),
        )
        .await;
        // Delete once the event was handled — delivered, nobody opted in, or
        // email intentionally skipped (no SMTP). An infrastructure failure
        // (`handled == false`) keeps the row queued for the next poll instead
        // of silently dropping the event.
        if !handled {
            continue;
        }
        if let Err(e) = state.db.error_queue.delete_entry(entry.id).await {
            tracing::error!(target: "zerf::error_notify", "delete entry {} failed: {e}", entry.id);
        }
    }
}
