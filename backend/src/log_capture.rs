//! Centralized capture of warn/error log messages into the database.
//!
//! A [`DbLogLayer`] is attached to the global tracing subscriber (main.rs), so
//! every `tracing::warn!` / `tracing::error!` emitted anywhere in the backend
//! is captured here — no call-site changes needed. Because the tracing
//! pipeline is synchronous while database writes are async, captured records
//! are handed to a background writer task over a bounded channel:
//!
//!   tracing event → DbLogLayer::on_event → channel → run_writer → app_logs
//!
//! The channel is bounded and uses `try_send`, so logging never blocks and a
//! stalled database cannot grow memory without limit (excess records are
//! dropped, but still reach stdout via the fmt layer).
//!
//! The level is a promise to the admins, because only errors alert them:
//!
//! - `error!` — something the app was supposed to do did not happen or went
//!   wrong (an upload, a reminder run, a cleanup, a mail, a holiday fetch…).
//!   Every captured error is raised to the opted-in admins, in the app and by
//!   email (see [`raises_admin_alert`]).
//! - `warn!` — worth a line in the System Log, but the app still did its job:
//!   a lockout working as intended, a fallback that took over, a conflict the
//!   user was shown. Warnings never alert anybody, so anything that impairs
//!   the app's regular function must not be logged as one.
//!
//! A call site that raises its own, more specific alert (through
//! `services::notifications::enqueue_error`) still logs at error level, and
//! marks the event with the field `admin_notified = true` so the admins are
//! not told twice.

use crate::db::DatabasePool;
use crate::repository::AppLogDb;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// Tracing target used by the writer task itself. The layer skips events with
/// this target so a failing database write can never feed back into the
/// channel and loop forever (it still appears on stdout via the fmt layer).
pub const WRITER_TARGET: &str = "zerf::log_capture";

/// Tracing target sqlx's own query logger uses for every "query took Nms"
/// event (see `sqlx-core`'s `QueryLogger::finish`), including the "slow
/// statement: execution time exceeded alert threshold" message once a query
/// crosses its 1s default threshold. It is always a driver-level timing
/// diagnostic, never an application concern — app code only ever logs under
/// `zerf::*` targets — so it is excluded from the admin-facing System Log.
/// It still reaches stdout via the fmt layer in main.rs.
const SQLX_QUERY_TARGET: &str = "sqlx::query";

/// Upper bound on buffered records while the writer is busy or the pool is
/// not up yet (records emitted during startup are held here until `run_writer`
/// starts draining).
const CHANNEL_CAPACITY: usize = 1024;

/// Very long messages are cut before persisting; the log table is a
/// diagnostic aid, not a blob store.
const MAX_MESSAGE_BYTES: usize = 8192;

/// One captured warn/error event, ready to be persisted.
pub struct CapturedLogRecord {
    pub level: &'static str,
    pub message: String,
    pub target: String,
    pub fields: Option<serde_json::Value>,
    /// Captured at event time — persisting happens later on the writer task.
    pub occurred_at: DateTime<Utc>,
}

/// Create the capture layer and the receiving end for the writer task.
pub fn channel() -> (DbLogLayer, mpsc::Receiver<CapturedLogRecord>) {
    let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    (DbLogLayer { sender }, receiver)
}

pub struct DbLogLayer {
    sender: mpsc::Sender<CapturedLogRecord>,
}

impl<S: Subscriber> Layer<S> for DbLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        // Defensive re-check: the layer is composed with a WARN level filter
        // in main.rs, but must never rely on that for correctness.
        if *metadata.level() > Level::WARN
            || metadata.target() == WRITER_TARGET
            || metadata.target() == SQLX_QUERY_TARGET
        {
            return;
        }

        let mut collector = FieldCollector::default();
        event.record(&mut collector);

        let record = CapturedLogRecord {
            level: if *metadata.level() == Level::ERROR {
                "error"
            } else {
                "warn"
            },
            message: truncate_at_char_boundary(collector.message),
            target: metadata.target().to_string(),
            fields: if collector.fields.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(collector.fields))
            },
            occurred_at: Utc::now(),
        };
        // Never block the logging path; drop the record if the buffer is full.
        let _ = self.sender.try_send(record);
    }
}

