//! End-to-end reports workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use chrono::{Datelike, NaiveDate};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{TestApp, TestClient};
use crate::helpers::*;

async fn assert_get_forbidden(client: &TestClient, path: &str, label: &str) {
    let (status, _) = client.get(path).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{label}");
}

#[tokio::test]
async fn report_export_queue_requeues_past_month_mutations() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::REPORT_UPLOAD_ENABLED_KEY, "true")
        .await
        .expect("enable report upload");

    let (_lead_id, lead_pw, emp_id, emp_pw, _default_monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "export-requeue").await;
    let lead = login_change_pw(&app, "lead-export-requeue@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-export-requeue@example.com", &emp_pw).await;

    let original_day = next_monday(-75);
    let moved_day = next_monday(-40);
    let absence_day = original_day + chrono::Duration::days(1);
    let revoked_absence_day = moved_day + chrono::Duration::days(1);
    let original_iso = original_day.format("%Y-%m-%d").to_string();
    let moved_iso = moved_day.format("%Y-%m-%d").to_string();
    let absence_iso = absence_day.format("%Y-%m-%d").to_string();
    let revoked_absence_iso = revoked_absence_day.format("%Y-%m-%d").to_string();
    let original_period = original_day.format("%Y-%m").to_string();
    let moved_period = moved_day.format("%Y-%m").to_string();
    assert_ne!(
        original_period, moved_period,
        "test setup must span two past months"
    );

    let (status, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": original_iso,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "approval export queue"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create export queue entry");
    let entry_id = id(&body);
    let (status, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
        .await;
    assert_eq!(status, StatusCode::OK, "submit export queue entry");
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve queues original month");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(emp_id, original_period.clone())],
        "batch approval must requeue the approved entry month"
    );
    app.state
        .db
        .export_queue
        .delete_entry(emp_id, &original_period)
        .await
        .unwrap();

    let (status, _) = admin
        .put(
            &format!("/api/v1/team-settings/{emp_id}"),
            &json!({"allow_reopen_without_approval": false, "allow_submission_without_approval": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable submission auto-approval");
    let (status, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": original_iso,
                "start_time": "12:15",
                "end_time": "12:45",
                "category_id": cat_id,
                "comment": "auto-approved export queue"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create auto-approved queue entry");
    let auto_approved_entry_id = id(&body);
    let (status, body) = emp
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [auto_approved_entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "auto-approved submit queues month");
    assert_eq!(body["auto_approved"], true);
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(emp_id, original_period.clone())],
        "auto-approved submission must requeue the approved entry month"
    );
    app.state
        .db
        .export_queue
        .delete_entry(emp_id, &original_period)
        .await
        .unwrap();
    let (status, _) = admin
        .put(
            &format!("/api/v1/team-settings/{emp_id}"),
            &json!({"allow_reopen_without_approval": false, "allow_submission_without_approval": false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "disable submission auto-approval");

    let (status, _) = admin
        .put(
            &format!("/api/v1/time-entries/{entry_id}"),
            &json!({
                "entry_date": moved_iso,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "moved approved entry"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "admin moves approved entry");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![
            (emp_id, original_period.clone()),
            (emp_id, moved_period.clone())
        ],
        "admin correction must requeue both the source and destination months"
    );
    for period in [&original_period, &moved_period] {
        app.state
            .db
            .export_queue
            .delete_entry(emp_id, period)
            .await
            .unwrap();
    }

    let (status, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": original_iso,
                "start_time": "13:00",
                "end_time": "14:00",
                "category_id": cat_id,
                "comment": "rejection export queue"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create rejection candidate");
    let rejected_entry_id = id(&body);
    let (status, _) = emp
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [rejected_entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "submit rejection candidate");
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-reject",
            &json!({"ids": [rejected_entry_id], "reason": "needs correction"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "reject queues original month");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(emp_id, original_period.clone())],
        "batch rejection must requeue the rejected entry month"
    );
    app.state
        .db
        .export_queue
        .delete_entry(emp_id, &original_period)
        .await
        .unwrap();

    let (status, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "special_leave",
                "start_date": absence_iso,
                "end_date": absence_iso
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create report-affecting absence");
    let absence_id = id(&body);
    let (status, _) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "absence approval queues month");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(emp_id, original_period.clone())],
        "absence approval must requeue the affected month"
    );
    app.state
        .db
        .export_queue
        .delete_entry(emp_id, &original_period)
        .await
        .unwrap();
    let (status, _) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "requesting absence cancellation succeeds"
    );
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    assert!(
        pending.is_empty(),
        "cancellation request keeps the absence report-effective and should not requeue yet"
    );
    let (status, _) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/approve-cancellation"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "cancellation approval queues month");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(emp_id, original_period.clone())],
        "approved cancellation must requeue the affected month"
    );
    app.state
        .db
        .export_queue
        .delete_entry(emp_id, &original_period)
        .await
        .unwrap();

    let (status, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "special_leave",
                "start_date": revoked_absence_iso,
                "end_date": revoked_absence_iso
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create revoke candidate absence");
    let revoke_absence_id = id(&body);
    let (status, _) = lead
        .post(
            &format!("/api/v1/absences/{revoke_absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve revoke candidate absence");
    app.state
        .db
        .export_queue
        .delete_entry(emp_id, &moved_period)
        .await
        .unwrap();
    let (status, _) = admin
        .post(
            &format!("/api/v1/absences/{revoke_absence_id}/revoke"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "admin revoke queues month");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(emp_id, moved_period.clone())],
        "admin revocation must requeue the affected month"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn report_export_requeue_preserves_period_after_start_date_change() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::REPORT_UPLOAD_ENABLED_KEY, "true")
        .await
        .expect("enable report upload");

    let (_lead_id, _lead_pw, emp_id, _emp_pw, _default_monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "export-pre-start-requeue").await;
    let old_day = next_monday(-75);
    let old_period = old_day.format("%Y-%m").to_string();
    let period_start = NaiveDate::from_ymd_opt(old_day.year(), old_day.month(), 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(
        old_day.year(),
        old_day.month(),
        zerf::time_calc::last_day_of_month(old_day.year(), old_day.month()),
    )
    .unwrap();
    sqlx::query("UPDATE users SET start_date=$2 WHERE id=$1")
        .bind(emp_id)
        .bind(period_start)
        .execute(&app.state.pool)
        .await
        .expect("set original start date to the start of the exported period");
    let new_start_date = period_start + chrono::Duration::days(14);
    let (status, _) = admin
        .put(
            &format!("/api/v1/users/{emp_id}"),
            &json!({ "start_date": new_start_date }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "move start date into old period");

    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "period {old_period} must stay queued after start_date moved to {new_start_date}"
    );
    assert_eq!(pending[0].user_id, emp_id);
    assert_eq!(pending[0].period, old_period);
    assert!(
        pending[0].requires_start_date_review,
        "mid-month start-date changes require explicit review before upload"
    );

    zerf::services::reports::requeue_export_for_dates(&app.state.pool, &[(emp_id, old_day)]).await;

    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "normal requeues must not duplicate the pending start-date review row"
    );
    assert_eq!(pending[0].user_id, emp_id);
    assert_eq!(pending[0].period, old_period);
    assert!(
        pending[0].requires_start_date_review,
        "normal requeues must not clear the review flag"
    );

    let has_hidden_content = app
        .state
        .db
        .reports
        .has_report_content_before_start_date(emp_id, period_start, period_end, new_start_date)
        .await
        .expect("check pre-start content");
    assert!(
        !has_hidden_content,
        "the start-date review flag must cover target-only partial-month drift"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn report_export_queue_includes_tracking_disabled_users_with_history() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::REPORT_UPLOAD_ENABLED_KEY, "true")
        .await
        .expect("enable report upload");
    let (_, categories) = admin.get("/api/v1/categories").await;
    let cat_id = categories.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let historical_day = next_monday(-75);
    let period = historical_day.format("%Y-%m").to_string();
    let month_start = NaiveDate::from_ymd_opt(historical_day.year(), historical_day.month(), 1)
        .expect("valid month start");
    let month_end = NaiveDate::from_ymd_opt(
        historical_day.year(),
        historical_day.month(),
        zerf::time_calc::last_day_of_month(historical_day.year(), historical_day.month()),
    )
    .expect("valid month end");

    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "historical-export-admin@example.com",
                "first_name": "Historical",
                "last_name": "Admin",
                "role": "admin",
                "tracks_time": true,
                "weekly_hours": 39,
                "start_date": "2024-01-01"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create tracking admin");
    let admin_id = id(&body);

    sqlx::query(
        "INSERT INTO time_entries \
         (user_id, entry_date, start_time, end_time, category_id, status, reviewed_by, reviewed_at) \
         VALUES ($1, $2, '08:00', '12:00', $3, 'approved', 1, NOW())",
    )
    .bind(admin_id)
    .bind(historical_day)
    .bind(cat_id)
    .execute(&app.state.pool)
    .await
    .expect("insert approved historical entry");

    let (status, _) = admin
        .put(
            &format!("/api/v1/users/{admin_id}"),
            &json!({ "tracks_time": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "disable tracking");

    let members = app
        .state
        .db
        .reports
        .timesheet_members_for_period(month_start, month_end)
        .await
        .expect("list export members");
    assert!(
        members.iter().any(|member| member.id == admin_id),
        "tracking-disabled users with approved history must still be selected for export"
    );

    zerf::services::reports::requeue_export_for_dates(
        &app.state.pool,
        &[(admin_id, historical_day)],
    )
    .await;
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "disabled user's historical month is requeued"
    );
    assert_eq!(pending[0].user_id, admin_id);
    assert_eq!(pending[0].period, period);

    app.cleanup().await;
}

#[tokio::test]
async fn report_export_holds_tracking_disabled_users_with_unresolved_time_rows() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::REPORT_UPLOAD_ENABLED_KEY, "true")
        .await
        .expect("enable report upload");
    let (_, categories) = admin.get("/api/v1/categories").await;
    let cat_id = categories.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let historical_day = next_monday(-75);
    let period = historical_day.format("%Y-%m").to_string();
    let month_start = NaiveDate::from_ymd_opt(historical_day.year(), historical_day.month(), 1)
        .expect("valid month start");
    let month_end = NaiveDate::from_ymd_opt(
        historical_day.year(),
        historical_day.month(),
        zerf::time_calc::last_day_of_month(historical_day.year(), historical_day.month()),
    )
    .expect("valid month end");

    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "historical-unresolved-admin@example.com",
                "first_name": "Historical",
                "last_name": "Unresolved",
                "role": "admin",
                "tracks_time": true,
                "weekly_hours": 39,
                "start_date": "2024-01-01"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create tracking admin");
    let admin_id = id(&body);

    sqlx::query(
        "INSERT INTO time_entries \
         (user_id, entry_date, start_time, end_time, category_id, status, submitted_at) \
         VALUES ($1, $2, '08:00', '12:00', $3, 'submitted', NOW())",
    )
    .bind(admin_id)
    .bind(historical_day)
    .bind(cat_id)
    .execute(&app.state.pool)
    .await
    .expect("insert submitted historical entry");

    let (status, _) = admin
        .put(
            &format!("/api/v1/users/{admin_id}"),
            &json!({ "tracks_time": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "disable tracking");

    let status_after_disable: String =
        sqlx::query_scalar("SELECT status FROM time_entries WHERE user_id=$1 AND entry_date=$2")
            .bind(admin_id)
            .bind(historical_day)
            .fetch_one(&app.state.pool)
            .await
            .expect("load reverted entry status");
    assert_eq!(
        status_after_disable, "draft",
        "disabling tracking reverts submitted rows to draft"
    );

    let has_unresolved_rows = app
        .state
        .db
        .reports
        .has_unresolved_time_entries_in_range(admin_id, month_start, month_end)
        .await
        .expect("check unresolved rows");
    assert!(
        has_unresolved_rows,
        "historical-only exports must hold while draft rows remain in the month"
    );

    zerf::services::reports::requeue_export_for_dates(
        &app.state.pool,
        &[(admin_id, historical_day)],
    )
    .await;
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "unresolved historical month stays queued for visible follow-up"
    );
    assert_eq!(pending[0].user_id, admin_id);
    assert_eq!(pending[0].period, period);

    app.cleanup().await;
}

#[tokio::test]
async fn report_export_gate_waits_for_pending_absence_decision() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, emp_pw, _default_monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "export-pending-absence").await;
    let lead = login_change_pw(&app, "lead-export-pending-absence@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-export-pending-absence@example.com", &emp_pw).await;

    let day = next_monday(-75);
    sqlx::query("UPDATE users SET weekly_hours=8, workdays_per_week=1, start_date=$2 WHERE id=$1")
        .bind(emp_id)
        .bind(day)
        .execute(&app.state.pool)
        .await
        .expect("set one-day work week from test day");

    let day_iso = day.format("%Y-%m-%d").to_string();
    let (status, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": day_iso,
                "start_time": "08:00",
                "end_time": "16:00",
                "category_id": cat_id,
                "comment": "submitted despite pending absence"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create submitted entry candidate");
    let entry_id = id(&body);
    let (status, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
        .await;
    assert_eq!(status, StatusCode::OK, "submit entry");

    let mut later_day = day + chrono::Duration::days(7);
    while later_day.month() == day.month() {
        let later_iso = later_day.format("%Y-%m-%d").to_string();
        let (status, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": later_iso,
                    "start_time": "08:00",
                    "end_time": "16:00",
                    "category_id": cat_id,
                    "comment": "submitted later one-day week"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "create later submitted entry");
        let later_entry_id = id(&body);
        let (status, _) = emp
            .post(
                "/api/v1/time-entries/submit",
                &json!({"ids": [later_entry_id]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "submit later entry");
        later_day += chrono::Duration::days(7);
    }

    let special_leave = absence_cat(&app.state.pool, "special_leave").await;
    let (status, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "category_id": special_leave.id,
                "start_date": day_iso,
                "end_date": day_iso,
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create pending absence over submitted day"
    );
    let absence_id = id(&body);

    let user_start_date = day;
    let user_facing_ready_while_pending = zerf::services::reports::all_weeks_submitted_for_month(
        &app.state.pool,
        emp_id,
        day,
        day,
        user_start_date,
        false,
        1,
    )
    .await
    .expect("check user-facing completeness while absence pending");
    assert!(
        user_facing_ready_while_pending,
        "pending absence must excuse user-facing completeness when entries are blocked"
    );
    let month = day.format("%Y-%m").to_string();
    let (status, team_rows) = admin
        .get(&format!("/api/v1/reports/team?month={month}"))
        .await;
    assert_eq!(status, StatusCode::OK, "team report while absence pending");
    let employee_row = team_rows["rows"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["user_id"].as_i64() == Some(emp_id))
        })
        .expect("employee row in team report");
    assert_eq!(
        employee_row["weeks_all_submitted"], true,
        "team report must use the same user-facing pending-absence completeness"
    );

    let export_ready_while_pending = zerf::services::reports::all_weeks_ready_for_timesheet_export(
        &app.state.pool,
        emp_id,
        day,
        day,
        user_start_date,
        false,
        1,
    )
    .await
    .expect("check export gate while absence pending");
    assert!(
        !export_ready_while_pending,
        "pending absence must hold the export gate even when time is submitted"
    );

    let (status, _) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/reject"),
            &json!({"reason": "not an absence"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "reject pending absence");
    let ready_after_rejection = zerf::services::reports::all_weeks_ready_for_timesheet_export(
        &app.state.pool,
        emp_id,
        day,
        day,
        user_start_date,
        false,
        1,
    )
    .await
    .expect("check export gate after absence rejection");
    assert!(
        ready_after_rejection,
        "decided absence should release the export gate once entries are submitted"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn report_export_gate_ignores_pending_absence_outside_export_month() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _default_monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "export-adjacent-absence").await;

    let month_start = NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
    let month_end = NaiveDate::from_ymd_opt(2025, 7, 31).unwrap();
    let user_start_date = NaiveDate::from_ymd_opt(2025, 7, 28).unwrap();
    sqlx::query("UPDATE users SET weekly_hours=40, workdays_per_week=5, start_date=$2 WHERE id=$1")
        .bind(emp_id)
        .bind(user_start_date)
        .execute(&app.state.pool)
        .await
        .expect("set user start date to last week of July");

    for day in 28..=31 {
        let entry_date = NaiveDate::from_ymd_opt(2025, 7, day).unwrap();
        sqlx::query(
            "INSERT INTO time_entries \
             (user_id, entry_date, start_time, end_time, category_id, status, reviewed_by, reviewed_at) \
             VALUES ($1, $2, '08:00', '16:00', $3, 'approved', 1, NOW())",
        )
        .bind(emp_id)
        .bind(entry_date)
        .bind(cat_id)
        .execute(&app.state.pool)
        .await
        .expect("insert approved July entry");
    }

    let special_leave = absence_cat(&app.state.pool, "special_leave").await;
    let adjacent_pending_day = NaiveDate::from_ymd_opt(2025, 8, 1).unwrap();
    sqlx::query(
        "INSERT INTO absences (user_id, category_id, start_date, end_date, status) \
         VALUES ($1, $2, $3, $3, 'requested')",
    )
    .bind(emp_id)
    .bind(special_leave.id)
    .bind(adjacent_pending_day)
    .execute(&app.state.pool)
    .await
    .expect("insert adjacent pending absence");

    let export_ready = zerf::services::reports::all_weeks_ready_for_timesheet_export(
        &app.state.pool,
        emp_id,
        month_start,
        month_end,
        user_start_date,
        false,
        5,
    )
    .await
    .expect("check export gate with adjacent pending absence");

    assert!(
        export_ready,
        "a pending absence outside the exported month must not hold that month's export"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn reports_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Range CSV and category totals for booked entries --
    {
        let (lead_id, lead_pw, emp_id, emp_pw, monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "1").await;
        let lead = login_change_pw(&app, "lead-1@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-1@example.com", &emp_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "=draft formula"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create draft report entry");
        let entry_id = id(&body);

        let (st, _) = lead
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "13:00",
                    "end_time": "17:00",
                    "category_id": cat_id,
                    "comment": "lead own time"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create lead draft entry");

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "pure-admin-category-scope@example.com",
                    "first_name": "Pure",
                    "last_name": "CategoryScope",
                    "role": "admin",
                    "weekly_hours": 0,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                    "tracks_time": false
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create pure-admin scope fixture");
        let pure_admin_id = id(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "inactive-category-scope@example.com",
                    "first_name": "Inactive",
                    "last_name": "CategoryScope",
                    "role": "employee",
                    "weekly_hours": 39,
                    "start_date": "2024-01-01",
                    "approver_ids": [lead_id]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create inactive scope fixture");
        let inactive_id = id(&body);
        // Set user inactive via direct DB mutation (deactivation feature removed).
        sqlx::query("UPDATE users SET active=FALSE WHERE id=$1")
            .bind(inactive_id)
            .execute(&app.state.pool)
            .await
            .expect("set scope fixture inactive");

        for excluded_user_id in [pure_admin_id, inactive_id] {
            sqlx::query(
                "INSERT INTO time_entries(user_id, entry_date, start_time, end_time, category_id, status, reviewed_by, reviewed_at) \
                 VALUES ($1,$2,'08:00','12:00',$3,'approved',$4,CURRENT_TIMESTAMP)",
            )
            .bind(excluded_user_id)
            .bind(chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap())
            .bind(cat_id)
            .bind(lead_id)
            .execute(&app.state.pool)
            .await
            .unwrap();
        }

        // Draft entries are booked time and should appear in category totals.
        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report with only draft");
        assert_eq!(body.as_array().unwrap()[0]["minutes"], 240);

        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead aggregate category report");
        assert_eq!(
            body.as_array().unwrap()[0]["minutes"],
            480,
            "aggregate must include lead + direct report booked time"
        );

        let (st, body) = admin
            .get(&format!(
                "/api/v1/reports/categories?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "admin aggregate category scope");
        assert_eq!(
            body.as_array().unwrap()[0]["minutes"],
            480,
            "admin aggregate must exclude pure-admin and inactive legacy entries"
        );

        // Submit and approve the entry
        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit entry");
        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve entry");

        // Approved entries remain visible in category totals.
        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report with approved");
        assert_eq!(body.as_array().unwrap()[0]["minutes"], 240);

        let (st, csv_body) = lead
            .get_raw(&format!(
                "/api/v1/reports/csv?user_id={}&from={}&to={}",
                emp_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "range CSV export");
        assert!(csv_body.contains("08:00"));
        assert!(csv_body.contains("'=draft formula"));

        let (st, _) = lead
            .get(&format!(
                "/api/v1/reports/csv?user_id={}&from=2026-05-02&to=2026-05-01",
                emp_id
            ))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "CSV inverted range rejected");

        let too_far = (chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap()
            + chrono::Duration::days(367))
        .format("%Y-%m-%d")
        .to_string();
        let (st, _) = lead
            .get(&format!(
                "/api/v1/reports/csv?user_id={}&from={}&to={}",
                emp_id, monday, too_far
            ))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "CSV max range rejected");

        let (st, _) = emp
            .get(&format!(
                "/api/v1/reports/csv?user_id={}&from={}&to={}",
                lead_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "employee cannot export lead CSV");

        let month = &monday[..7];
        let (st, _) = lead
            .get_raw(&format!(
                "/api/v1/reports/month/csv?user_id={}&month={}",
                emp_id, month
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "legacy month CSV remains available");
    }

    // -- Flextime reduction blocks the day but does not credit hours or submission coverage --
    {
        let (_lead_id, lead_pw, emp_id, emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "5").await;
        let lead = login_change_pw(&app, "lead-5@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-5@example.com", &emp_pw).await;
        let tuesday = (chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap()
            + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

        let (_, categories_body) = admin.get("/api/v1/categories").await;
        let flextime_reduction_category_id =
            category_id_by_name(&categories_body, "Flextime Reduction")
                .expect("flextime reduction category exists");

        // Give the employee a large opening flextime balance so B8
        // (validate_flextime_balance) passes. The integration test user is
        // created with start_date=2024-01-01 but has no approved hours, so
        // without a positive seed the balance would be deeply negative.
        sqlx::query("UPDATE users SET overtime_start_balance_min = 9999999 WHERE id = $1")
            .bind(emp_id)
            .execute(&app.state.pool)
            .await
            .expect("seed flextime balance");

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "flextime_reduction",
                    "start_date": monday,
                    "end_date": monday,
                    "comment": "use balance"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create flextime reduction absence");
        let absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve flextime reduction absence");

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": flextime_reduction_category_id,
                    "comment": "should still be blocked"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "approved flextime reduction absence blocks the day"
        );

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": tuesday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": flextime_reduction_category_id,
                    "comment": "flex reduction entry"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create flextime reduction entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit flextime reduction entry");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve flextime reduction entry");

        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report with flextime reduction");

        let monday_row = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert_eq!(monday_row["absence"], "flextime_reduction");
        assert_eq!(monday_row["target_min"], per_day_target_minutes(39));
        assert_eq!(monday_row["actual_min"], 0);

        let tuesday_row = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == tuesday)
            .unwrap();
        assert_eq!(tuesday_row["actual_min"], 0);
        assert_eq!(tuesday_row["entries"].as_array().unwrap().len(), 1);
        assert_eq!(body["submitted_min"], 0);
        // Category totals include all non-rejected entries regardless of
        // crediting status (user-guide: "not only crediting categories").
        // The approved flextime-reduction entry (4h = 240 min) appears here.
        let cat_totals = body["category_totals"].as_object().unwrap();
        assert_eq!(cat_totals.len(), 1, "one category in totals");
        assert_eq!(
            cat_totals
                .get("Flextime Reduction")
                .and_then(|v| v.as_i64()),
            Some(240)
        );
        assert_eq!(body["weeks_all_submitted"], false);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "flextime report with flextime reduction"
        );
        let rows = body.as_array().unwrap();
        assert_eq!(rows[0]["target_min"], per_day_target_minutes(39));
        assert_eq!(rows[0]["actual_min"], 0);
        assert_eq!(rows[1]["target_min"], per_day_target_minutes(39));
        assert_eq!(rows[1]["actual_min"], 0);

        let (st, _body) = emp
            .get(&format!(
                "/api/v1/reports/categories?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "employee still needs user_id for category report"
        );

        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "category report includes non-crediting entries"
        );
        // Category breakdowns include all non-rejected entries regardless of
        // crediting status (user-guide: "not only crediting categories").
        let cat_arr = body.as_array().unwrap();
        assert_eq!(cat_arr.len(), 1, "one category in report");
        assert_eq!(cat_arr[0]["category"], "Flextime Reduction");
        assert_eq!(cat_arr[0]["minutes"], 240);

        let (st, csv_body) = lead
            .get_raw(&format!(
                "/api/v1/reports/month/csv?user_id={}&month={}",
                emp_id, month
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "month CSV with flextime reduction");
        assert!(
            csv_body.contains(",Total,,,,0,"),
            "CSV total must ignore non-crediting flextime reduction entries: {csv_body}"
        );
    }

    // -- Partial sick day counts worked time and removes target --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;
        let lead = login_change_pw(&app, "lead-2@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-2@example.com", &emp_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "sick",
                    "start_date": monday,
                    "end_date": monday,
                    "comment": "cold"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create sick leave");
        assert_eq!(body["status"], "approved");

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "worked half day"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create partial sick-day entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit partial sick-day entry");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve partial sick-day entry");

        // Sick leave removes the target for that day. Actual remains the approved
        // worked time only; absence credit is shown separately in absence reporting.
        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={}", month))
            .await;
        assert_eq!(st, StatusCode::OK, "month report");
        let day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert_eq!(day["absence"], "sick");
        assert_eq!(day["target_min"], 0);
        assert_eq!(day["actual_min"], 240);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime report");
        assert_eq!(body.as_array().unwrap()[0]["target_min"], 0);
        assert_eq!(body.as_array().unwrap()[0]["actual_min"], 240);
    }

    // -- Reports include current day in hours and categories --
    {
        let (_lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3").await;
        let lead = login_change_pw(&app, "lead-3@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-3@example.com", &emp_pw).await;
        let today = today();

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": today,
                    "start_time": "00:00",
                    "end_time": "00:01",
                    "category_id": cat_id,
                    "comment": "today should report"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create today entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit today entry");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve today entry");

        let month = &today[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report");
        // Month report is now month-to-date and therefore includes today's approved entries.
        assert_eq!(body["actual_min"], 1);
        assert!(!body["category_totals"].as_object().unwrap().is_empty());
        let today_row = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == today)
            .unwrap();
        assert_eq!(today_row["actual_min"], 1);
        assert_eq!(today_row["entries"].as_array().unwrap().len(), 1);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, today, today
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report for today");
        let rows = body.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["minutes"], 1);
    }

    // -- cancellation_pending absences remove day target like approved absences --
    {
        let (_lead_id, _lead_pw, emp_id, emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3b").await;
        let emp = login_change_pw(&app, "emp-3b@example.com", &emp_pw).await;

        // Insert a cancellation_pending vacation absence directly to pin report semantics.
        // Time-entry validation treats this status as blocking, so reports/flextime must
        // also remove target minutes for the covered day.
        sqlx::query(
            "INSERT INTO absences(user_id, category_id, start_date, end_date, status, created_at) \
             SELECT $1, id, $2, $2, 'cancellation_pending', CURRENT_TIMESTAMP \
             FROM absence_categories WHERE slug='vacation'",
        )
        .bind(emp_id)
        .bind(chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap())
        .execute(&app.state.pool)
        .await
        .unwrap();

        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report for cancellation_pending");
        let day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert_eq!(day["absence"], "vacation");
        assert_eq!(day["target_min"], 0);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime for cancellation_pending");
        assert_eq!(body.as_array().unwrap()[0]["absence"], "vacation");
        assert_eq!(body.as_array().unwrap()[0]["target_min"], 0);
    }

    // -- requested absences do not remove day target before approval --
    {
        let expected_day_target = per_day_target_minutes(39);
        let (_lead_id, _lead_pw, emp_id, emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3c").await;
        let emp = login_change_pw(&app, "emp-3c@example.com", &emp_pw).await;

        // Insert a requested vacation absence directly to pin report semantics.
        // Requested absences are not yet approved and therefore must NOT remove
        // target minutes in month/flextime views.
        sqlx::query(
            "INSERT INTO absences(user_id, category_id, start_date, end_date, status, created_at) \
             SELECT $1, id, $2, $2, 'requested', CURRENT_TIMESTAMP \
             FROM absence_categories WHERE slug='vacation'",
        )
        .bind(emp_id)
        .bind(chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap())
        .execute(&app.state.pool)
        .await
        .unwrap();

        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report for requested absence");
        let day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert!(day["absence"].is_null());
        assert_eq!(day["target_min"], expected_day_target);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime for requested absence");
        assert!(body.as_array().unwrap()[0]["absence"].is_null());
        assert_eq!(
            body.as_array().unwrap()[0]["target_min"],
            expected_day_target
        );
    }

    // -- Reports ignore legacy time before user start date --
    {
        let (lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "4").await;
        let lead = login_change_pw(&app, "lead-4@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-4@example.com", &emp_pw).await;
        let legacy_date = chrono::NaiveDate::from_ymd_opt(2023, 12, 29).unwrap();
        let legacy_date_iso = legacy_date.format("%Y-%m-%d").to_string();

        sqlx::query(
            "INSERT INTO time_entries(user_id, entry_date, start_time, end_time, category_id, status, reviewed_by, reviewed_at) \
             VALUES ($1,$2,$3,$4,$5,'approved',$6,CURRENT_TIMESTAMP)",
        )
        .bind(emp_id)
        .bind(legacy_date)
        .bind("08:00")
        .bind("12:00")
        .bind(cat_id)
        .bind(lead_id)
        .execute(&app.state.pool)
        .await
        .unwrap();

        let (st, body) = emp.get("/api/v1/reports/month?month=2023-12").await;
        assert_eq!(st, StatusCode::OK, "month report before start date");
        assert_eq!(body["actual_min"], 0);
        assert!(body["category_totals"].as_object().unwrap().is_empty());
        let legacy_day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == legacy_date_iso)
            .unwrap();
        assert_eq!(legacy_day["target_min"], 0);
        assert_eq!(legacy_day["actual_min"], 0);
        assert!(legacy_day["entries"].as_array().unwrap().is_empty());

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                legacy_date_iso, legacy_date_iso
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime before start date");
        assert_eq!(body.as_array().unwrap()[0]["actual_min"], 0);
        assert_eq!(body.as_array().unwrap()[0]["target_min"], 0);

        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, legacy_date_iso, legacy_date_iso
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report before start date");
        assert!(body.as_array().unwrap().is_empty());
    }

    // -- Assistant behavior is role-based, not weekly_hours-based --
    {
        let (lead_id, _lead_pw, _emp_id, _emp_pw, _monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "assistant-role").await;
        let month = today()[..7].to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"assistant-reports@example.com",
                    "first_name":"Role",
                    "last_name":"Assistant",
                    "role":"assistant",
                    "weekly_hours":0,
                    "start_date":"2024-01-01",
                    "approver_ids":[lead_id]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create assistant for reports");
        let assistant_id = id(&body);

        // Simulate legacy/imported inconsistency that bypasses API validation.
        sqlx::query(
            "UPDATE users SET weekly_hours = 39.0, overtime_start_balance_min = 120 WHERE id = $1",
        )
        .bind(assistant_id)
        .execute(&app.state.pool)
        .await
        .unwrap();

        let (st, body) = admin
            .get(&format!(
                "/api/v1/reports/month?user_id={assistant_id}&month={month}"
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "assistant month report");
        assert_eq!(
            body["target_min"], 0,
            "assistant month target must remain 0"
        );
        assert_eq!(
            body["full_month_target_min"], 0,
            "assistant full-month target must remain 0"
        );

        let (st, body) = admin
            .get(&format!("/api/v1/reports/team?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "team report for assistant checks");
        let row = body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["user_id"].as_i64() == Some(assistant_id))
            .expect("assistant row present in team report");
        assert!(
            row["flextime_balance_min"].is_null(),
            "assistant team flextime balance must be null"
        );
        assert!(
            row["diff_min"].is_null(),
            "assistant team monthly diff must be null"
        );
    }

    // -- Range, overtime, and team category reports enforce scope and aggregate correctly --
    {
        let (lead_id, lead_pw, emp_id, emp_pw, monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "6").await;
        let lead = login_change_pw(&app, "lead-6@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-6@example.com", &emp_pw).await;
        let tuesday = (chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap()
            + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "range approved"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create monday entry");
        let monday_entry = id(&body);

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": tuesday,
                    "start_time": "09:00",
                    "end_time": "11:00",
                    "category_id": cat_id,
                    "comment": "range draft"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create tuesday entry");
        let tuesday_entry = id(&body);

        let (st, _) = emp
            .post(
                "/api/v1/time-entries/submit",
                &json!({"ids": [monday_entry, tuesday_entry]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "submit both entries");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [monday_entry]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve monday entry");

        let (st, range_body) = emp
            .get(&format!(
                "/api/v1/reports/range?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "own range report");
        assert_eq!(
            range_body["actual_min"], 240,
            "range actual counts only approved time"
        );
        assert_eq!(
            range_body["submitted_min"], 360,
            "submitted_min includes submitted-but-not-yet-approved work"
        );

        let (st, _) = emp
            .get(&format!(
                "/api/v1/reports/range?user_id={lead_id}&from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "employee cannot read another user's range report"
        );

        let monday_year = &monday[..4];
        let (st, overtime_body) = emp
            .get(&format!("/api/v1/reports/overtime?year={monday_year}"))
            .await;
        assert_eq!(st, StatusCode::OK, "own overtime report");
        assert!(
            overtime_body
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["month"] == monday[..7]),
            "overtime contains the active month"
        );

        let (st, lead_team_categories) = lead
            .get(&format!(
                "/api/v1/reports/team-categories?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead team category report");
        let rows = lead_team_categories.as_array().unwrap();
        assert!(rows
            .iter()
            .any(|row| row["user_id"].as_i64() == Some(emp_id)));
        let emp_row = rows
            .iter()
            .find(|row| row["user_id"].as_i64() == Some(emp_id))
            .expect("employee row in team categories");
        assert!(
            emp_row["categories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|cat| cat["minutes"].as_i64().unwrap_or(0) >= 360),
            "team categories aggregate submitted and approved entry minutes"
        );
    }

    // -- Assistant overtime is empty and admin subjects are excluded from lead-scoped team categories --
    {
        let (lead_id, lead_pw, _emp_id, _emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "7").await;
        let lead = login_change_pw(&app, "lead-7@example.com", &lead_pw).await;

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"assistant-report@example.com",
                    "first_name":"Assist",
                    "last_name":"Report",
                    "role":"assistant",
                    "weekly_hours":0,
                    "start_date":"2024-01-01",
                    "approver_ids":[lead_id]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create assistant report user");
        let assistant_id = id(&body);
        let assistant_pw = temp_pw(&body);
        let assistant = login_change_pw(&app, "assistant-report@example.com", &assistant_pw).await;

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"admin-report-subject@example.com",
                    "first_name":"Admin",
                    "last_name":"Subject",
                    "role":"admin",
                    "weekly_hours":39,
                    "start_date":"2024-01-01",
                    "approver_ids":[1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin subject");
        let admin_subject_id = id(&body);

        let monday_year = &monday[..4];
        let (st, assistant_overtime) = assistant
            .get(&format!("/api/v1/reports/overtime?year={monday_year}"))
            .await;
        assert_eq!(st, StatusCode::OK, "assistant overtime request succeeds");
        assert_eq!(
            assistant_overtime.as_array().unwrap().len(),
            0,
            "assistants have no overtime rows"
        );

        let (st, team_categories) = lead
            .get(&format!(
                "/api/v1/reports/team-categories?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead team categories loads");
        let rows = team_categories.as_array().unwrap();
        assert!(
            rows.iter()
                .any(|row| row["user_id"].as_i64() == Some(assistant_id)),
            "assistant direct report stays visible"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row["user_id"].as_i64() == Some(admin_subject_id)),
            "admin subjects are excluded from lead-scoped team category reports"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn report_permission_guards_reject_non_reportable_users_on_every_personal_endpoint() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, _emp_id, _emp_pw, monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "permission-guard").await;
    let month = monday[..7].to_string();
    let year = &monday[..4];

    let (status, pure_admin_body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "reports-pure-admin-target@example.com",
                "first_name": "Pure",
                "last_name": "ReportTarget",
                "role": "admin",
                "tracks_time": false,
                "weekly_hours": 39,
                "start_date": "2024-01-01"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create pure-admin report target");
    let pure_admin_id = id(&pure_admin_body);
    let pure_admin_password = temp_pw(&pure_admin_body);
    let pure_admin = login_change_pw(
        &app,
        "reports-pure-admin-target@example.com",
        &pure_admin_password,
    )
    .await;

    let (status, inactive_body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "reports-inactive-target@example.com",
                "first_name": "Inactive",
                "last_name": "ReportTarget",
                "role": "employee",
                "weekly_hours": 39,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create inactive report target");
    let inactive_id = id(&inactive_body);
    // Archive the report target (archive sets active=FALSE; archived users are
    // still included in historical reports since they had time data).
    let (status, _) = admin
        .post(&format!("/api/v1/users/{inactive_id}/archive"), &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "archive report target");

    let personal_paths = |target_id: i64| {
        vec![
            format!("/api/v1/reports/month?user_id={target_id}&month={month}"),
            format!("/api/v1/reports/month/csv?user_id={target_id}&month={month}"),
            format!("/api/v1/reports/range?user_id={target_id}&from={monday}&to={monday}"),
            format!("/api/v1/reports/csv?user_id={target_id}&from={monday}&to={monday}"),
            format!("/api/v1/reports/categories?user_id={target_id}&from={monday}&to={monday}"),
            format!("/api/v1/reports/overtime?user_id={target_id}&year={year}"),
            format!("/api/v1/reports/flextime?user_id={target_id}&from={monday}&to={monday}"),
        ]
    };

    for path in personal_paths(pure_admin_id) {
        assert_get_forbidden(&admin, &path, "admin cannot report on a pure-admin account").await;
    }
    for path in personal_paths(inactive_id) {
        assert_get_forbidden(&admin, &path, "admin cannot report on an inactive account").await;
    }

    let self_paths = vec![
        format!("/api/v1/reports/month?month={month}"),
        format!("/api/v1/reports/month/csv?month={month}"),
        format!("/api/v1/reports/range?from={monday}&to={monday}"),
        format!("/api/v1/reports/csv?from={monday}&to={monday}"),
        format!("/api/v1/reports/categories?user_id={pure_admin_id}&from={monday}&to={monday}"),
        format!("/api/v1/reports/overtime?year={year}"),
        format!("/api/v1/reports/flextime?from={monday}&to={monday}"),
    ];
    for path in self_paths {
        assert_get_forbidden(
            &pure_admin,
            &path,
            "pure-admin cannot default or explicitly report on themselves",
        )
        .await;
    }

    app.cleanup().await;
}

/// A non-assistant employee with `weekly_hours=0` is a non-booking user: the
/// monthly submission reminder already skips them, so week-completeness
/// checks (Submissions tile, team report) must exempt them too — otherwise
/// they are flagged "weeks missing" by a check tied to a reminder that never
/// fires. A normal employee with unlogged past weeks is used as a control to
/// confirm the exemption is specific to zero hours, not a blanket pass.
#[tokio::test]
async fn zero_weekly_hours_employee_exempt_from_week_completeness_checks() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "zero-hours").await;
    let lead = login_change_pw(&app, "lead-zero-hours@example.com", &lead_pw).await;
    let month = monday[..7].to_string();

    let (st, zero_hours_body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "zero-hours-emp@example.com",
                "first_name": "Zero",
                "last_name": "Hours",
                "role": "employee",
                "weekly_hours": 0,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create zero-weekly-hours employee");
    let zero_hours_id = id(&zero_hours_body);

    let (st, control_body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "normal-hours-emp@example.com",
                "first_name": "Normal",
                "last_name": "Hours",
                "role": "employee",
                "weekly_hours": 39,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create normal-hours control employee");
    let control_id = id(&control_body);

    // Neither employee logs any time entries for the fully elapsed past week
    // returned by bootstrap (`monday`, two weeks ago) — the control employee
    // must be flagged "weeks missing" for it; the zero-hours employee must not.
    let (st, zero_hours_report) = admin
        .get(&format!(
            "/api/v1/reports/month?user_id={zero_hours_id}&month={month}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "zero-hours employee month report");
    assert_eq!(
        zero_hours_report["weeks_all_submitted"], true,
        "zero-weekly-hours employee is exempt from week-completeness checks"
    );
    assert_eq!(zero_hours_report["weeks_all_approved"], true);

    let (st, control_report) = admin
        .get(&format!(
            "/api/v1/reports/month?user_id={control_id}&month={month}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "control employee month report");
    assert_eq!(
        control_report["weeks_all_submitted"], false,
        "a normal employee with unlogged past weeks is still flagged (control case)"
    );

    // The team report's per-row `weeks_all_submitted` must reflect the same
    // exemption for the same reason.
    let (st, team_rows) = lead
        .get(&format!("/api/v1/reports/team?month={month}"))
        .await;
    assert_eq!(st, StatusCode::OK, "team report");
    let team_rows = team_rows["rows"].as_array().unwrap();
    let zero_hours_row = team_rows
        .iter()
        .find(|row| row["user_id"].as_i64() == Some(zero_hours_id))
        .expect("zero-hours employee present in team report");
    assert_eq!(
        zero_hours_row["weeks_all_submitted"], true,
        "team report exempts the zero-weekly-hours employee too"
    );
    let control_row = team_rows
        .iter()
        .find(|row| row["user_id"].as_i64() == Some(control_id))
        .expect("control employee present in team report");
    assert_eq!(
        control_row["weeks_all_submitted"], false,
        "team report still flags the normal-hours control employee"
    );

    app.cleanup().await;
}

/// The combined "all employees" timesheet PDF (`GET /reports/pdf` with no
/// `user_id`) must order its per-user sections by role — team leads, then
/// employees, then assistants, then admins — and alphabetically within each
/// role, matching every on-screen user roster. This mirrors
/// `roles::role_sort_rank` / `services::reports::build_team_timesheet_sections`.
///
/// The names below are chosen so a naive global-alphabetical order would give a
/// different sequence (the team lead's last name sorts last, the admin's sorts
/// first), so the assertion genuinely exercises the role grouping rather than
/// coincidental alphabetical order.
#[tokio::test]
async fn combined_timesheet_pdf_orders_sections_by_role_then_name() {
    let app = TestApp::spawn().await;
    // The seeded admin is "Test Admin" (role admin, tracks_time defaults TRUE).
    let admin = admin_login(&app).await;

    // Team lead whose last name ("Zeta") sorts last alphabetically — role rank
    // must still place them first.
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"tl-pdf@example.com","first_name":"Tim","last_name":"Zeta",
                "role":"team_lead","weekly_hours":39,
                "start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create team lead");

    // Two employees prove alphabetical ordering *within* the employee group.
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"empa-pdf@example.com","first_name":"Ann","last_name":"Alpha",
                "role":"employee","weekly_hours":39,
                "start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee Alpha");
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"empb-pdf@example.com","first_name":"Bob","last_name":"Beta",
                "role":"employee","weekly_hours":39,
                "start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee Beta");

    // Assistant: weekly_hours must be 0 with no fixed workdays per week.
    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"asst-pdf@example.com","first_name":"Sam","last_name":"Sigma",
                "role":"assistant","weekly_hours":0,"workdays_per_week":null,
                "start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create assistant");

    let (st, _) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"technical-admin-pdf@example.com","first_name":"Nora","last_name":"NoTime",
                "role":"admin","tracks_time":false,"weekly_hours":39,
                "start_date":"2024-01-01"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create non-tracking admin");

    // Omitting user_id requests the combined team export (admin scope = all).
    let from = date_offset(-7);
    let to = today();
    let (st, pdf) = admin
        .get_raw(&format!("/api/v1/reports/pdf?from={from}&to={to}"))
        .await;
    assert_eq!(st, StatusCode::OK, "combined PDF export");
    assert!(pdf.starts_with("%PDF"), "response body is a PDF");

    // Each section header embeds the user's full name in the (uncompressed)
    // content stream, so section order equals the byte order of those names.
    let position = |needle: &str| {
        pdf.find(needle)
            .unwrap_or_else(|| panic!("section for '{needle}' missing from PDF"))
    };
    let team_lead = position("Tim Zeta");
    let employee_alpha = position("Ann Alpha");
    let employee_beta = position("Bob Beta");
    let assistant = position("Sam Sigma");
    let admin_section = position("Test Admin");

    assert!(
        team_lead < employee_alpha,
        "team lead section precedes employees despite a later last name"
    );
    assert!(
        employee_alpha < employee_beta,
        "employees are alphabetical within their role group (Alpha before Beta)"
    );
    assert!(employee_beta < assistant, "employees precede the assistant");
    assert!(
        assistant < admin_section,
        "assistant precedes the admin, which sorts last despite an early last name"
    );
    assert!(
        !pdf.contains("Nora NoTime"),
        "combined PDF must exclude users with time tracking disabled"
    );

    app.cleanup().await;
}
