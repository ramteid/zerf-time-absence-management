//! Outbound email helper.
//!
//! Best-effort delivery via SMTP when [`SmtpConfig`] is present. All errors
//! are logged at WARN and never propagated to the calling business flow.
//! The whole feature is no-op when SMTP is not configured in admin settings.

use crate::config::SmtpConfig;
use lettre::message::{header::ContentType, Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::sync::Arc;

/// Send `subject` / `body_text` to `to`. Returns immediately and runs the
/// actual SMTP transaction in a detached task. Safe to call from any async
/// handler.  When `smtp` is `None`, this is a silent no-op.
pub fn send_async(smtp: Option<Arc<SmtpConfig>>, to: String, subject: String, body_text: String) {
    let Some(cfg) = smtp else { return };
    if to.trim().is_empty() {
        return;
    }
    tokio::spawn(async move {
        if let Err(e) = send_now(&cfg, &to, &subject, &body_text).await {
            tracing::warn!(target:"zerf::email", "failed to send email to {}: {}", to, e);
        }
    });
}

/// Test the SMTP connection by performing a NOOP command. Returns `Ok(())`
/// on success or an error describing the failure.
pub async fn test_connection(
    cfg: &SmtpConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    let mailer: AsyncSmtpTransport<Tokio1Executor> = builder
        .timeout(Some(std::time::Duration::from_secs(10)))
        .build();
    mailer.test_connection().await?;
    Ok(())
}

async fn send_now(
    cfg: &SmtpConfig,
    to: &str,
    subject: &str,
    body_text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let from: Mailbox = cfg.from.parse()?;
    let to_box: Mailbox = to.parse()?;
    let email = Message::builder()
        .from(from)
        .to(to_box)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body_text.to_string())?;

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
    let mailer = builder.build();
    mailer.send(email).await?;
    Ok(())
}
