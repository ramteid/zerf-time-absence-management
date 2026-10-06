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
//!
//! A failed delivery is never silent either. Its first failed attempt, any
//! attempt that fails for a different reason than the one before, and its
//! 100th (`PERSISTENT_FAILURE_ATTEMPTS`) are logged as errors and reported to
//! the opted-in admins (see `is_reported_failure` and
//! `report_failed_delivery`); the repeats in between are logged at INFO only.

use crate::error::AppResult;
use crate::repository::EmailQueueEntry;
use crate::services::notifications::{enqueue_error, load_language, SYSTEM_ERROR_KIND};
use crate::AppState;
use std::time::Duration;

/// Poll cadence: check the queue every 2 minutes rather than sending emails
/// in a detached fire-and-forget task.
const POLL_INTERVAL: Duration = Duration::from_secs(2 * 60);

/// Rows processed per wake-up. Bounds the work per tick during a burst; the
/// rest simply waits for the next poll.
const BATCH_LIMIT: i64 = 50;

/// Attempt count at which an email that is *still* undeliverable is reported
/// once more, even though its reason has not changed. The first report may have
/// been dismissed as a passing blip; after this many polls (hours, not minutes)
/// it clearly is not one. Past this point only a changed reason is reported
/// again — the row keeps retrying forever regardless, per the "never drop an
/// email" requirement.
const PERSISTENT_FAILURE_ATTEMPTS: i32 = 100;

/// Longest server reply shown in an admin alert. The full text stays in the
/// log and in the row's `last_error`; a pathological multi-line reply must not
/// bloat a notification.
const MAX_ALERT_ERROR_CHARS: usize = 300;

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
        .await {
            Ok(()) => {
                // SMTP already confirmed delivery — a failure here must not
                // leave the row looking untouched, or the next poll would
                // send this exact message again. Retry the delete itself
                // (idempotent: deleting an already-gone row is a no-op)
                // before giving up.
                if let Err(e) = delete_with_retry(&state.db.email_queue, entry.id).await {
                    tracing::error!(
                        target: "zerf::email_queue",
                        "delete entry {} failed after retries; the email was already sent but \
                         the row could not be removed, so it will be sent again next cycle: {e}",
                        entry.id
                    );
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
                let error_text = e.to_string();
                let attempts = match state
                    .db
                    .email_queue
                    .record_failure(entry.id, &error_text)
                    .await
                {
                    Ok(attempts) => attempts,
                    Err(db_err) => {
                        // The failed send is the news, whatever state the
                        // attempt counter is in: a bookkeeping error must
                        // never swallow it.
                        tracing::error!(
                            target: "zerf::email_queue",
                            "email {} to {} could not be delivered: {error_text}",
                            entry.id,
                            entry.to_address
                        );
                        tracing::error!(
                            target: "zerf::email_queue",
                            "record_failure for entry {} failed: {db_err}",
                            entry.id
                        );
                        continue;
                    }
                };
                report_failed_delivery(state, &entry, attempts, &error_text).await;
            }
        }
    }
}

/// Whether this failure is reported (logged as an error and raised to the
/// admins) rather than merely noted at INFO level.
///
/// Three kinds of failure are news to an admin:
/// * the first one of a mail, because a new problem has appeared;
/// * one whose reason differs from the previous attempt's. The first report
///   may have been about a passing outage, and the server that is back now
///   refuses the recipient — that must not wait hours for the next milestone;
/// * the [`PERSISTENT_FAILURE_ATTEMPTS`]th, because the problem has not gone
///   away.
///
/// The quiet for everything else protects the System Log: it keeps 1000 rows,
/// and one address the server keeps refusing is retried on every poll — an
/// error row per retry would push every other entry out of the log within days.
fn is_reported_failure(attempts: i32, previous_error: Option<&str>, error_text: &str) -> bool {
    attempts == 1
        || attempts == PERSISTENT_FAILURE_ATTEMPTS
        // No previous error on a later attempt cannot happen in practice;
        // reporting is the safe reading of it.
        || previous_error.map(failure_class) != Some(failure_class(error_text))
}

