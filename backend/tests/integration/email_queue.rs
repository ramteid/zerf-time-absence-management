//! Outbound email queue: enqueue gating on SMTP settings, oldest-first
//! draining, attempts/last_error tracking on failed delivery, and the shared
//! circuit breaker backing off once SMTP is confirmed broken.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::{TestApp, TestClient};
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

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
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

    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
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
        .enqueue("someone@example.com", "Someone", "test", "subject", "body", None)
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
        .enqueue("someone@example.com", "Someone", "test", "subject", "body", None)
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
        .enqueue("stuck@example.com", "Stuck", "test", "subject", "body", None)
        .await
        .expect("enqueue stuck row");
    let stuck_id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

    // One failed attempt sets `last_attempt_at` on the stuck row.
    zerf::background::email_queue::process_pending(&app.state).await;

    // A fresh row queued afterwards has never been attempted.
    app.state
        .db
        .email_queue
        .enqueue("fresh@example.com", "Fresh", "test", "subject", "body", None)
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
        .enqueue("someone@example.com", "Someone", "test", "subject", "body", None)
        .await
        .expect("enqueue");
    let id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

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
        .enqueue("someone@example.com", "Someone", "test", "subject", "body", None)
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

/// A failed delivery must reach the opted-in admins, naming the recipient, the
/// subject, the attempt and the server's reason — and never the mail body,
/// which can hold a temporary password or a reset link.
#[tokio::test]
async fn a_failed_delivery_is_reported_to_the_opted_in_admins() {
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
    let id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

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
    assert!(body.contains("Attempts: 1"), "{body}");
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

/// The same stuck mail is retried on every poll, so it may be reported only at
/// its first failure and once more when it has clearly not recovered — not on
/// every retry.
#[tokio::test]
async fn a_failing_delivery_is_reported_at_the_first_and_the_persistent_attempt_only() {
    let app = TestApp::spawn().await;
    opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue("stuck@example.com", "Stuck", "test", "subject", "body", None)
        .await
        .expect("enqueue");
    let id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

    // Attempt 1 is reported.
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(queued_alerts(&app).await.len(), 1);
    discard_queued_alerts(&app).await;

    // Attempt 2 stays quiet.
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, 2);
    assert!(
        queued_alerts(&app).await.is_empty(),
        "a plain retry must not raise another alert"
    );

    // Jump close to the persistent milestone: attempt 99 stays quiet too...
    sqlx::query("UPDATE email_queue SET attempts = 98 WHERE id = $1")
        .bind(id)
        .execute(&app.state.pool)
        .await
        .expect("fast-forward the attempt counter");
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, 99);
    assert!(queued_alerts(&app).await.is_empty());

    // ...and attempt 100 reports again, with the new attempt count. (Four
    // failures in a row by now: the circuit breaker is still closed.)
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(attempts_and_error(&app, id).await.0, 100);
    let alerts = queued_alerts(&app).await;
    assert_eq!(
        alerts.len(),
        1,
        "the persistent failure is reported once more"
    );
    assert_eq!(
        alerts[0].dedupe_key.as_deref(),
        Some(format!("email_delivery_failed_{id}").as_str()),
        "same key as the first report, so it re-raises that notification"
    );
    assert!(alerts[0].body.as_deref().unwrap().contains("Attempts: 100"));

    app.cleanup().await;
}

/// The first report may describe a passing outage. When the server is back and
/// fails the same mail for a *different* reason, that is new information and
/// must not wait for the persistent milestone.
#[tokio::test]
async fn a_failure_with_a_new_reason_is_reported_again() {
    let app = TestApp::spawn().await;
    opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue("someone@example.com", "Someone", "test", "subject", "body", None)
        .await
        .expect("enqueue");
    let id = app.state.db.email_queue.list_pending(1).await.unwrap()[0].id;

    // Attempt 1 is reported.
    zerf::background::email_queue::process_pending(&app.state).await;
    assert_eq!(queued_alerts(&app).await.len(), 1);
    discard_queued_alerts(&app).await;

    // Pretend that attempt had failed for another reason: the server had
    // answered and refused the recipient. The next failure (connection refused
    // again) then differs from its predecessor...
    sqlx::query("UPDATE email_queue SET last_error = $2 WHERE id = $1")
        .bind(id)
        .bind("permanent error (550): 5.1.1 User unknown")
        .execute(&app.state.pool)
        .await
        .expect("rewrite the previous reason");
    zerf::background::email_queue::process_pending(&app.state).await;

    // ...and is reported although it is only attempt 2.
    assert_eq!(attempts_and_error(&app, id).await.0, 2);
    let alerts = queued_alerts(&app).await;
    assert_eq!(alerts.len(), 1, "a changed reason must be reported again");
    assert!(alerts[0].body.as_deref().unwrap().contains("Attempts: 2"));

    app.cleanup().await;
}

/// The alert about a failed mail is itself a mail. When the mail server is
/// down it fails too, and reporting *that* would queue yet another alert mail,
/// forever. Its failure is recorded like any other but never reported.
#[tokio::test]
async fn a_failed_alert_mail_is_not_reported_again() {
    let app = TestApp::spawn().await;
    opt_in_admin(&app).await;
    configure_unreachable_smtp(&app).await;
    app.state
        .db
        .email_queue
        .enqueue("someone@example.com", "Someone", "test", "subject", "body", None)
        .await
        .expect("enqueue");

    // The ordinary mail fails and is reported; the fan-out turns the report
    // into an email for the admin, queued because SMTP counts as configured.
    zerf::background::email_queue::process_pending(&app.state).await;
    zerf::background::error_notifications::process_pending(&app.state).await;
    let pending = app.state.db.email_queue.list_pending(10).await.unwrap();
    let alert_mail = pending
        .iter()
        .find(|mail| mail.kind == "system_error")
        .expect("the alert email must be queued with the system_error kind");
    let alert_mail_id = alert_mail.id;

    // The alert mail fails as well. That is logged and counted, nothing more.
    zerf::background::email_queue::process_pending(&app.state).await;
    let (attempts, last_error) = attempts_and_error(&app, alert_mail_id).await;
    assert_eq!(attempts, 1, "the alert mail was attempted");
    assert!(last_error.is_some(), "its failure is recorded");
    assert!(
        queued_alerts(&app).await.is_empty(),
        "a failed alert mail must not raise another alert"
    );

    app.cleanup().await;
}

