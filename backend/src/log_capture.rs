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
        if *metadata.level() > Level::WARN || metadata.target() == WRITER_TARGET {
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
fn target_is_notification_subsystem(target: &str) -> bool {
    target == WRITER_TARGET
        || target.starts_with("zerf::notifications")
        || target.starts_with("zerf::email")
        || target.starts_with("zerf::error_notify")
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
        // them for the async worker — unless they originate from the delivery
        // subsystems (loop guard). Warnings are logged but never notified.
        if record.level == "error" && !target_is_notification_subsystem(&record.target) {
            let dedupe = error_notification_dedupe_key(&record.target, &record.message);
            // The queue stores fully-rendered text (matching every other
            // notification producer in the app), so the title is translated
            // here at enqueue time using the app's configured UI language.
            // The raw log message itself is intentionally left untranslated —
            // like every other tracing::warn!/error! call in the codebase, it
            // is a technical diagnostic string, not user-facing notification
            // copy, even though the System Log page also displays it to admins.
            let language = crate::i18n::load_ui_language(&pool).await.unwrap_or_default();
            let title = crate::i18n::translate(&language, "error_notification_title", &[]);
            if let Err(err) = error_queue
                .enqueue(Some(&dedupe), &title, Some(&record.message), "app")
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
        let subscriber =
            tracing_subscriber::registry().with(layer.with_filter(LevelFilter::WARN));
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
    fn truncates_overlong_messages_at_char_boundaries() {
        // 'ü' is 2 bytes in UTF-8; an odd byte limit would split it without
        // the boundary walk.
        let long = "ü".repeat(MAX_MESSAGE_BYTES);
        let truncated = truncate_at_char_boundary(long);
        assert!(truncated.len() <= MAX_MESSAGE_BYTES + '…'.len_utf8());
        assert!(truncated.ends_with('…'));
    }
}
