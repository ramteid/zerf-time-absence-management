//! Background worker: drains the `email_queue` table and delivers each row
//! via SMTP, guarded by the shared [`crate::email::CircuitBreaker`].
//!
//! Every notification-driven email in the app is queued through
//! `crate::email::queue_email` rather than sent directly (see that module's
//! doc comment for the two exceptions: the payroll report's own attachment
//! send, and the admin "test connection" probe). This worker is the single
//! consumer of that queue: it polls every 2 minutes, sends the rows that are
//! due in the order `EmailQueueDb::list_due` hands back (new messages first,
//! then previously-failed ones least-recently-retried first — see that
//! method's doc comment for why strict creation order would let one
//! undeliverable message starve everything behind it), and deletes a row only
//! once the SMTP server confirmed it accepted the message.
//!
//! A mail that fails is neither dropped nor reported at once. The mail server
//! is often unreachable for only a short while, and the next try then simply
//! succeeds, so the mail is retried on a widening schedule
//! ([`RETRY_DELAYS_SECONDS`]). Only when the last scheduled attempt fails too
//! is it given up: the row is marked `given_up_at` and never tried again, the
//! failure is logged as an error and reported — once — to the opted-in admins
//! (see `report_failed_attempt`). Every earlier failed attempt is logged as a
//! warning and reaches nobody.

use crate::error::AppResult;
use crate::repository::{EmailQueueEntry, FailedAttempt};
use crate::services::notifications::{enqueue_error, load_language, SYSTEM_ERROR_KIND};
use crate::AppState;
use std::time::Duration;

/// Poll cadence: check the queue every 2 minutes rather than sending emails
/// in a detached fire-and-forget task.
const POLL_INTERVAL: Duration = Duration::from_secs(2 * 60);

/// Rows processed per wake-up. Bounds the work per tick during a burst; the
/// rest simply waits for the next poll.
const BATCH_LIMIT: i64 = 50;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;

/// How long a mail waits after its 1st, 2nd, ... failed attempt before the
/// next one is made. The first retries come quickly, because the usual
/// outage is short; the later, longer waits cover one that lasts for hours.
/// Together they keep a mail in play for 17 hours and 20 minutes after its
/// first attempt (5 min + 15 min + 1 h + 4 h + 12 h), which is long enough to
/// ride out a night and short enough for an admin to still react the next
/// morning.
///
/// No wait may be shorter than [`POLL_INTERVAL`]: the worker could not honour
/// it anyway, since a due row is picked up at the next poll. The waits are
/// also what keeps one stuck message from being retried on every poll and
/// from hammering the mail server in the meantime.
pub const RETRY_DELAYS_SECONDS: [i64; 5] = [5 * MINUTE, 15 * MINUTE, HOUR, 4 * HOUR, 12 * HOUR];

/// Attempts a mail gets in total: the first one plus one after each waiting
/// time. The attempt that fails as number `MAX_ATTEMPTS` is the final failure.
pub const MAX_ATTEMPTS: i32 = RETRY_DELAYS_SECONDS.len() as i32 + 1;

/// Days a given-up mail stays in the table before the daily cleanup removes
/// it. Long enough to look up what was lost, short enough not to keep a
/// temporary password or reset link around for good.
pub const GIVEN_UP_RETENTION_DAYS: i32 = 30;

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

/// Drain and process one batch of due emails. Public so integration tests can
/// drive a single deterministic pass without the polling loop.
pub async fn process_pending(state: &AppState) {
    // SMTP may have been disabled (or never configured) after some emails
    // were already queued; leave them queued untouched rather than
    // attempting delivery or warning about it — this is an accepted,
    // intentional state, not a failure.
    let Some(smtp) = state.db.settings.load_smtp_config().await else {
        return;
    };

    let entries = match state
        .db
        .email_queue
        .list_due(BATCH_LIMIT, &RETRY_DELAYS_SECONDS)
        .await
    {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!(target: "zerf::email_queue", "list due failed: {e}");
            return;
        }
    };

    for entry in entries {
        match crate::email::send_queued(&state.email_circuit_breaker, &smtp, &entry).await {
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
                let failed_attempt = match state
                    .db
                    .email_queue
                    .record_failure(entry.id, &error_text, MAX_ATTEMPTS)
                    .await
                {
                    Ok(failed_attempt) => failed_attempt,
                    Err(db_err) => {
                        // The failed send is the news, whatever state the
                        // attempt counter is in: a bookkeeping error must
                        // never swallow it. The row keeps its old counter, so
                        // it is simply tried again at the next poll.
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
                report_failed_attempt(state, &entry, &failed_attempt, &error_text).await;
            }
        }
    }
}

/// Log one failed delivery attempt and, when it was the mail's last one, raise
/// it to the opted-in admins like any other technical error.
///
/// Only the final failure is news to an admin. An attempt that will be
/// followed by another one is a passing outage as far as anybody can tell, and
/// the retry usually succeeds. It is logged as a warning ("the app still did
/// its job"): that keeps it in the System Log, where a flaky mail server shows
/// up as a pattern, without alerting anyone. The number of warnings is bounded
/// by [`MAX_ATTEMPTS`] per mail, so a mail that keeps failing cannot flood the
/// log that keeps only 1000 rows.
///
/// The final failure is logged as an error under this module's own
/// `zerf::email_queue` target, which the log-capture writer keeps out of its
/// automatic error → admin alert path (a delivery failure must not spawn a
/// notification about itself). So the alert is raised explicitly here — except
/// for a failed *admin alert* ([`SYSTEM_ERROR_KIND`]). Reporting that one would
/// queue one more alert mail, which fails the same way, and so on for as long
/// as the mail server stays down. Such a failure is still logged, and the
/// alert about the original problem already sits in the admins' app
/// notifications.
///
/// The mail body is never logged or quoted: it can carry a temporary password
/// or a reset link.
async fn report_failed_attempt(
    state: &AppState,
    entry: &EmailQueueEntry,
    failed_attempt: &FailedAttempt,
    error_text: &str,
) {
    let attempts = failed_attempt.attempts;
    if !failed_attempt.given_up {
        tracing::warn!(
            target: "zerf::email_queue",
            "email {} to {} could not be delivered (attempt {attempts} of {MAX_ATTEMPTS}), \
             it will be tried again later: {error_text}",
            entry.id,
            entry.to_address
        );
        return;
    }

    tracing::error!(
        target: "zerf::email_queue",
        "email {} to {} (subject {:?}) could not be delivered; giving up after {attempts} \
         attempts: {error_text}",
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
    // One key per queued mail, so a different mail failing for the same reason
    // gets a notification of its own. A mail is given up only once, which makes
    // the key a safeguard against a duplicate rather than a way to update one.
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
pub async fn delete_with_retry(
    email_queue: &crate::repository::EmailQueueDb,
    id: i64,
) -> AppResult<()> {
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

    /// The schedule must widen, and never ask for a retry sooner than the next
    /// poll could deliver it. A zero or shrinking wait would quietly turn the
    /// widening schedule back into a retry on every poll.
    #[test]
    fn the_retry_waits_widen_and_none_is_shorter_than_a_poll() {
        let poll_seconds = POLL_INTERVAL.as_secs() as i64;
        assert!(RETRY_DELAYS_SECONDS[0] >= poll_seconds);
        assert!(
            RETRY_DELAYS_SECONDS
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "each wait must be longer than the one before: {RETRY_DELAYS_SECONDS:?}"
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
