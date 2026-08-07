//! Background worker: drains the `email_queue` table and delivers each row
//! via SMTP, guarded by the shared [`crate::email::CircuitBreaker`].
//!
//! Every notification-driven email in the app is queued through
//! `crate::email::queue_email` rather than sent directly (see that module's
//! doc comment for the two exceptions: the payroll report's own attachment
//! send, and the admin "test connection" probe). This worker is the single
//! consumer of that queue: it polls every 2 minutes, sends pending rows in
//! the order `EmailQueueDb::list_pending` hands back (new messages first,
//! then previously-failed ones least-recently-retried first — see that
//! method's doc comment for why strict creation order would let one
//! undeliverable message starve everything behind it), and deletes a row
//! only once the SMTP server confirmed it accepted the message. A row that
//! keeps failing simply stays queued — nothing is ever silently dropped, and
//! it is retried indefinitely.

use crate::AppState;
use std::time::Duration;

/// Poll cadence: check the queue every 2 minutes rather than sending emails
/// in a detached fire-and-forget task.
const POLL_INTERVAL: Duration = Duration::from_secs(2 * 60);

/// Rows processed per wake-up. Bounds the work per tick during a burst; the
/// rest simply waits for the next poll.
const BATCH_LIMIT: i64 = 50;

/// Attempt count at which a still-failing email is worth a one-time log
/// line. Not re-logged on every attempt past this point — the row keeps
/// retrying forever regardless, per the "never drop an email" requirement.
const ATTEMPT_WARNING_THRESHOLD: i32 = 100;

pub async fn run_loop(state: AppState) {
    loop {
        process_pending(&state).await;
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Drain and process one batch of pending emails. Public so integration
/// tests can drive a single deterministic pass without the polling loop.
pub async fn process_pending(state: &AppState) {
    // SMTP may have been disabled (or never configured) after some emails
    // were already queued; leave them queued untouched rather than
    // attempting delivery or warning about it — this is an accepted,
    // intentional state, not a failure.
    let Some(smtp) = state.db.settings.load_smtp_config().await else {
        return;
    };

    let entries = match state.db.email_queue.list_pending(BATCH_LIMIT).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!(target: "zerf::email_queue", "list pending failed: {e}");
            return;
        }
    };

    for entry in entries {
        match crate::email::send_queued(
            &state.email_circuit_breaker,
            &smtp,
            &entry.to_address,
            &entry.to_name,
            &entry.subject,
            &entry.body_text,
        )
        .await
        {
            Ok(()) => {
                if let Err(e) = state.db.email_queue.delete_entry(entry.id).await {
                    tracing::error!(target: "zerf::email_queue", "delete entry {} failed: {e}", entry.id);
                }
            }
            Err(crate::email::GuardedSendError::CircuitOpen) => {
                // The breaker denied this attempt outright — no SMTP
                // transaction happened, so this entry's attempt counter is
                // left untouched. Stop the whole batch: the server is known
                // to be down right now, so trying the rest would just fail
                // too; they all get another chance next cycle.
                tracing::debug!(
                    target: "zerf::email_queue",
                    "circuit breaker open; deferring remaining queue to the next cycle"
                );
                break;
            }
            Err(crate::email::GuardedSendError::Smtp(e)) => {
                let attempts = match state
                    .db
                    .email_queue
                    .record_failure(entry.id, &e.to_string())
                    .await
                {
                    Ok(attempts) => attempts,
                    Err(db_err) => {
                        tracing::error!(
                            target: "zerf::email_queue",
                            "record_failure for entry {} failed: {db_err}",
                            entry.id
                        );
                        continue;
                    }
                };
                if attempts == ATTEMPT_WARNING_THRESHOLD {
                    tracing::warn!(
                        target: "zerf::email_queue",
                        "email {} to {} has failed {attempts} delivery attempts and remains queued: {e}",
                        entry.id,
                        entry.to_address
                    );
                }
            }
        }
    }
}