/// Extracts the `message` field plus any additional structured fields from a
/// tracing event. All values are stored as display text — the log page only
/// ever renders them, so typed JSON would add complexity for nothing.
#[derive(Default)]
struct FieldCollector {
    message: String,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl FieldCollector {
    fn set(&mut self, field: &Field, text: String) {
        if field.name() == "message" {
            self.message = text;
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(text));
        }
    }
}

impl Visit for FieldCollector {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field, value.to_string());
    }

    // Covers every non-string type (numbers, bools, errors, …).
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.set(field, format!("{value:?}"));
    }
}

fn truncate_at_char_boundary(mut message: String) -> String {
    if message.len() > MAX_MESSAGE_BYTES {
        let mut cut = MAX_MESSAGE_BYTES;
        while !message.is_char_boundary(cut) {
            cut -= 1;
        }
        message.truncate(cut);
        message.push('…');
    }
    message
}

/// Background task: drain captured records into the `app_logs` table.
///
/// Bounds (1000 rows / 365 days) are enforced by a daily cleanup pass
/// (main.rs), the same pattern used for every other prunable table in this
/// codebase (audit_log, notifications, reopen_requests) — not per-write, to
/// avoid an extra DELETE scan on every single insert during a warning burst.
/// Error logs from the notification/email/log subsystems must NOT spawn admin
/// notifications: a delivery failure there would otherwise feed back into a new
/// notification and loop. Their events still reach `app_logs` and stdout.
///
/// The email queue worker is the one subsystem whose failures admins must
/// hear about anyway. It raises that alert explicitly instead
/// (`background::email_queue::report_failed_delivery`), which lets it skip the
/// alert mails themselves; do not drop `zerf::email` from this list to get the
/// same result automatically, or a failing alert mail would spawn another.
fn target_is_notification_subsystem(target: &str) -> bool {
    target == WRITER_TARGET
        || target.starts_with("zerf::notifications")
        || target.starts_with("zerf::email")
        || target.starts_with("zerf::error_notify")
}

/// Whether a captured event becomes an admin error notification: every error,
/// except those from the delivery subsystems (loop guard, see above) and those
/// whose call site already raised a dedicated alert (`admin_notified = true`).
/// Warnings are logged but never notified.
fn raises_admin_alert(record: &CapturedLogRecord) -> bool {
    let already_notified = record
        .fields
        .as_ref()
        .and_then(|fields| fields.get("admin_notified"))
        .is_some_and(|value| value == "true");
    record.level == "error"
        && !target_is_notification_subsystem(&record.target)
        && !already_notified
}

/// Stable dedupe key derived from an event's target + message, so repeat
/// occurrences re-alert (pinned upsert) instead of piling up duplicate rows.
fn error_notification_dedupe_key(target: &str, message: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{target}\n{message}").as_bytes());
    format!("app_error_{}", &hex::encode(digest)[..16])
}

