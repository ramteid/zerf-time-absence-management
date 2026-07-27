//! Outbound email helper.
//!
//! Best-effort delivery via SMTP when [`SmtpConfig`] is present. All errors
//! are logged at WARN and never propagated to the calling business flow.
//! The whole feature is no-op when SMTP is not configured in admin settings.

use crate::config::SmtpConfig;
use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::sync::Arc;
use std::time::Duration;

/// A file attached to an outbound email.
pub struct EmailAttachment {
    pub filename: String,
    /// MIME type, e.g. `application/pdf`.
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Send `subject` / `body_text` to `to`. `to_name` is the recipient's display
/// name (e.g. `"Jane Doe"`); when non-empty the envelope becomes
/// `"Jane Doe" <jane@example.com>` so it renders correctly in email clients.
/// Returns immediately and runs the actual SMTP transaction in a detached
/// task. Safe to call from any async handler. When `smtp` is `None`, this is
/// a silent no-op.
pub fn send_async(
    smtp: Option<Arc<SmtpConfig>>,
    to: String,
    to_name: String,
    subject: String,
    body_text: String,
) {
    let Some(cfg) = smtp else { return };
    if to.trim().is_empty() {
        return;
    }
    tokio::spawn(async move {
        if let Err(e) = send_now(&cfg, &to, &to_name, &subject, &body_text, None).await {
            tracing::warn!(target:"zerf::email", "failed to send email to {}: {}", to, e);
        }
    });
}

/// Send an email and wait for the SMTP transaction to finish, optionally with
/// one attached file. Unlike [`send_async`] the caller learns whether delivery
/// succeeded — required for the scheduled payroll report, which may only drop a
/// month from its queue once the message was actually accepted.
pub async fn send_with_attachment(
    cfg: &SmtpConfig,
    to: &str,
    to_name: &str,
    subject: &str,
    body_text: &str,
    attachment: EmailAttachment,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // The caller awaits this inside a background loop, so an unresponsive SMTP
    // server must fail rather than stall the loop until the process restarts.
    tokio::time::timeout(
        ATTACHMENT_SEND_TIMEOUT,
        send_now(cfg, to, to_name, subject, body_text, Some(attachment)),
    )
    .await
    .map_err(|_| -> Box<dyn std::error::Error + Send + Sync> {
        format!(
            "SMTP delivery timed out after {} seconds",
            ATTACHMENT_SEND_TIMEOUT.as_secs()
        )
        .into()
    })?
}

/// Upper bound for one awaited delivery including its attachment upload.
const ATTACHMENT_SEND_TIMEOUT: Duration = Duration::from_secs(120);

/// Test the SMTP connection by performing a NOOP command. Returns `Ok(())`
/// on success or an error describing the failure.
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
    let email = match attachment {
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
    };
    build_mailer(cfg, None)?.send(email).await?;
    Ok(())
}
