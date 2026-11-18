//! Outbound email queue: enqueue gating on SMTP settings, oldest-first
//! draining, attempts/last_error tracking on failed delivery, the widening
//! retry schedule, giving a mail up and reporting only that final failure, and
//! the shared circuit breaker backing off once SMTP is confirmed broken.

use reqwest::StatusCode;
use serde_json::json;
use zerf::background::email_queue::{GIVEN_UP_RETENTION_DAYS, MAX_ATTEMPTS, RETRY_DELAYS_SECONDS};

use crate::common::{TestApp, TestClient};
use crate::helpers::*;

/// The queued mails the worker may attempt right now: never attempted, or
/// failed before and done waiting.
async fn due_rows(app: &TestApp) -> Vec<zerf::repository::EmailQueueEntry> {
    app.state
        .db
        .email_queue
        .list_due(100, &RETRY_DELAYS_SECONDS)
        .await
        .expect("list due mails")
}

/// Pretend the waiting time after the last failed attempt has passed, so the
/// next `process_pending` tries the mail again. Two days is longer than every
/// wait in the schedule.
async fn make_retry_due(app: &TestApp, id: i64) {
    sqlx::query(
        "UPDATE email_queue SET last_attempt_at = CURRENT_TIMESTAMP - INTERVAL '2 days' \
         WHERE id = $1",
    )
    .bind(id)
    .execute(&app.state.pool)
    .await
    .expect("make the retry due");
}

/// Pretend the mail has already failed `failed_attempts` times and that its
/// wait is over. With `MAX_ATTEMPTS - 1` the next failing attempt is the final
/// failure. Jumping there in one step also keeps the circuit breaker, which
/// opens after five failures in a row, out of the way.
async fn fast_forward(app: &TestApp, id: i64, failed_attempts: i32) {
    sqlx::query(
        "UPDATE email_queue SET attempts = $2, \
         last_attempt_at = CURRENT_TIMESTAMP - INTERVAL '2 days' WHERE id = $1",
    )
    .bind(id)
    .bind(failed_attempts)
    .execute(&app.state.pool)
    .await
    .expect("fast-forward the attempt counter");
}

