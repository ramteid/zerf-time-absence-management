//! Outbound email queue: enqueue gating on SMTP settings, oldest-first
//! draining, attempts/last_error tracking on failed delivery, and the shared
//! circuit breaker backing off once SMTP is confirmed broken.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

/// Point SMTP at a closed local port: `load_smtp_config` returns a config (so
/// the send path is reachable) while no mail can ever leave the test machine.
/// Mirrors `payroll_report::configure_unreachable_smtp`.
async fn configure_unreachable_smtp(app: &TestApp) {
    for (key, value) in [
        ("smtp_enabled", "true"),
        ("smtp_host", "127.0.0.1"),
        ("smtp_port", "1"),
        ("smtp_from", "zerf@example.com"),
        ("smtp_encryption", "none"),
    ] {
        app.state
            .db
            .settings
            .save_setting(key, value)
            .await
            .expect("configure smtp");
    }
}

async fn attempts_and_error(app: &TestApp, id: i64) -> (i32, Option<String>) {
    sqlx::query_as("SELECT attempts, last_error FROM email_queue WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.pool)
        .await
        .expect("load attempts/last_error")
}

#[tokio::test]
async fn queue_email_is_a_noop_without_smtp_configured() {
    let app = TestApp::spawn().await;

    zerf::email::queue_email(
        &app.state.db.email_queue,
        false,
        "someone@example.com",
        "Someone",
        "subject",
        "body",
    )
    .await;

    assert_eq!(
        app.state.db.email_queue.count().await.unwrap(),
        0,
        "nothing is queued while SMTP is unconfigured"
    );
    app.cleanup().await;
}

#[tokio::test]
async fn queue_email_persists_the_already_rendered_message() {
    let app = TestApp::spawn().await;

    zerf::email::queue_email(
        &app.state.db.email_queue,
        true,
        "someone@example.com",
        "Someone",
        "subject line",
        "body text",
    )
    .await;

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].to_address, "someone@example.com");
    assert_eq!(pending[0].to_name, "Someone");
    assert_eq!(pending[0].subject, "subject line");
    assert_eq!(pending[0].body_text, "body text");
    app.cleanup().await;
}

/// End-to-end: a real handler-triggered notification (new-user onboarding)
/// lands in the queue while SMTP is configured, proving the full wiring from
/// `services::notifications::deliver` through `email::queue_email`.
#[tokio::test]
async fn account_created_email_is_queued_when_smtp_is_configured() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;

    let (status, _body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"onboard@example.com","first_name":"On","last_name":"Board",
                "role":"employee","weekly_hours":39,"start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create user");

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
    assert!(
        pending
            .iter()
            .any(|e| e.to_address == "onboard@example.com"),
        "account-created email must be queued for the new user"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn failed_delivery_keeps_the_row_queued_and_records_the_error() {
    let app = TestApp::spawn().await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue("someone@example.com", "Someone", "subject", "body")
        .await
        .expect("enqueue");

    zerf::background::email_queue::process_pending(&app.state).await;

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "a failed delivery must leave the row queued, never drop it"
    );
    let (attempts, last_error) = attempts_and_error(&app, pending[0].id).await;
    assert_eq!(attempts, 1);
    assert!(last_error.is_some(), "the failure reason must be recorded");

    app.cleanup().await;
}

#[tokio::test]
async fn disabling_smtp_leaves_queued_emails_untouched() {
    let app = TestApp::spawn().await;
    // No SMTP configured at all: process_pending must not touch the queue.
    app.state
        .db
        .email_queue
        .enqueue("someone@example.com", "Someone", "subject", "body")
        .await
        .expect("enqueue");

    zerf::background::email_queue::process_pending(&app.state).await;

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
    assert_eq!(pending.len(), 1, "row must remain queued");
    let (attempts, last_error) = attempts_and_error(&app, pending[0].id).await;
    assert_eq!(attempts, 0, "no delivery was ever attempted");
    assert!(last_error.is_none());

    app.cleanup().await;
}

/// A message that already failed once must not permanently block a fresh
/// message queued after it: `list_pending` demotes previously-attempted rows
/// behind never-yet-attempted ones, so a single persistently undeliverable
/// address (e.g. a typo'd recipient) can't monopolize the circuit breaker's
/// scarce half-open retry slot and starve every healthy email behind it.
#[tokio::test]
async fn a_previously_failed_row_is_demoted_behind_a_fresh_one() {
    let app = TestApp::spawn().await;
    configure_unreachable_smtp(&app).await;

    app.state
        .db
        .email_queue
        .enqueue("stuck@example.com", "Stuck", "subject", "body")
        .await
        .expect("enqueue stuck row");
    let stuck_id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

    // One failed attempt sets `last_attempt_at` on the stuck row.
    zerf::background::email_queue::process_pending(&app.state).await;

    // A fresh row queued afterwards has never been attempted.
    app.state
        .db
        .email_queue
        .enqueue("fresh@example.com", "Fresh", "subject", "body")
        .await
        .expect("enqueue fresh row");

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(
        pending[0].to_address, "fresh@example.com",
        "the never-attempted row must be processed before the previously-failed one, \
         even though it was queued later"
    );
    assert_eq!(pending[1].id, stuck_id);

    app.cleanup().await;
}

/// Repeated failures trip the shared circuit breaker: once it opens, further
/// poll cycles stop attempting delivery altogether (no SMTP transaction, no
/// attempt-count increment) until the cooldown elapses.
#[tokio::test]
async fn repeated_failures_trip_the_circuit_breaker_and_stop_further_attempts() {
    let app = TestApp::spawn().await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue("someone@example.com", "Someone", "subject", "body")
        .await
        .expect("enqueue");
    let id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

    // Five consecutive failures open the breaker
    // (CircuitBreaker::DEFAULT_FAILURE_THRESHOLD).
    for _ in 0..5 {
        zerf::background::email_queue::process_pending(&app.state).await;
    }
    let (attempts_after_five, _) = attempts_and_error(&app, id).await;
    assert_eq!(attempts_after_five, 5);

    // The breaker is now open with a 5-minute cooldown: the next cycle must
    // not add to the attempt count, since no SMTP transaction is made at all.
    zerf::background::email_queue::process_pending(&app.state).await;
    let (attempts_after_six, _) = attempts_and_error(&app, id).await;
    assert_eq!(
        attempts_after_six, 5,
        "circuit breaker must block further attempts, not just further deliveries"
    );

    app.cleanup().await;
}
