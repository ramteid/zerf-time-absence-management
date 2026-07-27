//! Monthly payroll report: settings endpoint, report content, and the
//! readiness gate that decides when a month may be mailed out.

use chrono::{Datelike, Duration, NaiveDate};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{TestApp, TestClient};
use crate::helpers::*;
use zerf::services::payroll_report::{self, PayrollReportConfig};

/// Monday a few weeks back whose Monday-to-Wednesday block stays inside a
/// single calendar month, so the period under test is unambiguous and every
/// date is safely in the past.
fn anchor_monday() -> NaiveDate {
    for weeks_back in [3, 4, 2] {
        let monday = next_monday(-7 * weeks_back);
        if (monday + Duration::days(2)).month() == monday.month() {
            return monday;
        }
    }
    panic!("no anchor monday with a Mon-Wed block inside one month");
}

fn month_bounds(date: NaiveDate) -> (NaiveDate, NaiveDate) {
    let from = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
    let last_day = zerf::time_calc::last_day_of_month(date.year(), date.month());
    (
        from,
        NaiveDate::from_ymd_opt(date.year(), date.month(), last_day).unwrap(),
    )
}

fn config(slugs: &[&str], assistant_hours: bool, employee_hours: bool) -> PayrollReportConfig {
    PayrollReportConfig {
        enabled: true,
        recipient: "payroll@example.com".into(),
        day_of_month: 1,
        absence_category_slugs: slugs.iter().map(|slug| slug.to_string()).collect(),
        include_assistant_hours: assistant_hours,
        include_employee_hours: employee_hours,
    }
}

/// Point SMTP at a closed local port: `load_smtp_config` returns a config (so
/// the send path is reachable) while no mail can ever leave the test machine.
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

async fn create_assistant(admin: &TestClient, approver_id: i64, suffix: &str) -> (i64, String) {
    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": format!("aushilfe-{suffix}@example.com"),
                "first_name": "Alex",
                "last_name": format!("Assist{suffix}"),
                "role": "assistant",
                "weekly_hours": 0,
                "leave_days_current_year": 0,
                "leave_days_next_year": 0,
                "annual_leave_days": 0,
                "start_date": "2024-01-01",
                "approver_ids": [approver_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create assistant: {body}");
    (id(&body), temp_pw(&body))
}

#[tokio::test]
async fn payroll_report_settings_are_validated_and_persisted() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Enabling without a recipient is rejected — the report could never be sent.
    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_enabled": true, "payroll_report_recipient": ""}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "recipient is required");

    // Enabling without any section is rejected — an empty PDF helps nobody.
    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipient": "payroll@example.com",
                "payroll_report_absence_categories": [],
                "payroll_report_include_assistant_hours": false,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "content is required");

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_recipient": "not-an-address"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "recipient must be valid");

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_day_of_month": 29}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "day must be 1-28");

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_absence_categories": ["does_not_exist"]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unknown category rejected");

    // A complete, valid configuration round-trips through the admin settings.
    let (status, body) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipient": " payroll@example.com ",
                "payroll_report_day_of_month": 7,
                "payroll_report_absence_categories": ["sick", "unpaid", "sick"],
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "valid save: {body}");
    assert_eq!(body["payroll_report_enabled"], true);
    assert_eq!(body["payroll_report_recipient"], "payroll@example.com");
    assert_eq!(body["payroll_report_day_of_month"], 7);
    assert_eq!(
        body["payroll_report_absence_categories"],
        json!(["sick", "unpaid"]),
        "duplicates are collapsed"
    );

    let (status, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["payroll_report_include_assistant_hours"], true);
    assert_eq!(settings["payroll_report_include_employee_hours"], false);

    // Omitted fields keep their stored value.
    let (status, body) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_day_of_month": 3}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "partial save: {body}");
    assert_eq!(body["payroll_report_recipient"], "payroll@example.com");
    assert_eq!(body["payroll_report_day_of_month"], 3);

    // Non-admins may not touch the configuration or trigger a run.
    let (_lead_id, lead_pw, _emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-perm").await;
    let lead = login_change_pw(&app, "lead-payroll-perm@example.com", &lead_pw).await;
    let (status, _) = lead
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_day_of_month": 9}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "lead cannot configure");
    let (status, _) = lead
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "lead cannot trigger a run");

    app.cleanup().await;
}