// ---------------------------------------------------------------------------
// Reply-To: mail that follows an approver's decision is answered by them
// ---------------------------------------------------------------------------

/// Everything queued for `address` with the given kind.
async fn queued_mail(
    app: &TestApp,
    address: &str,
    kind: &str,
) -> Vec<zerf::repository::EmailQueueEntry> {
    app.state
        .db
        .email_queue
        .list_pending(100)
        .await
        .expect("list queued mail")
        .into_iter()
        .filter(|mail| mail.to_address == address && mail.kind == kind)
        .collect()
}

/// A team (lead approving one employee) with both logged in, set up *before*
/// SMTP is configured so the onboarding mails of the new accounts stay out of
/// the queue.
async fn team_with_smtp(app: &TestApp) -> (TestClient, TestClient, String, i64) {
    let admin = admin_login(app).await;
    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team(app, &admin, false).await;
    let lead = login_change_pw(app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(app, "emp-r@example.com", &emp_pw).await;
    configure_unreachable_smtp(app).await;
    (lead, emp, monday_iso, cat_id)
}

/// The lead's decision on an absence reaches the employee with the lead as
/// Reply-To, while the employee's own request to the lead carries none.
#[tokio::test]
async fn rejected_absence_email_is_answered_by_the_approver() {
    let app = TestApp::spawn().await;
    let (lead, emp, _monday_iso, _cat_id) = team_with_smtp(&app).await;

    let day = next_monday(21).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind": "vacation", "start_date": day, "end_date": day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "request vacation: {body}");
    let absence_id = id(&body);

    let requested = queued_mail(&app, "lead-r@example.com", "absence_requested").await;
    assert_eq!(requested.len(), 1, "the lead is told about the request");
    assert_eq!(
        requested[0].reply_to_address, "",
        "a request is not an approver's decision: no Reply-To"
    );

    let (st, body) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/reject"),
            &json!({"reason": "team is short-staffed"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject vacation: {body}");

    let rejected = queued_mail(&app, "emp-r@example.com", "absence_rejected").await;
    assert_eq!(rejected.len(), 1, "the employee is told about the decision");
    assert_eq!(rejected[0].reply_to_address, "lead-r@example.com");
    assert_eq!(rejected[0].reply_to_name, "Lara Lead");

    app.cleanup().await;
}

#[tokio::test]
async fn rejected_timesheet_email_is_answered_by_the_approver() {
    let app = TestApp::spawn().await;
    let (lead, emp, monday_iso, cat_id) = team_with_smtp(&app).await;

    let entry_id = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let submitted = queued_mail(&app, "lead-r@example.com", "timesheet_submitted").await;
    assert_eq!(submitted.len(), 1, "the lead is told about the submission");
    assert_eq!(submitted[0].reply_to_address, "");

    let (st, _) = lead
        .post(
            "/api/v1/time-entries/batch-reject",
            &json!({"ids": [entry_id], "reason": "wrong category"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reject week");

    let rejected = queued_mail(&app, "emp-r@example.com", "timesheet_rejected").await;
    assert_eq!(rejected.len(), 1, "the employee is told about the decision");
    assert_eq!(rejected[0].reply_to_address, "lead-r@example.com");
    assert_eq!(rejected[0].reply_to_name, "Lara Lead");

    app.cleanup().await;
}

#[tokio::test]
async fn approved_timesheet_email_is_answered_by_the_approver() {
    let app = TestApp::spawn().await;
    let (lead, emp, monday_iso, cat_id) = team_with_smtp(&app).await;

    let entry_id = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let (st, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve week");

    let approved = queued_mail(&app, "emp-r@example.com", "timesheet_approved").await;
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].reply_to_address, "lead-r@example.com");

    app.cleanup().await;
}

#[tokio::test]
async fn reopen_decision_email_is_answered_by_the_approver() {
    let app = TestApp::spawn().await;
    let (lead, emp, monday_iso, cat_id) = team_with_smtp(&app).await;

    let entry_id = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
    let (st, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve week");

    let (st, body) = emp
        .post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso, "reason": "Need to fix an entry"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "request reopen: {body}");
    let request_id = id(&body);

    let created = queued_mail(&app, "lead-r@example.com", "reopen_request_created").await;
    assert_eq!(created.len(), 1, "the lead is told about the request");
    assert_eq!(created[0].reply_to_address, "");

    let (st, _) = lead
        .post(
            &format!("/api/v1/reopen-requests/{request_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve reopen");

    let approved = queued_mail(&app, "emp-r@example.com", "reopen_approved").await;
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].reply_to_address, "lead-r@example.com");
    assert_eq!(approved[0].reply_to_name, "Lara Lead");

    app.cleanup().await;
}

/// System mail that is nobody's decision (here: the account-created mail an
/// admin's action triggers) keeps coming from the system sender only.
#[tokio::test]
async fn system_mail_has_no_reply_to() {
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

    let onboarding = queued_mail(&app, "onboard@example.com", "account_created").await;
    assert_eq!(onboarding.len(), 1);
    assert_eq!(onboarding[0].reply_to_address, "");

    app.cleanup().await;
}