/// The stable head of an SMTP error text, such as `permanent error (550)` or
/// `Connection error`: what is left once the details after the first colon
/// (the server's wording, addresses, OS error numbers) are dropped. Two
/// failures of one class are the same reason. Comparing whole texts instead
/// would report every retry whenever the server's wording carries something
/// that changes from attempt to attempt, such as a queue id.
fn failure_class(error_text: &str) -> &str {
    error_text.split(':').next().unwrap_or(error_text).trim()
}

/// Log one failed delivery attempt and, when it is worth reporting (see
/// [`is_reported_failure`]), raise it to the opted-in admins like any other
/// technical error.
///
/// The error is logged under this module's own `zerf::email_queue` target,
/// which the log-capture writer keeps out of its automatic error → admin
/// alert path (a delivery failure must not spawn a notification about
/// itself). So the alert is raised explicitly here — except for a failed
/// *admin alert* ([`SYSTEM_ERROR_KIND`]). Reporting that one would queue one
/// more alert mail, which fails the same way, and so on for as long as the
/// mail server stays down. Such a failure is still logged, and the alert
/// about the original problem already sits in the admins' app notifications.
///
/// The mail body is never logged or quoted: it can carry a temporary password
/// or a reset link.
async fn report_failed_delivery(
    state: &AppState,
    entry: &EmailQueueEntry,
    attempts: i32,
    error_text: &str,
) {
    if !is_reported_failure(attempts, entry.last_error.as_deref(), error_text) {
        tracing::info!(
            target: "zerf::email_queue",
            "email {} to {} is still not delivered (attempt {attempts}): {error_text}",
            entry.id,
            entry.to_address
        );
        return;
    }

    tracing::error!(
        target: "zerf::email_queue",
        "email {} to {} (subject {:?}) could not be delivered (attempt {attempts}): {error_text}",
        entry.id,
        entry.to_address,
        entry.subject
    );

    if entry.kind == SYSTEM_ERROR_KIND {
        return;
    }

    let language = load_language(&state.pool).await;
    let text = crate::i18n::notification_text(
        &language,
        "email_delivery_failed_title",
        "email_delivery_failed_body",
        &[
            ("recipient", entry.to_address.clone()),
            ("subject", entry.subject.clone()),
            ("attempts", attempts.to_string()),
            ("error", alert_error_text(error_text)),
        ],
    );
    // One key per queued mail: a later report about the same mail (new reason,
    // or the persistent milestone) re-raises the first one with its new text
    // instead of piling up beside it, while a different mail failing for the
    // same reason gets a notification of its own.
    enqueue_error(
        state,
        &language,
        &format!("email_delivery_failed_{}", entry.id),
        &text.title,
        &text.body,
    )
    .await;
}

