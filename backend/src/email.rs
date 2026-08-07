//! Outbound email: the one place in the app that talks to SMTP.
//!
//! There are two producer paths:
//!
//! * [`queue_email`] — the normal path used by every notification-driven
//!   email (password resets, absence decisions, reminders, ...). It durably
//!   persists the already-rendered message to the `email_queue` table and
//!   returns immediately; [`crate::background::email_queue`] drains that
//!   table on a 2-minute poll and only deletes a row once SMTP confirmed
//!   delivery. A message that keeps failing simply stays queued forever —
//!   nothing is silently lost to a transient SMTP outage anymore.
//! * [`send_with_attachment`] — used only by the monthly payroll report,
//!   which already has its own period-keyed retry queue
//!   (`payroll_report_queue`) with "stays queued until confirmed sent"
//!   semantics. It stays a synchronous, awaited call so its caller learns
//!   the outcome immediately, but now shares the same [`CircuitBreaker`] as
//!   the queue drain so a broadly down SMTP server doesn't get hammered by
//!   both paths independently.
//!
//! Both paths funnel their actual SMTP transaction through the same
//! breaker-guarded senders in this module — that is the "central point"
//! where emails are sent. The whole feature is a no-op when SMTP is not
//! enabled/configured in admin settings ([`crate::repository::SettingsDb::load_smtp_config`]).

use crate::config::SmtpConfig;
use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A file attached to an outbound email.
pub struct EmailAttachment {
    pub filename: String,
    /// MIME type, e.g. `application/pdf`.
    pub content_type: String,
    pub bytes: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Circuit breaker
// ---------------------------------------------------------------------------

/// Guards every real SMTP attempt so a longer-lasting outage (bad
/// credentials, blocked port, provider downtime) stops hammering the server
/// once it is clearly failing, instead of retrying every single queued
/// message on every 2-minute poll.
///
/// Classic three-state breaker, implemented without an explicit `enum`:
/// `opened_at == None` is Closed; `Some(t)` within the cooldown is Open;
/// `Some(t)` past the cooldown grants exactly one Half-Open trial (and
/// immediately rearms the cooldown so a concurrent caller — the payroll
/// report can run alongside the queue drain — can't also get a trial slot
/// before the first one resolves).
pub struct CircuitBreaker {
    inner: Mutex<CircuitBreakerState>,
    failure_threshold: u32,
    cooldown: Duration,
}

struct CircuitBreakerState {
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    /// Consecutive SMTP failures (across every guarded sender, queue drain
    /// and payroll report alike) before the breaker opens.
    const DEFAULT_FAILURE_THRESHOLD: u32 = 5;
    /// How long the breaker stays open before allowing one half-open trial.
    const DEFAULT_COOLDOWN: Duration = Duration::from_secs(5 * 60);

    pub fn new() -> Self {
        Self::with_params(Self::DEFAULT_FAILURE_THRESHOLD, Self::DEFAULT_COOLDOWN)
    }

    fn with_params(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            inner: Mutex::new(CircuitBreakerState {
                consecutive_failures: 0,
                opened_at: None,
            }),
            failure_threshold,
            cooldown,
        }
    }

    /// Whether a delivery attempt may proceed right now.
    fn allow_attempt(&self) -> bool {
        let mut state = self.inner.lock().unwrap();
        match state.opened_at {
            None => true,
            Some(opened_at) => {
                if opened_at.elapsed() >= self.cooldown {
                    // Half-open: grant this one trial and rearm the cooldown
                    // clock now so a racing caller sees the breaker as still
                    // open until this trial resolves.
                    state.opened_at = Some(Instant::now());
                    true
                } else {
                    false
                }
            }
        }
    }

    fn record_success(&self) {
        let mut state = self.inner.lock().unwrap();
        state.consecutive_failures = 0;
        state.opened_at = None;
    }

