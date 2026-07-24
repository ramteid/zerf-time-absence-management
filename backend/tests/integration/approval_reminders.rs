use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{
    admin_login, bootstrap_team_with_suffix, create_and_submit_entry, id, login_change_pw,
    next_monday,
};

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

    zerf::background::approval_reminders::run_check(&app.state).await;
    zerf::background::approval_reminders::run_check(&app.state).await;

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

    zerf::background::approval_reminders::run_check(&app.state).await;

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

/// A submitted week must count as ONE pending item toward the reminder total,
/// no matter how many daily time-entry rows (or same-day split entries) it
/// contains. Regression test for the "18 pending approvals" miscount, where
/// the reminder counted every submitted time-entry row individually instead
/// of grouping by week.
#[tokio::test]
async fn approval_reminder_counts_weeks_not_time_entry_rows() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "approval-rem-weeks").await;
    let emp = login_change_pw(&app, "emp-approval-rem-weeks@example.com", &emp_pw).await;

    // Week A: two entries on the same day plus one entry on the next day —
    // three time_entries rows, all within a single ISO week.
    let week_a_monday = next_monday(-28);
    let week_a_tuesday = week_a_monday + chrono::Duration::days(1);
    let mut week_a_ids = Vec::new();
    for (day, start, end) in [
        (week_a_monday, "08:00", "12:00"),
        (week_a_monday, "13:00", "17:00"),
        (week_a_tuesday, "08:00", "12:00"),
    ] {
        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day.format("%Y-%m-%d").to_string(),
                    "start_time": start, "end_time": end,
                    "category_id": cat_id, "comment": "work"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create week A entry");
        week_a_ids.push(id(&body));
    }
    let (st, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": week_a_ids}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit week A");

    // Week B: a single entry in a different ISO week.
    let week_b_monday = next_monday(-14).format("%Y-%m-%d").to_string();
    let _ = create_and_submit_entry(&emp, &week_b_monday, cat_id).await;

    // Four time_entries rows total across two distinct weeks — the pending
    // count for the lead must be 2 (one per submitted week), not 4.
    let users = zerf::repository::UserDb::new(app.state.pool.clone());
    let pending = users
        .pending_approvers_for_reminders()
        .await
        .expect("pending approver reminders");
    let lead_row = pending
        .iter()
        .find(|row| row.0 == lead_id)
        .expect("lead has pending approvals");
    assert_eq!(
        lead_row.1, 2,
        "expected one pending item per submitted week, not per time-entry row"
    );

    app.cleanup().await;
}