#[tokio::test]
async fn payroll_report_lists_absence_days_and_assistant_hours() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-data").await;
    let lead = login_change_pw(&app, "lead-payroll-data@example.com", &lead_pw).await;
    let _employee = login_change_pw(&app, "emp-payroll-data@example.com", &emp_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "data").await;
    let assistant = login_change_pw(&app, "aushilfe-data@example.com", &assistant_pw).await;

    let monday = anchor_monday();
    let wednesday = monday + Duration::days(2);
    let (from, to) = month_bounds(monday);

    // Employee: a three-day sick absence inside the reported month.
    let sick = absence_cat(&app.state.pool, "sick").await;
    app.state
        .db
        .absences
        .create(emp_id, sick.id, true, monday, wednesday, None, "approved")
        .await
        .expect("create sick absence");

    // Employee: a weekend-only unpaid absence, which has no payroll effect and
    // must therefore not produce a row.
    let unpaid = absence_cat(&app.state.pool, "unpaid").await;
    let saturday = monday + Duration::days(5);
    app.state
        .db
        .absences
        .create(
            emp_id,
            unpaid.id,
            false,
            saturday,
            saturday + Duration::days(1),
            None,
            "approved",
        )
        .await
        .expect("create weekend absence");

    // Assistant: two approved half-days (4 h each) inside the same month.
    let mut entry_ids = Vec::new();
    for offset in [0, 1] {
        let day = (monday + Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string();
        entry_ids.push(create_and_submit_entry(&assistant, &day, cat_id).await);
    }
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": entry_ids}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve assistant entries");

    let members = app
        .state
        .db
        .reports
        .timesheet_members_for_period(from, to)
        .await
        .expect("members");
    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        from,
        to,
        &members,
        &config(&["sick", "unpaid"], true, false),
        &language,
    )
    .await
    .expect("build report data");

    let absence_rows = data.absence_rows.as_ref().expect("absence section enabled");
    assert_eq!(
        absence_rows.len(),
        1,
        "only the sick absence has payroll-relevant days"
    );
    let row = &absence_rows[0];
    assert!(
        row.employee.contains("Emp"),
        "employee name: {}",
        row.employee
    );
    assert_eq!(row.category, sick.name);
    assert_eq!(row.from, monday);
    assert_eq!(row.to, wednesday);
    let holidays = app
        .state
        .db
        .reports
        .holiday_set(from, to)
        .await
        .expect("holidays");
    assert_eq!(
        row.days,
        zerf::time_calc::count_workdays(monday, wednesday, &holidays, 5),
        "days are contract workdays without holidays"
    );

    assert_eq!(data.hours_sections.len(), 1, "only assistants requested");
    let assistant_rows = &data.hours_sections[0].rows;
    assert_eq!(assistant_rows.len(), 1, "one assistant in scope");
    assert_eq!(assistant_rows[0].work_days, 2);
    assert_eq!(assistant_rows[0].minutes, 480, "2 x 4 h approved");
    assert!(
        !assistant_rows
            .iter()
            .any(|hours_row| hours_row.employee.contains("Emp")),
        "employees are not part of the assistant section"
    );
    assert_ne!(assistant_id, emp_id);

    // The rendered document is a valid PDF.
    let bytes = zerf::report_pdf::render_payroll_report_pdf(&data, &language);
    assert!(bytes.starts_with(b"%PDF"), "renders a PDF");

    app.cleanup().await;
}

#[tokio::test]
async fn payroll_report_waits_until_every_month_is_final() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-gate").await;
    let lead = login_change_pw(&app, "lead-payroll-gate@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "gate").await;
    let assistant = login_change_pw(&app, "aushilfe-gate@example.com", &assistant_pw).await;

    configure_unreachable_smtp(&app).await;
    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipient": "payroll@example.com",
                "payroll_report_day_of_month": 1,
                "payroll_report_absence_categories": [],
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    // An assistant with a submitted-but-unapproved entry: their hours are in
    // the report, so the month is not payroll-final yet even though assistants
    // are exempt from the weekly submission gate.
    let monday = anchor_monday();
    let day = monday.format("%Y-%m-%d").to_string();
    let entry_id = create_and_submit_entry(&assistant, &day, cat_id).await;

    let (from, to) = month_bounds(monday);
    let assistant_user = app
        .state
        .db
        .users
        .find_by_id(assistant_id)
        .await
        .expect("load assistant")
        .expect("assistant exists");
    assert!(
        zerf::services::reports::month_export_readiness(&app.state.pool, &assistant_user, from, to)
            .await
            .expect("readiness")
            .is_ready(),
        "assistants pass the shared submission gate"
    );
    assert!(
        app.state
            .db
            .reports
            .has_unresolved_time_entries_in_range(assistant_id, from, to)
            .await
            .expect("unresolved check"),
        "the submitted-but-unapproved entry is what holds the report back"
    );

    let (status, _) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "run is triggered");

    let pending = app
        .state
        .db
        .payroll_queue
        .list_pending()
        .await
        .expect("queue");
    assert!(
        !pending.is_empty(),
        "the previous month stays queued while a month is not final"
    );

    let queued_errors = app
        .state
        .db
        .error_queue
        .list_pending(50)
        .await
        .expect("error queue");
    assert!(
        queued_errors.iter().any(|entry| entry
            .dedupe_key
            .as_deref()
            .is_some_and(|key| key.starts_with("payroll_report_blocked_"))),
        "admins are told which month is blocked"
    );

    // Approving the entry settles the assistant's month: their hours are final
    // and no longer hold the report back.
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve assistant entry");
    assert!(
        !app.state
            .db
            .reports
            .has_unresolved_time_entries_in_range(assistant_id, from, to)
            .await
            .expect("unresolved check"),
        "approved hours are payroll-final"
    );

    app.cleanup().await;
}