    fn record_failure(&self) {
        let mut state = self.inner.lock().unwrap();
        state.consecutive_failures += 1;
        if state.consecutive_failures >= self.failure_threshold {
            state.opened_at = Some(Instant::now());
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of a breaker-guarded delivery attempt.
#[derive(Debug)]
pub enum GuardedSendError {
    /// The breaker is currently open; no SMTP attempt was made at all.
    CircuitOpen,
    /// An SMTP attempt was made and it failed.
    Smtp(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for GuardedSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardedSendError::CircuitOpen => {
                write!(f, "email circuit breaker is open (repeated SMTP failures)")
            }
            GuardedSendError::Smtp(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GuardedSendError {}

// ---------------------------------------------------------------------------
// Producer: queue a message (the path almost every email takes)
// ---------------------------------------------------------------------------

/// Queue `subject`/`body_text` for delivery to `to` (`to_name` may be
/// empty). `smtp_configured` reflects
/// `SettingsDb::load_smtp_config().is_some()` at the call site: emails are
/// only ever added to the queue while SMTP is enabled and fully configured —
/// nothing is queued otherwise, matching the previous silent no-op when SMTP
/// was unset. A message already queued when SMTP is later disabled is left
/// in place untouched; only enqueueing is gated, not draining.
pub async fn queue_email(
    email_queue: &crate::repository::EmailQueueDb,
    smtp_configured: bool,
    to: &str,
    to_name: &str,
    subject: &str,
    body_text: &str,
) {
    if !smtp_configured || to.trim().is_empty() {
        return;
    }
    if let Err(e) = email_queue.enqueue(to, to_name, subject, body_text).await {
        tracing::warn!(target: "zerf::email", "failed to queue email to {to}: {e}");
    }
}

// ---------------------------------------------------------------------------
// Central, breaker-guarded senders
// ---------------------------------------------------------------------------

/// Send one queued email, gated by `breaker`. Returns
/// [`GuardedSendError::CircuitOpen`] without attempting SMTP when the
/// breaker is open, so the caller (the queue-drain worker) can tell "not
/// attempted" apart from "attempted and failed" — only the latter should
/// count against the message's own attempt counter.
pub async fn send_queued(
    breaker: &CircuitBreaker,
    cfg: &SmtpConfig,
    to: &str,
    to_name: &str,
    subject: &str,
    body_text: &str,
) -> Result<(), GuardedSendError> {
    guarded_send(breaker, QUEUED_EMAIL_SEND_TIMEOUT, || {
        send_now(cfg, to, to_name, subject, body_text, None)
    })
    .await
}

/// Send an email to one or more equal recipients (all placed in the `To`
/// header — no primary/CC distinction) with one attached file, gated by
/// `breaker`, and wait for the SMTP transaction to finish. Used only by the
/// scheduled payroll report, which may only drop a month from its own queue
/// once the message was actually accepted.
pub async fn send_with_attachment(
    breaker: &CircuitBreaker,
    cfg: &SmtpConfig,
    to: &[String],
    subject: &str,
    body_text: &str,
    attachment: EmailAttachment,
) -> Result<(), GuardedSendError> {
    guarded_send(breaker, ATTACHMENT_SEND_TIMEOUT, || {
        send_now_multi(cfg, to, subject, body_text, Some(attachment))
    })
    .await
}

/// Shared breaker-and-timeout wrapper around one SMTP attempt. Both queued
/// callers await this inside a background loop (the queue-drain worker and
/// the payroll report scheduler), so an unresponsive SMTP server — one that
/// accepts the connection but never finishes the conversation — must fail
/// after `timeout` rather than hang the loop forever. Without this bound a
/// single stuck server would silently stop *all* future email delivery for
/// the lifetime of the process, which is strictly worse than the fire-and-
/// forget design this module replaced.
async fn guarded_send<F, Fut>(
    breaker: &CircuitBreaker,
    timeout: Duration,
    send: F,
) -> Result<(), GuardedSendError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>>,
{
    if !breaker.allow_attempt() {
        return Err(GuardedSendError::CircuitOpen);
    }
    match tokio::time::timeout(timeout, send()).await {
        Ok(Ok(())) => {
            breaker.record_success();
            Ok(())
        }
        Ok(Err(e)) => {
            breaker.record_failure();
            Err(GuardedSendError::Smtp(e))
        }
        Err(_) => {
            breaker.record_failure();
            let timeout_err: Box<dyn std::error::Error + Send + Sync> =
                format!("SMTP delivery timed out after {} seconds", timeout.as_secs()).into();
            Err(GuardedSendError::Smtp(timeout_err))
        }
    }
}

/// Upper bound for one awaited plain-text queued delivery (no attachment).
const QUEUED_EMAIL_SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound for one awaited delivery including its attachment upload.
const ATTACHMENT_SEND_TIMEOUT: Duration = Duration::from_secs(120);

/// Test the SMTP connection by performing a NOOP command. Returns `Ok(())`
/// on success or an error describing the failure. Deliberately bypasses the
/// circuit breaker: this is the admin diagnosing a *candidate* configuration
/// (often while fixing the very problem that opened the breaker), and it
/// never sends a real message, so it must neither be blocked by breaker
/// state nor feed back into it.
pub async fn test_connection(
    cfg: &SmtpConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    build_mailer(cfg, Some(Duration::from_secs(10)))?
        .test_connection()
        .await?;
    Ok(())
}

fn build_mailer(
    cfg: &SmtpConfig,
    timeout: Option<Duration>,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, Box<dyn std::error::Error + Send + Sync>> {
    let mut builder = match cfg.encryption.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
            .port(cfg.port)
            .tls(Tls::Wrapper(TlsParameters::new(cfg.host.clone())?)),
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
            .port(cfg.port)
            .tls(Tls::Required(TlsParameters::new(cfg.host.clone())?)),
        _ => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host).port(cfg.port),
    };
    if let (Some(u), Some(p)) = (&cfg.username, &cfg.password) {
        builder = builder.credentials(Credentials::new(u.clone(), p.clone()));
    }
    Ok(builder.timeout(timeout).build())
}

async fn send_now(
    cfg: &SmtpConfig,
    to: &str,
    to_name: &str,
    subject: &str,
    body_text: &str,
    attachment: Option<EmailAttachment>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let from: Mailbox = cfg.from.parse()?;
    // Build a properly quoted RFC 5322 display-name when a name is provided.
    let to_box: Mailbox = if to_name.trim().is_empty() {
        to.parse()?
    } else {
        let quoted_name = to_name.trim().replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\" <{}>", quoted_name, to).parse()?
    };
    let builder = Message::builder().from(from).to(to_box).subject(subject);
    let email = finish_message(builder, body_text, attachment)?;
    build_mailer(cfg, None)?.send(email).await?;
    Ok(())
}

/// Like `send_now`, but addresses every entry in `to` as an equal recipient
/// in a single message's `To` header (no per-recipient display name — repeated
/// `.to()` calls append to the same header instead of overwriting it).
async fn send_now_multi(
    cfg: &SmtpConfig,
    to: &[String],
    subject: &str,
    body_text: &str,
    attachment: Option<EmailAttachment>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let from: Mailbox = cfg.from.parse()?;
    let mut builder = Message::builder().from(from).subject(subject);
    for address in to {
        builder = builder.to(address.parse()?);
    }
    let email = finish_message(builder, body_text, attachment)?;
    build_mailer(cfg, None)?.send(email).await?;
    Ok(())
}

/// Render the plain-text body, plus the attachment as a second part when
/// present, onto an already-addressed message builder.
fn finish_message(
    builder: lettre::message::MessageBuilder,
    body_text: &str,
    attachment: Option<EmailAttachment>,
) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
    Ok(match attachment {
        // Plain text stays a single-part message so nothing changes for the
        // existing notification emails.
        None => builder
            .header(ContentType::TEXT_PLAIN)
            .body(body_text.to_string())?,
        Some(file) => {
            let content_type = ContentType::parse(&file.content_type)
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").unwrap());
            builder.multipart(
                MultiPart::mixed()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(body_text.to_string()),
                    )
                    .singlepart(
                        Attachment::new(file.filename.clone()).body(file.bytes, content_type),
                    ),
            )?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the assumption `send_now_multi` relies on: repeated `.to()`
    /// calls append to the same `To` header instead of overwriting it, so
    /// every payroll report recipient is addressed as an equal `To`
    /// recipient in one message rather than only the last one winning.
    #[test]
    fn repeated_to_calls_join_into_one_header_with_every_recipient() {
        let message = Message::builder()
            .from("from@example.com".parse::<Mailbox>().unwrap())
            .to("a@example.com".parse::<Mailbox>().unwrap())
            .to("b@example.com".parse::<Mailbox>().unwrap())
            .subject("subject")
            .header(ContentType::TEXT_PLAIN)
            .body("body".to_string())
            .unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        let to_line = raw
            .lines()
            .find(|line| line.starts_with("To:"))
            .expect("To header present");
        assert!(to_line.contains("a@example.com"), "To header: {to_line}");
        assert!(to_line.contains("b@example.com"), "To header: {to_line}");
        assert_eq!(
            raw.lines().filter(|line| line.starts_with("To:")).count(),
            1,
            "recipients share a single To header, not one each"
        );
    }

    #[test]
    fn breaker_starts_closed() {
        let breaker = CircuitBreaker::with_params(3, Duration::from_secs(60));
        assert!(breaker.allow_attempt());
    }

    #[test]
    fn breaker_opens_after_reaching_the_failure_threshold() {
        let breaker = CircuitBreaker::with_params(3, Duration::from_secs(60));
        breaker.record_failure();
        assert!(breaker.allow_attempt(), "below threshold: still closed");
        breaker.record_failure();
        assert!(breaker.allow_attempt(), "still below threshold");
        breaker.record_failure();
        assert!(
            !breaker.allow_attempt(),
            "third consecutive failure hits the threshold and opens the breaker"
        );
    }

    #[test]
    fn breaker_stays_open_until_the_cooldown_elapses() {
        let breaker = CircuitBreaker::with_params(1, Duration::from_millis(50));
        breaker.record_failure();
        assert!(!breaker.allow_attempt(), "open immediately after tripping");
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            breaker.allow_attempt(),
            "cooldown elapsed: half-open trial granted"
        );
    }

    #[test]
    fn breaker_half_open_trial_blocks_a_second_concurrent_trial() {
        let breaker = CircuitBreaker::with_params(1, Duration::from_millis(50));
        breaker.record_failure();
        std::thread::sleep(Duration::from_millis(80));
        assert!(breaker.allow_attempt(), "first trial granted");
        assert!(
            !breaker.allow_attempt(),
            "cooldown was rearmed by the first trial; a second caller must wait"
        );
    }

    #[test]
    fn breaker_success_resets_failure_count_and_closes() {
        let breaker = CircuitBreaker::with_params(2, Duration::from_secs(60));
        breaker.record_failure();
        breaker.record_success();
        breaker.record_failure();
        assert!(
            breaker.allow_attempt(),
            "success reset the streak, so a single subsequent failure must not open it"
        );
    }

    #[test]
    fn breaker_reopens_when_the_half_open_trial_fails() {
        let breaker = CircuitBreaker::with_params(1, Duration::from_millis(50));
        breaker.record_failure();
        std::thread::sleep(Duration::from_millis(80));
        assert!(breaker.allow_attempt(), "half-open trial granted");
        breaker.record_failure();
        assert!(
            !breaker.allow_attempt(),
            "failed trial reopens the breaker with a fresh cooldown"
        );
    }
}
