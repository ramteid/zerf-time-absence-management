use reqwest::StatusCode;

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team_with_suffix, create_and_submit_entry, login_change_pw};

#[tokio::test]
async fn approval_reminders_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "approval-rem").await;
    let lead = login_change_pw(&app, "lead-approval-rem@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-approval-rem@example.com", &emp_pw).await;

    // A submitted week should produce a pending approval target for the approver.
    let _ = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

    // Keep only reminder-generated rows in assertions below.
    let (st, _) = lead.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    zerf::approval_reminders::run_check(&app.state).await;
    zerf::approval_reminders::run_check(&app.state).await;

    let (st, body) = lead.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|item| item["kind"] == "approval_reminder")
        .collect();
    assert_eq!(reminders.len(), 1, "reminders must be idempotent per day");

    // Turning reminders off should suppress newly generated reminder rows.
    sqlx::query(
        "INSERT INTO app_settings(key, value) VALUES ('approval_reminders_enabled', 'false') \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .execute(&app.state.pool)
    .await
    .expect("disable approval reminders setting");

    let (st, _) = lead.delete("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);

    zerf::approval_reminders::run_check(&app.state).await;

    let (st, body) = lead.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK);
    let reminders: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|item| item["kind"] == "approval_reminder")
        .collect();
    assert_eq!(
        reminders.len(),
        0,
        "disabled approval reminders must not create reminder rows"
    );

    app.cleanup().await;
}