pub async fn run_writer(pool: DatabasePool, mut receiver: mpsc::Receiver<CapturedLogRecord>) {
    let db = AppLogDb::new(pool.clone());
    let error_queue = crate::repository::ErrorNotificationQueueDb::new(pool.clone());
    while let Some(record) = receiver.recv().await {
        // Turn error-level events into admin error notifications by enqueueing
        // them for the async worker (see `raises_admin_alert` for the
        // exceptions). Warnings are logged but never notified.
        if raises_admin_alert(&record) {
            let dedupe = error_notification_dedupe_key(&record.target, &record.message);
            // The queue stores fully-rendered text (matching every other
            // notification producer in the app), so the title is translated
            // here at enqueue time using the app's configured UI language. The
            // source records that language for the later email wrapper too.
            // The raw log message itself is intentionally left untranslated —
            // like every other tracing::warn!/error! call in the codebase, it
            // is a technical diagnostic string, not user-facing notification
            // copy, even though the System Log page also displays it to admins.
            let language = crate::i18n::load_ui_language(&pool)
                .await
                .unwrap_or_default();
            let text = crate::i18n::technical_error_text(&language, record.message.clone());
            let source = format!("app:{}", language.code());
            if let Err(err) = error_queue
                .enqueue(Some(&dedupe), &text.title, Some(&text.body), &source)
                .await
            {
                tracing::error!(target: WRITER_TARGET, "failed to enqueue error notification: {err}");
            }
        }
        if let Err(err) = db
            .insert(
                record.level,
                &record.message,
                &record.target,
                record.fields,
                record.occurred_at,
            )
            .await
        {
            // WRITER_TARGET keeps this out of the capture layer (no feedback
            // loop); the fmt layer still prints it to stdout.
            tracing::error!(target: WRITER_TARGET, "failed to persist log entry: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::prelude::*;

    fn capture_events(emit: impl FnOnce()) -> Vec<CapturedLogRecord> {
        let (layer, mut receiver) = channel();
        let subscriber = tracing_subscriber::registry().with(layer.with_filter(LevelFilter::WARN));
        tracing::subscriber::with_default(subscriber, emit);

        let mut records = Vec::new();
        while let Ok(record) = receiver.try_recv() {
            records.push(record);
        }
        records
    }

    #[test]
    fn captures_warn_and_error_but_not_info() {
        let records = capture_events(|| {
            tracing::info!("informational, must not be captured");
            tracing::warn!("something odd");
            tracing::error!("something broke");
        });

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].level, "warn");
        assert_eq!(records[0].message, "something odd");
        assert_eq!(records[1].level, "error");
        assert_eq!(records[1].message, "something broke");
    }

    #[test]
    fn captures_structured_fields_and_target() {
        let records = capture_events(|| {
            tracing::warn!(target: "zerf::email", user_id = 7_i64, retry = true, "delivery failed");
        });

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].target, "zerf::email");
        let fields = records[0].fields.as_ref().expect("fields captured");
        assert_eq!(fields["user_id"], serde_json::json!("7"));
        assert_eq!(fields["retry"], serde_json::json!("true"));
    }

    #[test]
    fn skips_writer_target_to_prevent_feedback_loops() {
        let records = capture_events(|| {
            tracing::error!(target: WRITER_TARGET, "db write failed");
        });
        assert!(records.is_empty());
    }

    #[test]
    fn skips_sqlx_query_target_as_driver_noise() {
        let records = capture_events(|| {
            tracing::warn!(
                target: "sqlx::query",
                "slow statement: execution time exceeded alert threshold"
            );
        });
        assert!(records.is_empty());
    }

    #[test]
    fn only_errors_raise_an_admin_alert() {
        let records = capture_events(|| {
            tracing::warn!(target: "zerf::report_upload", "noted, nothing broke");
            tracing::error!(target: "zerf::report_upload", "upload failed");
        });
        assert_eq!(records.len(), 2);
        assert!(!raises_admin_alert(&records[0]), "warnings never alert");
        assert!(raises_admin_alert(&records[1]), "errors alert the admins");
    }

    #[test]
    fn errors_with_their_own_alert_are_not_raised_twice() {
        let records = capture_events(|| {
            tracing::error!(target: "zerf::report_upload", admin_notified = true, "upload blocked");
            tracing::error!(target: "zerf::report_upload", admin_notified = false, "upload failed");
        });
        assert_eq!(records.len(), 2);
        assert!(!raises_admin_alert(&records[0]));
        assert!(raises_admin_alert(&records[1]));
    }

    #[test]
    fn errors_from_the_delivery_subsystems_never_raise_an_alert() {
        let records = capture_events(|| {
            tracing::error!(target: "zerf::email_queue", "delivery failed");
            tracing::error!(target: "zerf::notifications", "insert failed");
            tracing::error!(target: "zerf::error_notify", "list pending failed");
        });
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| !raises_admin_alert(record)));
    }

    #[test]
    fn truncates_overlong_messages_at_char_boundaries() {
        // 'ü' is 2 bytes in UTF-8; an odd byte limit would split it without
        // the boundary walk.
        let long = "ü".repeat(MAX_MESSAGE_BYTES);
        let truncated = truncate_at_char_boundary(long);
        assert!(truncated.len() <= MAX_MESSAGE_BYTES + '…'.len_utf8());
        assert!(truncated.ends_with('…'));
    }
}