/// The server's reply as a single line of at most [`MAX_ALERT_ERROR_CHARS`]
/// characters. SMTP replies can span several lines.
fn alert_error_text(error_text: &str) -> String {
    let one_line = error_text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = one_line.chars();
    let shortened: String = chars.by_ref().take(MAX_ALERT_ERROR_CHARS).collect();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

/// Delay between delete attempts. Short: this only needs to ride out a
/// momentary DB hiccup (a dropped connection, a pool exhausted for an
/// instant), not a real outage.
const DELETE_RETRY_DELAY: Duration = Duration::from_millis(200);

/// Number of delete attempts before giving up and accepting the row will be
/// re-sent next cycle.
const DELETE_RETRY_ATTEMPTS: u32 = 3;

/// Retry a confirmed-sent row's delete a few times. `DELETE ... WHERE id=$1`
/// is idempotent (deleting an already-gone row is a harmless no-op), so
/// retrying is always safe and never risks a double-delete — it only closes
/// the window in which a transient DB failure would otherwise leave the row
/// looking exactly like one that was never attempted, causing the same
/// email to go out again on the next poll. Public so integration tests can
/// exercise it directly (`process_pending`'s own success branch needs a real
/// SMTP accept to reach it, which the test suite has no way to fake).
pub async fn delete_with_retry(email_queue: &crate::repository::EmailQueueDb, id: i64) -> AppResult<()> {
    // The first DELETE_RETRY_ATTEMPTS - 1 tries get a short backoff between
    // them; the final try's Result is returned directly so the caller sees
    // exactly why the delete kept failing.
    for _ in 1..DELETE_RETRY_ATTEMPTS {
        if email_queue.delete_entry(id).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(DELETE_RETRY_DELAY).await;
    }
    email_queue.delete_entry(id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFUSED: &str = "Connection error: Connection refused (os error 111)";
    const UNKNOWN_USER: &str = "permanent error (550): 5.1.1 User unknown";

    #[test]
    fn the_first_and_the_persistent_attempt_are_always_reported() {
        assert!(is_reported_failure(1, None, REFUSED));
        assert!(is_reported_failure(
            PERSISTENT_FAILURE_ATTEMPTS,
            Some(REFUSED),
            REFUSED
        ));
    }

    #[test]
    fn a_repeat_of_the_same_reason_stays_quiet() {
        for attempts in [
            2,
            3,
            5,
            50,
            PERSISTENT_FAILURE_ATTEMPTS - 1,
            PERSISTENT_FAILURE_ATTEMPTS + 1,
            500,
        ] {
            assert!(
                !is_reported_failure(attempts, Some(REFUSED), REFUSED),
                "attempt {attempts} must stay quiet"
            );
        }
    }

    #[test]
    fn a_changed_reason_is_reported_between_the_milestones() {
        // The server was unreachable at first and now answers, refusing the
        // recipient: new information, not a repeat.
        assert!(is_reported_failure(2, Some(REFUSED), UNKNOWN_USER));
        assert!(is_reported_failure(
            PERSISTENT_FAILURE_ATTEMPTS + 7,
            Some(UNKNOWN_USER),
            REFUSED
        ));
        // A failure with no recorded predecessor is reported rather than lost.
        assert!(is_reported_failure(3, None, REFUSED));
    }

    #[test]
    fn changing_server_detail_is_not_a_changed_reason() {
        assert!(!is_reported_failure(
            7,
            Some("permanent error (550): 5.7.1 Blocked, id=abc123"),
            "permanent error (550): 5.7.1 Blocked, id=def456"
        ));
        assert!(!is_reported_failure(
            7,
            Some("Connection error: Connection refused (os error 111)"),
            "Connection error: timed out"
        ));
    }

    #[test]
    fn the_failure_class_is_the_text_before_the_first_colon() {
        assert_eq!(failure_class(UNKNOWN_USER), "permanent error (550)");
        assert_eq!(failure_class(REFUSED), "Connection error");
        assert_eq!(failure_class("Connection error"), "Connection error");
        assert_eq!(
            failure_class("SMTP delivery timed out after 30 seconds"),
            "SMTP delivery timed out after 30 seconds"
        );
    }

    #[test]
    fn a_multi_line_server_reply_becomes_one_line() {
        assert_eq!(
            alert_error_text("550-5.7.1 blocked\n550 5.7.1   see policy\r\n"),
            "550-5.7.1 blocked 550 5.7.1 see policy"
        );
    }

    #[test]
    fn an_overlong_server_reply_is_cut_at_a_character_boundary() {
        // 'ü' is two bytes in UTF-8, so a byte-based cut would split it.
        let long = "ü".repeat(MAX_ALERT_ERROR_CHARS + 50);
        let shortened = alert_error_text(&long);
        assert_eq!(shortened.chars().count(), MAX_ALERT_ERROR_CHARS + 1);
        assert!(shortened.ends_with('…'));

        // A reply exactly at the limit is left alone.
        let exact = "x".repeat(MAX_ALERT_ERROR_CHARS);
        assert_eq!(alert_error_text(&exact), exact);
    }
}
