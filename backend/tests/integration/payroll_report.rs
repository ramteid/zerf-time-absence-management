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

fn config(assistant_hours: bool, employee_hours: bool) -> PayrollReportConfig {
    PayrollReportConfig {
        enabled: true,
        recipients: vec!["payroll@example.com".into()],
        day_of_month: 1,
        include_assistant_hours: assistant_hours,
        include_employee_hours: employee_hours,
        excluded_user_ids: Vec::new(),
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
            &json!({"payroll_report_enabled": true, "payroll_report_recipients": []}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "recipient is required");
    // The "no section enabled" case (both hours off and no payroll-relevant
    // absence category exists) is covered at the unit level in
    // `services::payroll_report::tests` — reaching it here would require
    // reconfiguring every seeded category's cost_type first.

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({"payroll_report_recipients": ["not-an-address"]}),
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

    // A complete, valid configuration round-trips through the admin settings.
    // Recipients are equal, order-preserving, and folded case-insensitively.
    let (status, body) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": [" payroll@example.com ", "PAYROLL@example.com", "second@example.com"],
                "payroll_report_day_of_month": 7,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "valid save: {body}");
    assert_eq!(body["payroll_report_enabled"], true);
    assert_eq!(
        body["payroll_report_recipients"],
        json!(["payroll@example.com", "second@example.com"]),
        "case-insensitive duplicates are collapsed"
    );
    assert_eq!(body["payroll_report_day_of_month"], 7);
    // Categories are included automatically: "sick" qualifies as sick-like
    // (auto_approve_past) and the seeded "unpaid" category is flagged
    // unpaid. "vacation" and "flextime_reduction" are excluded because their
    // cost already shows up in the leave/flextime balances. "special_leave"
    // is cost_type='none' but not flagged unpaid, so it must be excluded
    // too: a paid day off doesn't reduce salary, so listing it here would
    // misreport what payroll owes.
    let auto_categories: Vec<String> = body["payroll_report_absence_categories"]
        .as_array()
        .expect("categories array")
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert!(auto_categories.contains(&"sick".to_string()));
    assert!(auto_categories.contains(&"unpaid".to_string()));
    assert!(!auto_categories.contains(&"vacation".to_string()));
    assert!(!auto_categories.contains(&"flextime_reduction".to_string()));
    assert!(
        !auto_categories.contains(&"special_leave".to_string()),
        "paid special leave must not be auto-included just because cost_type=none"
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
    assert_eq!(
        body["payroll_report_recipients"],
        json!(["payroll@example.com", "second@example.com"])
    );
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

/// `cost_type = 'none'` does not by itself mean a day is unpaid — paid
/// special leave and paid training are `cost_type = 'none'` too. The
/// payroll report only auto-includes a category once it is explicitly
/// flagged `unpaid` (or it is sick-like), and toggling that flag takes
/// effect immediately.
#[tokio::test]
async fn payroll_report_only_includes_categories_explicitly_marked_unpaid() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, cats_body) = admin.get("/api/v1/absence-categories/all").await;
    let special_leave_id = cats_body
        .as_array()
        .expect("categories array")
        .iter()
        .find(|c| c["slug"].as_str() == Some("special_leave"))
        .expect("special_leave seeded category exists")["id"]
        .as_i64()
        .expect("id is int");

    let (status, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(status, StatusCode::OK);
    let included = |settings: &serde_json::Value| -> Vec<String> {
        settings["payroll_report_absence_categories"]
            .as_array()
            .expect("categories array")
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect()
    };
    assert!(
        !included(&settings).contains(&"special_leave".to_string()),
        "cost_type='none' alone must not make a paid category payroll-relevant"
    );

    // Setting unpaid=true on a cost_type='vacation' category is nonsensical
    // (vacation is always paid through its own balance mechanics) and is
    // rejected by the same invariant the DB CHECK enforces.
    let vacation_id = cats_body
        .as_array()
        .expect("categories array")
        .iter()
        .find(|c| c["slug"].as_str() == Some("vacation"))
        .expect("vacation seeded category exists")["id"]
        .as_i64()
        .expect("id is int");
    let (status, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{vacation_id}"),
            &json!({"unpaid": true}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unpaid requires cost_type='none': {body}"
    );

    // Flip special_leave to unpaid — it must now appear.
    let (status, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{special_leave_id}"),
            &json!({"unpaid": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "mark special_leave unpaid: {body}");

    let (status, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        included(&settings).contains(&"special_leave".to_string()),
        "explicitly unpaid categories are included"
    );

    // Flipping it back off removes it again.
    let (status, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{special_leave_id}"),
            &json!({"unpaid": false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!included(&settings).contains(&"special_leave".to_string()));

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
        &config(true, false),
        &language,
        None,
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

/// An unfinished month is a normal business state, not a system fault. It must
/// never reach admins through the technical-error channel — the dashboard tile
/// is where an outstanding payroll report is surfaced now.
#[tokio::test]
async fn payroll_report_never_reports_missing_submissions_as_a_technical_error() {
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
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
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
        zerf::services::reports::month_export_readiness(
            &app.state.pool,
            &assistant_user,
            from,
            to,
            false,
        )
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
        !queued_errors.iter().any(|entry| entry
            .dedupe_key
            .as_deref()
            .is_some_and(|key| key.starts_with("payroll_report_blocked_"))),
        "people who have not submitted yet must not raise a technical error"
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

/// The dashboard and the send path use the same assistant relevance rule. An
/// assistant with booked hours is amber until approval, while an assistant
/// without any entry in the month is absent from the status altogether.
#[tokio::test]
async fn payroll_status_tracks_only_assistants_with_month_activity() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-assistant-status").await;
    let lead = login_change_pw(&app, "lead-payroll-assistant-status@example.com", &lead_pw).await;
    let (active_assistant_id, active_assistant_pw) =
        create_assistant(&admin, lead_id, "assistant-status-active").await;
    let (inactive_assistant_id, _inactive_assistant_pw) =
        create_assistant(&admin, lead_id, "assistant-status-inactive").await;
    let active_assistant = login_change_pw(
        &app,
        "aushilfe-assistant-status-active@example.com",
        &active_assistant_pw,
    )
    .await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let monday = anchor_monday();
    let day = monday.format("%Y-%m-%d").to_string();
    let entry_id = create_and_submit_entry(&active_assistant, &day, cat_id).await;

    let (status, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    let members = card["members"].as_array().expect("payroll members");
    let active_member = members
        .iter()
        .find(|member| member["user_id"].as_i64() == Some(active_assistant_id))
        .unwrap_or_else(|| panic!("active assistant missing from payroll status: {card}"));
    assert_eq!(
        active_member["status"], "awaiting_approval",
        "booked but unapproved assistant hours must be amber"
    );
    assert!(
        !members
            .iter()
            .any(|member| member["user_id"].as_i64() == Some(inactive_assistant_id)),
        "an assistant without month activity must not be counted or displayed"
    );

    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve assistant entry");

    let (status, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    let active_member = card["members"]
        .as_array()
        .expect("payroll members")
        .iter()
        .find(|member| member["user_id"].as_i64() == Some(active_assistant_id))
        .unwrap_or_else(|| panic!("active assistant missing after approval: {card}"));
    assert_eq!(active_member["status"], "ready");

    app.cleanup().await;
}

/// Assistants only matter in months in which they recorded time. The same
/// period-aware selection removes explicitly excluded people and admins.
#[tokio::test]
async fn payroll_members_drops_inactive_assistants_and_excluded_people() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-filter").await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "filter").await;
    let (inactive_assistant_id, _inactive_assistant_pw) =
        create_assistant(&admin, lead_id, "filter-inactive").await;
    let assistant = login_change_pw(&app, "aushilfe-filter@example.com", &assistant_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let day = monday.format("%Y-%m-%d").to_string();
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": day,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "work"
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create draft assistant entry: {body}"
    );

    let covered = payroll_report::payroll_members(&app.state, from, to, &[])
        .await
        .expect("covered payroll members");
    assert!(
        !covered.iter().any(|member| member.role == "admin"),
        "admins never appear in the payroll report"
    );
    for expected in [lead_id, emp_id, assistant_id] {
        assert!(
            covered.iter().any(|member| member.id == expected),
            "user {expected} is covered by the report"
        );
    }
    assert!(
        !covered
            .iter()
            .any(|member| member.id == inactive_assistant_id),
        "an assistant without a time entry is irrelevant for this month"
    );

    // Excluding an employee and an assistant removes exactly those two.
    let narrowed = payroll_report::payroll_members(&app.state, from, to, &[emp_id, assistant_id])
        .await
        .expect("narrowed payroll members");
    assert!(!narrowed.iter().any(|member| member.id == emp_id));
    assert!(!narrowed.iter().any(|member| member.id == assistant_id));
    assert!(
        narrowed.iter().any(|member| member.id == lead_id),
        "people who were not excluded stay in"
    );

    app.cleanup().await;
}

/// The exclusion list survives a save/load round trip through `app_settings`.
#[tokio::test]
async fn payroll_report_settings_persist_the_exclusion_list() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-excl").await;

    let (status, body) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({ "payroll_report_excluded_user_ids": [emp_id, lead_id] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "save exclusion list");
    assert_eq!(
        body["payroll_report_excluded_user_ids"]
            .as_array()
            .expect("excluded list")
            .iter()
            .filter_map(|value| value.as_i64())
            .collect::<Vec<_>>(),
        vec![emp_id, lead_id],
        "the saved order is preserved"
    );

    let stored = payroll_report::load_config(&app.state.pool)
        .await
        .expect("load config");
    assert_eq!(stored.excluded_user_ids, vec![emp_id, lead_id]);

    // Clearing it puts everybody back in.
    let (status, body) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({ "payroll_report_excluded_user_ids": [] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "clear exclusion list");
    assert!(body["payroll_report_excluded_user_ids"]
        .as_array()
        .expect("excluded list")
        .is_empty());

    app.cleanup().await;
}

/// The dashboard tile must give a team lead the true company-wide picture —
/// otherwise they cannot tell whether the report is ready to go — while never
/// leaking the names of people outside their team.
#[tokio::test]
async fn payroll_status_counts_everyone_but_anonymizes_outside_a_leads_team() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-status").await;
    let lead = login_change_pw(&app, "lead-payroll-status@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-payroll-status@example.com", &emp_pw).await;

    // A second lead with their own employee, so each lead has somebody the
    // other one is not allowed to see.
    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email": "lead2-payroll-status@example.com", "first_name": "Otto",
                "last_name": "Other", "role": "team_lead", "weekly_hours": 39,
                "start_date": "2024-01-01", "approver_ids": [1]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create second lead");
    let other_lead_id = id(&body);

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": false,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "store payroll settings");

    // Switched off: the tile has nothing to show and stays hidden.
    let (status, body) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["enabled"],
        json!(false),
        "disabled report hides the tile"
    );

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({ "payroll_report_enabled": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let (status, admin_view) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    let total = admin_view["total"].as_u64().expect("total");
    assert!(total > 0, "the previous month covers somebody");
    assert_eq!(
        admin_view["ready"].as_u64().unwrap()
            + admin_view["awaiting_approval"].as_u64().unwrap()
            + admin_view["not_submitted"].as_u64().unwrap(),
        total,
        "every covered person lands in exactly one bucket"
    );
    let admin_members = admin_view["members"].as_array().expect("members");
    assert_eq!(admin_members.len() as u64, total);
    assert!(
        admin_members.iter().all(|member| !member["name"].is_null()),
        "an admin sees every name"
    );
    assert!(
        !admin_members
            .iter()
            .any(|member| member["user_id"].as_i64() == Some(1)),
        "the admin account itself is not part of the payroll report"
    );

    let (status, lead_view) = lead.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        lead_view["total"].as_u64().unwrap(),
        total,
        "a team lead sees the true company-wide count"
    );
    let lead_members = lead_view["members"].as_array().expect("members");
    let named: Vec<i64> = lead_members
        .iter()
        .filter_map(|member| member["user_id"].as_i64())
        .collect();
    assert!(named.contains(&lead_id), "a lead sees themselves");
    assert!(named.contains(&emp_id), "a lead sees their own report");
    assert!(
        !named.contains(&other_lead_id),
        "a lead must not see people outside their team"
    );
    // The hidden person is still counted, just stripped of any identity.
    let hidden: Vec<&serde_json::Value> = lead_members
        .iter()
        .filter(|member| member["user_id"].is_null())
        .collect();
    assert!(!hidden.is_empty(), "the outside person is still listed");
    assert!(
        hidden.iter().all(|member| member["name"].is_null()),
        "no name leaks for people the lead may not see"
    );

    // The tile is a lead-only feature.
    let (status, _) = employee.get("/api/v1/reports/payroll-status").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "employees have no payroll tile"
    );

    app.cleanup().await;
}

/// A month that covers nobody (a fresh installation, or everyone excluded) has
/// nothing to report. It must be settled rather than retried every night
/// forever — otherwise the queue grows without bound and the dashboard card
/// stays stuck on an outstanding month that can never be delivered.
#[tokio::test]
async fn payroll_report_settles_a_month_that_covers_nobody() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-empty").await;

    configure_unreachable_smtp(&app).await;

    // Exclude everyone the period could cover, so the report has no subject.
    let everyone: Vec<i64> = app
        .state
        .db
        .users
        .find_all_ordered()
        .await
        .expect("users")
        .into_iter()
        .map(|user| user.id)
        .collect();
    assert!(everyone.contains(&emp_id));

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
                "payroll_report_excluded_user_ids": everyone,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable with everyone excluded");

    // The scheduled run settles the empty month instead of parking it.
    zerf::background::payroll_report::run_once(&app.state)
        .await
        .expect("scheduled run");

    let pending = app
        .state
        .db
        .payroll_queue
        .list_pending()
        .await
        .expect("queue");
    assert!(
        pending.is_empty(),
        "a month with nobody in it must not stay queued forever: {pending:?}"
    );

    // With the period gone from the queue (and recorded as reached), the
    // dashboard card reports the month as done rather than staying live
    // forever on a delivery that can never happen.
    let (status, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["total"], json!(0), "nobody is covered");
    assert_eq!(
        card["sent"],
        json!(true),
        "a settled month greys the dashboard card out"
    );

    app.cleanup().await;
}

/// The card must not claim an outstanding delivery on an installation that has
/// been running for a while: months whose report already went out are gone
/// from the queue, and that is what "already sent" is read from.
#[tokio::test]
async fn payroll_status_reports_an_already_delivered_month_as_sent() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-sent").await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let period = zerf::background::schedule::previous_period(today);

    // Not queued yet (before the send day): outstanding, so the card is live.
    let (_, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(card["period"], json!(period));
    assert_eq!(
        card["sent"],
        json!(false),
        "a month that has not been queued yet is still outstanding"
    );

    // Queued and still waiting: outstanding.
    app.state
        .db
        .payroll_queue
        .enqueue(&period)
        .await
        .expect("enqueue");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record queue period");
    let (_, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(card["sent"], json!(false), "a queued month is outstanding");

    // Delivered: the scheduler drops the queue entry, and the card follows.
    app.state
        .db
        .payroll_queue
        .delete_entry(&period)
        .await
        .expect("delete entry");
    let (_, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(
        card["sent"],
        json!(true),
        "a delivered month greys the card out"
    );

    app.cleanup().await;
}

/// The card's colours must not reuse the send gate's relaxed rule, which only
/// requires approval when a person's hours literally appear in the PDF. With
/// employee hours excluded by default, a regular employee's submitted-but-
/// unapproved month must still show amber ("submitted, not yet approved") on
/// the tile — not green, which would silently misreport them as finished.
#[tokio::test]
async fn payroll_status_requires_approval_even_when_hours_are_not_in_the_report() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-approval-gate").await;

    // A zero-weekly-hours "employee" isolates the one condition this test
    // exists to check: `has_submission_obligation` exempts them from the
    // weekly-submission gate (same as an assistant), so a single unapproved
    // entry is the *only* thing keeping their month from `Ready` — a real
    // full-time employee would also need every day of the week populated,
    // which is unrelated setup noise for what this test is proving. They
    // still fall into the "employee hours" bucket (`is_assistant_role` is
    // false for role `employee`), so `include_employee_hours=false` (the
    // default) is exactly the condition under test: their hours are not part
    // of the report content, and must still gate their card colour.
    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "zero-hours-payroll-approval-gate@example.com",
                "first_name": "Zoe", "last_name": "ZeroHours",
                "role": "employee", "weekly_hours": 0,
                "start_date": "2024-01-01", "approver_ids": [lead_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create zero-hours employee: {body}");
    let emp_id = id(&body);
    let emp_pw = temp_pw(&body);
    let employee = login_change_pw(
        &app,
        "zero-hours-payroll-approval-gate@example.com",
        &emp_pw,
    )
    .await;

    // Default configuration: assistant hours included, employee hours are not
    // — so this employee's working time never appears in the PDF.
    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let monday = anchor_monday();
    let day = monday.format("%Y-%m-%d").to_string();
    create_and_submit_entry(&employee, &day, cat_id).await;

    let (status, card) = admin.get("/api/v1/reports/payroll-status").await;
    assert_eq!(status, StatusCode::OK);
    let member = card["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["user_id"].as_i64() == Some(emp_id))
        .unwrap_or_else(|| panic!("employee not found in payroll status: {card}"));
    assert_eq!(
        member["status"], "awaiting_approval",
        "submitted-but-unapproved must be amber even though this employee's \
         hours are not part of the report content: {member}"
    );

    app.cleanup().await;
}