async fn is_given_up(app: &TestApp, id: i64) -> bool {
    sqlx::query_scalar("SELECT given_up_at IS NOT NULL FROM email_queue WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.pool)
        .await
        .expect("load given_up_at")
}

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
        "test",
        "subject",
        "body",
        None,
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
        "test_kind",
        "subject line",
        "body text",
        None,
    )
    .await;

    let pending = due_rows(&app).await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].to_address, "someone@example.com");
    assert_eq!(pending[0].to_name, "Someone");
    assert_eq!(pending[0].kind, "test_kind");
    assert_eq!(pending[0].subject, "subject line");
    assert_eq!(pending[0].body_text, "body text");
    assert_eq!(
        pending[0].reply_to_address, "",
        "without a reply-to person the mail carries no Reply-To"
    );
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

    let pending = due_rows(&app).await;
    let queued = pending
        .iter()
        .find(|e| e.to_address == "onboard@example.com")
        .expect("account-created email must be queued for the new user");
    assert_eq!(
        queued.kind, "account_created",
        "the notification kind must travel with the queued mail"
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
        .enqueue(
            "someone@example.com",
            "Someone",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    zerf::background::email_queue::process_pending(&app.state).await;

    assert_eq!(
        app.state.db.email_queue.count().await.unwrap(),
        1,
        "a failed delivery must leave the row queued, never drop it"
    );
    let (attempts, last_error) = attempts_and_error(&app, id).await;
    assert_eq!(attempts, 1);
    assert!(last_error.is_some(), "the failure reason must be recorded");
    assert!(
        !is_given_up(&app, id).await,
        "the first failure is not the end: more attempts are scheduled"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn disabling_smtp_leaves_queued_emails_untouched() {
    let app = TestApp::spawn().await;
    // No SMTP configured at all: process_pending must not touch the queue.
    app.state
        .db
        .email_queue
        .enqueue(
            "someone@example.com",
            "Someone",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");

    zerf::background::email_queue::process_pending(&app.state).await;

    let pending = due_rows(&app).await;
    assert_eq!(pending.len(), 1, "row must remain queued");
    let (attempts, last_error) = attempts_and_error(&app, pending[0].id).await;
    assert_eq!(attempts, 0, "no delivery was ever attempted");
    assert!(last_error.is_none());

    app.cleanup().await;
}

/// A message that already failed once must not permanently block a fresh
/// message queued after it: `list_due` demotes previously-attempted rows
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
        .enqueue(
            "stuck@example.com",
            "Stuck",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue stuck row");
    let stuck_id = due_rows(&app).await[0].id;

    // One failed attempt sets `last_attempt_at` on the stuck row; its wait is
    // over by the time the fresh row below arrives.
    zerf::background::email_queue::process_pending(&app.state).await;
    make_retry_due(&app, stuck_id).await;

    // A fresh row queued afterwards has never been attempted.
    app.state
        .db
        .email_queue
        .enqueue(
            "fresh@example.com",
            "Fresh",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue fresh row");

    let pending = due_rows(&app).await;
    assert_eq!(pending.len(), 2);
    assert_eq!(
        pending[0].to_address, "fresh@example.com",
        "the never-attempted row must be processed before the previously-failed one, \
         even though it was queued later"
    );
    assert_eq!(pending[1].id, stuck_id);

    app.cleanup().await;
}

/// `delete_with_retry` is what `process_pending` calls once SMTP confirms
/// delivery. Verifies the happy path actually removes the row (the only
/// path integration tests can drive, since faking a real SMTP accept isn't
/// possible here) — the retry-on-failure behavior itself is exercised by
/// construction: `DELETE ... WHERE id=$1` is idempotent, so a retry loop
/// around it can never double-delete or corrupt state, only paper over a
/// transient failure that would otherwise cause the same email to be
/// resent.
#[tokio::test]
async fn delete_with_retry_removes_the_row() {
    let app = TestApp::spawn().await;
    app.state
        .db
        .email_queue
        .enqueue(
            "someone@example.com",
            "Someone",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    zerf::background::email_queue::delete_with_retry(&app.state.db.email_queue, id)
        .await
        .expect("delete succeeds");

    assert_eq!(
        app.state.db.email_queue.count().await.unwrap(),
        0,
        "the row must be gone after a successful delete"
    );

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
        .enqueue(
            "someone@example.com",
            "Someone",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    // Five consecutive failures open the breaker
    // (CircuitBreaker::DEFAULT_FAILURE_THRESHOLD). The mail's wait is skipped
    // each time, so only the breaker can stop the sixth attempt below.
    for _ in 0..5 {
        zerf::background::email_queue::process_pending(&app.state).await;
        make_retry_due(&app, id).await;
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

/// Opt the seeded admin in to technical error alerts and return their id.
async fn opt_in_admin(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "UPDATE users SET receives_error_notifications = TRUE \
         WHERE id = (SELECT MIN(id) FROM users WHERE role = 'admin') RETURNING id",
    )
    .fetch_one(&app.state.pool)
    .await
    .expect("opt the admin in")
}

/// Events waiting for fan-out to the admins.
async fn queued_alerts(app: &TestApp) -> Vec<zerf::repository::ErrorNotificationEntry> {
    app.state
        .db
        .error_queue
        .list_pending(10)
        .await
        .expect("list queued alerts")
}

/// Throw the queued alerts away without fanning them out. Fanning out would
/// queue an alert mail for the admin, and that mail's own failed attempts
/// would count towards the circuit breaker, which opens after five failures
/// in a row and would stop a test that needs more attempts than that.
async fn discard_queued_alerts(app: &TestApp) {
    for alert in queued_alerts(app).await {
        app.state
            .db
            .error_queue
            .delete_entry(alert.id)
            .await
            .expect("discard alert");
    }
}

/// A mail that is given up must reach the opted-in admins, naming the
/// recipient, the subject, the number of attempts and the server's reason — and
/// never the mail body, which can hold a temporary password or a reset link.
#[tokio::test]
async fn a_given_up_delivery_is_reported_to_the_opted_in_admins() {
    let app = TestApp::spawn().await;
    let admin_id = opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue(
            "jurij@example.com",
            "Jurij",
            "admin_password_reset",
            "Your temporary password",
            "TOP-SECRET-TEMPORARY-PASSWORD",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    // The final attempt fails like the ones before it.
    fast_forward(&app, id, MAX_ATTEMPTS - 1).await;
    zerf::background::email_queue::process_pending(&app.state).await;

    let alerts = queued_alerts(&app).await;
    assert_eq!(alerts.len(), 1, "one failed mail raises exactly one alert");
    assert_eq!(
        alerts[0].dedupe_key.as_deref(),
        Some(format!("email_delivery_failed_{id}").as_str())
    );
    assert_eq!(alerts[0].title, "Email could not be delivered");
    let body = alerts[0].body.as_deref().expect("alert body");
    assert!(body.contains("Recipient: jurij@example.com"), "{body}");
    assert!(body.contains("Subject: Your temporary password"), "{body}");
    assert!(
        body.contains(&format!("Attempts: {MAX_ATTEMPTS}")),
        "{body}"
    );
    assert!(
        !body.contains("TOP-SECRET-TEMPORARY-PASSWORD"),
        "the mail body must never be quoted in an alert: {body}"
    );

    // The alert travels the normal route to the admin's notification panel.
    zerf::background::error_notifications::process_pending(&app.state).await;
    let notes = app
        .state
        .db
        .notifications
        .list_for_user(admin_id)
        .await
        .unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.kind == "system_error" && n.title == "Email could not be delivered"),
        "the opted-in admin must receive the failure as a system error notification"
    );

    app.cleanup().await;
}

/// While more attempts are scheduled, a failing mail raises no alert: an outage
/// that passes within the schedule must reach nobody. The alert comes with the
/// final failure, once, and the mail is then given up for good.
#[tokio::test]
async fn a_failing_delivery_is_reported_only_when_it_is_given_up() {
    let app = TestApp::spawn().await;
    opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue(
            "stuck@example.com",
            "Stuck",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    // The first failure looks like a passing outage: nobody hears about it.
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, 1);
    assert!(
        queued_alerts(&app).await.is_empty(),
        "the first failed attempt must not raise an alert"
    );
    assert!(!is_given_up(&app, id).await);

    // A retry that fails changes nothing about that.
    make_retry_due(&app, id).await;
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, 2);
    assert!(queued_alerts(&app).await.is_empty());

    // Even the attempt right before the last one stays quiet.
    fast_forward(&app, id, MAX_ATTEMPTS - 2).await;
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, MAX_ATTEMPTS - 1);
    assert!(queued_alerts(&app).await.is_empty());
    assert!(!is_given_up(&app, id).await);

    // The last attempt is the final failure: one alert, and the mail is given up.
    make_retry_due(&app, id).await;
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, MAX_ATTEMPTS);
    assert!(is_given_up(&app, id).await);
    let alerts = queued_alerts(&app).await;
    assert_eq!(alerts.len(), 1, "the final failure is reported once");
    assert_eq!(
        alerts[0].dedupe_key.as_deref(),
        Some(format!("email_delivery_failed_{id}").as_str())
    );
    assert!(alerts[0]
        .body
        .as_deref()
        .unwrap()
        .contains(&format!("Attempts: {MAX_ATTEMPTS}")));

    // A given-up mail is never tried again, however long ago its last attempt was.
    discard_queued_alerts(&app).await;
    make_retry_due(&app, id).await;
    assert!(due_rows(&app).await.is_empty());
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, MAX_ATTEMPTS);
    assert!(
        queued_alerts(&app).await.is_empty(),
        "a given-up mail must not raise another alert"
    );
    assert_eq!(
        app.state.db.email_queue.count().await.unwrap(),
        1,
        "the given-up row stays for a while instead of being dropped"
    );

    app.cleanup().await;
}

/// A failed mail is offered again only once its wait has passed, and the waits
/// follow the schedule. Otherwise a mail the server keeps refusing would be
/// sent again on every poll.
#[tokio::test]
async fn a_failed_mail_waits_for_its_retry_delay() {
    let app = TestApp::spawn().await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue(
            "someone@example.com",
            "Someone",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    // Right after a failed attempt the mail is not due, and the next poll
    // leaves it alone.
    zerf::background::email_queue::process_pending(&app.state).await;
    assert!(due_rows(&app).await.is_empty());
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(
        attempts_and_error(&app, id).await.0,
        1,
        "a poll inside the waiting time must not attempt the mail again"
    );

    // For every number of failed attempts the mail is due once that wait has
    // passed, and not a minute sooner. A number beyond the schedule (a row
    // that failed more often under older rules) keeps the longest wait.
    let schedule_length = RETRY_DELAYS_SECONDS.len();
    for failed_attempts in 1..=(schedule_length + 3) {
        let wait = RETRY_DELAYS_SECONDS[failed_attempts.min(schedule_length) - 1];
        for (seconds_ago, expect_due) in [(wait - 60, false), (wait + 60, true)] {
            sqlx::query(
                "UPDATE email_queue SET attempts = $2, \
                 last_attempt_at = CURRENT_TIMESTAMP - $3::bigint * INTERVAL '1 second' \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(failed_attempts as i32)
            .bind(seconds_ago)
            .execute(&app.state.pool)
            .await
            .expect("set the time of the last attempt");
            let is_due = due_rows(&app).await.iter().any(|row| row.id == id);
            assert_eq!(
                is_due, expect_due,
                "after {failed_attempts} failed attempts the mail waits {wait}s; \
                 its last attempt was {seconds_ago}s ago"
            );
        }
    }

    app.cleanup().await;
}

/// A row that failed more often than the schedule allows was queued while a
/// worker without a limit was still retrying it. The change must not drop such
/// a mail silently: it gets one more attempt and is reported if that fails.
#[tokio::test]
async fn a_mail_beyond_the_schedule_gets_one_last_attempt_and_is_then_reported() {
    let app = TestApp::spawn().await;
    opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue("old@example.com", "Old", "test", "subject", "body", None)
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;
    fast_forward(&app, id, 300).await;
    assert_eq!(
        due_rows(&app).await.len(),
        1,
        "it is not given up, so it is still offered"
    );

    zerf::background::email_queue::process_pending(&app.state).await;

    assert_eq!(attempts_and_error(&app, id).await.0, 301);
    assert!(is_given_up(&app, id).await);
    assert_eq!(
        queued_alerts(&app).await.len(),
        1,
        "its failure is reported, not swallowed"
    );

    app.cleanup().await;
}

/// A given-up mail is kept for the retention period, so a lost mail can be
/// looked up, and then removed: its text may hold a temporary password or a
/// reset link. A mail still waiting for delivery is never removed.
#[tokio::test]
async fn given_up_mails_are_deleted_after_the_retention_period() {
    let app = TestApp::spawn().await;
    for address in [
        "old@example.com",
        "recent@example.com",
        "waiting@example.com",
    ] {
        app.state
            .db
            .email_queue
            .enqueue(address, "Someone", "test", "subject", "body", None)
            .await
            .expect("enqueue");
    }
    let ids: Vec<i64> = due_rows(&app).await.iter().map(|row| row.id).collect();
    let (old_id, recent_id, waiting_id) = (ids[0], ids[1], ids[2]);
    for (id, days_ago) in [
        (old_id, GIVEN_UP_RETENTION_DAYS + 1),
        (recent_id, GIVEN_UP_RETENTION_DAYS - 1),
    ] {
        sqlx::query(
            "UPDATE email_queue \
             SET given_up_at = CURRENT_TIMESTAMP - make_interval(days => $2) WHERE id = $1",
        )
        .bind(id)
        .bind(days_ago)
        .execute(&app.state.pool)
        .await
        .expect("mark the row as given up");
    }

    let deleted = app
        .state
        .db
        .email_queue
        .delete_given_up_older_than(GIVEN_UP_RETENTION_DAYS)
        .await
        .expect("delete old given-up mails");

    assert_eq!(deleted, 1, "only the mail given up too long ago goes");
    let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM email_queue ORDER BY id")
        .fetch_all(&app.state.pool)
        .await
        .expect("list the remaining rows");
    assert_eq!(left, vec![recent_id, waiting_id]);

    app.cleanup().await;
}

/// The alert about a failed mail is itself a mail. When the mail server is
/// down it is given up too, and reporting *that* would queue yet another alert
/// mail, forever. Its failure is recorded like any other but never reported.
#[tokio::test]
async fn a_failed_alert_mail_is_not_reported_again() {
    let app = TestApp::spawn().await;
    opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue(
            "someone@example.com",
            "Someone",
            "test",
            "subject",
            "body",
            None,
        )
        .await
        .expect("enqueue");
    let id = due_rows(&app).await[0].id;

    // The ordinary mail is given up and reported; the fan-out turns the report
    // into an email for the admin, queued because SMTP counts as configured.
    fast_forward(&app, id, MAX_ATTEMPTS - 1).await;
    zerf::background::email_queue::process_pending(&app.state).await;
    zerf::background::error_notifications::process_pending(&app.state).await;
    let alert_mail_id = due_rows(&app)
        .await
        .iter()
        .find(|mail| mail.kind == "system_error")
        .expect("the alert email must be queued with the system_error kind")
        .id;

    // The alert mail is given up as well. That is logged and recorded, nothing
    // more.
    fast_forward(&app, alert_mail_id, MAX_ATTEMPTS - 1).await;
    zerf::background::email_queue::process_pending(&app.state).await;
    let (attempts, last_error) = attempts_and_error(&app, alert_mail_id).await;
    assert_eq!(attempts, MAX_ATTEMPTS, "the alert mail was attempted");
    assert!(is_given_up(&app, alert_mail_id).await);
    assert!(last_error.is_some(), "its failure is recorded");
    assert!(
        queued_alerts(&app).await.is_empty(),
        "a failed alert mail must not raise another alert"
    );

    app.cleanup().await;
}

// ---------------------------------------------------------------------------
// Reply-To: mail that follows a person's action is answered by that person;
// every other mail ends with the "do not reply" notice
// ---------------------------------------------------------------------------

/// Everything queued for `address` with the given kind.
async fn queued_mail(
    app: &TestApp,
    address: &str,
    kind: &str,
) -> Vec<zerf::repository::EmailQueueEntry> {
    due_rows(app)
        .await
        .into_iter()
        .filter(|mail| mail.to_address == address && mail.kind == kind)
        .collect()
}

/// Both wordings of the closing "do not reply" notice; a mail follows the
/// configured interface language.
const NO_REPLY_NOTICES: [&str; 2] = [
    "Please do not reply! This email was sent automatically by the system.",
    "Bitte nicht antworten! Diese E-Mail wurde automatisch vom System versendet.",
];

fn ends_with_no_reply_notice(mail: &zerf::repository::EmailQueueEntry) -> bool {
    NO_REPLY_NOTICES
        .iter()
        .any(|notice| mail.body_text.ends_with(notice))
}

fn mentions_no_reply_notice(mail: &zerf::repository::EmailQueueEntry) -> bool {
    NO_REPLY_NOTICES
        .iter()
        .any(|notice| mail.body_text.contains(notice))
}

/// The mail is answered by `address`: it names that Reply-To and does not tell
/// the reader not to reply.
fn assert_answered_by(mail: &zerf::repository::EmailQueueEntry, address: &str) {
    assert_eq!(mail.reply_to_address, address, "kind {}", mail.kind);
    assert!(
        !mentions_no_reply_notice(mail),
        "a mail with a Reply-To must not tell the reader not to reply: {}",
        mail.body_text
    );
}

/// The mail has no Reply-To and ends with the do-not-reply notice.
fn assert_do_not_reply(mail: &zerf::repository::EmailQueueEntry) {
    assert_eq!(mail.reply_to_address, "", "kind {}", mail.kind);
    assert!(
        ends_with_no_reply_notice(mail),
        "a mail nobody can answer must end with the do-not-reply notice: {}",
        mail.body_text
    );
}

/// An admin, a lead approving one employee, and the employee, all logged in.
/// Set up *before* SMTP is configured so the onboarding mails of the new
/// accounts stay out of the queue.
async fn team_with_smtp(app: &TestApp) -> (TestClient, TestClient, TestClient, String, i64) {
    let admin = admin_login(app).await;
    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(app, &admin, false).await;
    let lead = login_change_pw(app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(app, "emp-r@example.com", &emp_pw).await;
    configure_unreachable_smtp(app).await;
    (admin, lead, emp, monday_iso, cat_id)
}

/// The employee asks for one vacation day: the first Monday `offset_days` or
/// more after the reference date. Returns the absence id.
async fn request_vacation(emp: &TestClient, offset_days: i64) -> i64 {
    let day = next_monday(offset_days).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind": "vacation", "start_date": day, "end_date": day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "request vacation: {body}");
    id(&body)
}

/// The employee's request reaches the lead with the employee as Reply-To; the
/// lead's decision reaches the employee with the lead as Reply-To.
#[tokio::test]
async fn absence_request_and_rejection_are_answered_by_whoever_acted() {
    let app = TestApp::spawn().await;
    let (_admin, lead, emp, _monday_iso, _cat_id) = team_with_smtp(&app).await;

    let absence_id = request_vacation(&emp, 21).await;
    let requested = queued_mail(&app, "lead-r@example.com", "absence_requested").await;
    assert_eq!(requested.len(), 1, "the lead is told about the request");
    assert_answered_by(&requested[0], "emp-r@example.com");
    assert_eq!(requested[0].reply_to_name, "Emil Emp");

    let (st, body) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/reject"),
            &json!({"reason": "team is short-staffed"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject vacation: {body}");

    let rejected = queued_mail(&app, "emp-r@example.com", "absence_rejected").await;
    assert_eq!(rejected.len(), 1, "the employee is told about the decision");
    assert_answered_by(&rejected[0], "lead-r@example.com");
    assert_eq!(rejected[0].reply_to_name, "Lara Lead");

    app.cleanup().await;
}

/// Approval, both outcomes of a cancellation request, the employee's
/// cancellation request itself, and an admin's revoke: each mail is answered by
/// the person whose action it reports.
#[tokio::test]
async fn absence_approval_cancellation_and_revoke_are_answered_by_whoever_acted() {
    let app = TestApp::spawn().await;
    let (admin, lead, emp, _monday_iso, _cat_id) = team_with_smtp(&app).await;

    let cancelled_id = request_vacation(&emp, 28).await;
    let kept_id = request_vacation(&emp, 35).await;
    let revoked_id = request_vacation(&emp, 42).await;
    for absence_id in [cancelled_id, kept_id, revoked_id] {
        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve vacation");
    }

    // The employee asks to cancel two approved absences; the lead grants one
    // and refuses the other.
    for absence_id in [cancelled_id, kept_id] {
        let (st, _) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
        assert_eq!(st, StatusCode::OK, "request cancellation");
    }
    let (st, _) = lead
        .post(
            &format!("/api/v1/absences/{cancelled_id}/approve-cancellation"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve cancellation");
    let (st, _) = lead
        .post(
            &format!("/api/v1/absences/{kept_id}/reject-cancellation"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject cancellation");

    // An admin withdraws the third, approved absence.
    let (st, _) = admin
        .post(&format!("/api/v1/absences/{revoked_id}/revoke"), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "revoke vacation");

    for (kind, expected_count, answered_by) in [
        ("absence_approved", 3, "lead-r@example.com"),
        ("absence_cancellation_approved", 1, "lead-r@example.com"),
        ("absence_cancellation_rejected", 1, "lead-r@example.com"),
        ("absence_revoked", 1, "admin@example.com"),
    ] {
        let mails = queued_mail(&app, "emp-r@example.com", kind).await;
        assert_eq!(mails.len(), expected_count, "{kind} mails to the employee");
        for mail in &mails {
            assert_answered_by(mail, answered_by);
        }
    }
    let cancellation_requests =
        queued_mail(&app, "lead-r@example.com", "absence_cancellation_requested").await;
    assert_eq!(
        cancellation_requests.len(),
        2,
        "one per cancellation request"
    );
    for mail in &cancellation_requests {
        assert_answered_by(mail, "emp-r@example.com");
    }

    app.cleanup().await;
}

/// Handing in a week reaches the lead with the employee as Reply-To; the
/// lead's decision reaches the employee with the lead as Reply-To.
#[tokio::test]
async fn timesheet_submission_and_decisions_are_answered_by_whoever_acted() {
    let app = TestApp::spawn().await;
    let (_admin, lead, emp, monday_iso, cat_id) = team_with_smtp(&app).await;

    let rejected_entry = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let submitted = queued_mail(&app, "lead-r@example.com", "timesheet_submitted").await;
    assert_eq!(submitted.len(), 1, "the lead is told about the submission");
    assert_answered_by(&submitted[0], "emp-r@example.com");

    let (st, _) = lead
        .post(
            "/api/v1/time-entries/batch-reject",
            &json!({"ids": [rejected_entry], "reason": "wrong category"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject week");
    let rejected = queued_mail(&app, "emp-r@example.com", "timesheet_rejected").await;
    assert_eq!(rejected.len(), 1, "the employee is told about the decision");
    assert_answered_by(&rejected[0], "lead-r@example.com");
    assert_eq!(rejected[0].reply_to_name, "Lara Lead");

    // Hand in another week and approve it.
    let other_monday = next_monday(-21).format("%Y-%m-%d").to_string();
    let approved_entry = create_and_submit_entry(&emp, &other_monday, cat_id).await;
    let (st, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [approved_entry]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve week");
    let approved = queued_mail(&app, "emp-r@example.com", "timesheet_approved").await;
    assert_eq!(approved.len(), 1);
    assert_answered_by(&approved[0], "lead-r@example.com");

    app.cleanup().await;
}

/// An employee's reopen request, the lead rejecting one, and an admin approving
/// another: the admin's decision also reaches the lead, who is assigned to the
/// employee, and replies to that notice go to the admin.
#[tokio::test]
async fn reopen_request_and_decisions_are_answered_by_whoever_acted() {
    let app = TestApp::spawn().await;
    let (admin, lead, emp, monday_iso, cat_id) = team_with_smtp(&app).await;

    // Two approved weeks, one reopen request for each.
    let other_monday = next_monday(-21).format("%Y-%m-%d").to_string();
    let first_entry = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let second_entry = create_and_submit_entry(&emp, &other_monday, cat_id).await;
    let (st, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [first_entry, second_entry]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve both weeks");

    let mut request_ids = Vec::new();
    for week in [&monday_iso, &other_monday] {
        let (st, body) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": week, "reason": "Need to fix an entry"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "request reopen: {body}");
        request_ids.push(id(&body));
    }

    let created = queued_mail(&app, "lead-r@example.com", "reopen_request_created").await;
    assert_eq!(created.len(), 2, "the lead is told about both requests");
    for mail in &created {
        assert_answered_by(mail, "emp-r@example.com");
    }

    // The lead rejects the first request, the admin approves the second.
    let (st, _) = lead
        .post(
            &format!("/api/v1/reopen-requests/{}/reject", request_ids[0]),
            &json!({"reason": "week is closed"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "lead rejects reopen");
    let (st, _) = admin
        .post(
            &format!("/api/v1/reopen-requests/{}/approve", request_ids[1]),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin approves reopen");

    let rejected = queued_mail(&app, "emp-r@example.com", "reopen_rejected").await;
    assert_eq!(rejected.len(), 1);
    assert_answered_by(&rejected[0], "lead-r@example.com");

    let approved = queued_mail(&app, "emp-r@example.com", "reopen_approved").await;
    assert_eq!(approved.len(), 1);
    assert_answered_by(&approved[0], "admin@example.com");

    let told_by_admin = queued_mail(&app, "lead-r@example.com", "reopen_approved_by_admin").await;
    assert_eq!(told_by_admin.len(), 1, "the assigned lead hears about it");
    assert_answered_by(&told_by_admin[0], "admin@example.com");

    app.cleanup().await;
}

/// Setting up an account and resetting a password are an admin's actions: the
/// person who receives the temporary password answers to that admin.
#[tokio::test]
async fn account_mails_are_answered_by_the_admin_who_sent_them() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;

    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"onboard@example.com","first_name":"On","last_name":"Board",
                "role":"employee","weekly_hours":39,"start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create user");
    let new_user_id = id(&body);

    let (status, _) = admin
        .post(
            &format!("/api/v1/users/{new_user_id}/reset-password"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "reset password");

    for kind in ["account_created", "admin_password_reset"] {
        let mails = queued_mail(&app, "onboard@example.com", kind).await;
        assert_eq!(mails.len(), 1, "{kind}");
        assert_answered_by(&mails[0], "admin@example.com");
    }

    app.cleanup().await;
}

/// Mail the system sends on its own — here the alert to an admin about a
/// technical error — has no Reply-To and ends with the notice.
#[tokio::test]
async fn automatic_mail_ends_with_the_do_not_reply_notice() {
    let app = TestApp::spawn().await;
    let admin_id = opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .error_queue
        .enqueue(
            Some("reply_to_test"),
            "Boom",
            Some("something broke"),
            "app",
        )
        .await
        .expect("enqueue error");
    zerf::background::error_notifications::process_pending(&app.state).await;

    let admin_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(admin_id)
        .fetch_one(&app.state.pool)
        .await
        .expect("admin email");
    let alerts = queued_mail(&app, &admin_email, "system_error").await;
    assert_eq!(alerts.len(), 1, "the opted-in admin gets the alert by mail");
    assert_do_not_reply(&alerts[0]);

    app.cleanup().await;
}

/// A deciding person whose address cannot be used in a header must not leave
/// the mail with neither a Reply-To nor the notice.
#[tokio::test]
async fn unusable_reply_to_address_falls_back_to_the_do_not_reply_notice() {
    let app = TestApp::spawn().await;
    let (_admin, lead, emp, _monday_iso, _cat_id) = team_with_smtp(&app).await;
    sqlx::query("UPDATE users SET email = 'not an address' WHERE email = 'lead-r@example.com'")
        .execute(&app.state.pool)
        .await
        .expect("break the lead's address");

    let absence_id = request_vacation(&emp, 21).await;
    let (st, body) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/reject"),
            &json!({"reason": "no"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject vacation: {body}");

    let rejected = queued_mail(&app, "emp-r@example.com", "absence_rejected").await;
    assert_eq!(rejected.len(), 1);
    assert_do_not_reply(&rejected[0]);

    app.cleanup().await;
}
