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

    // The report is delivered by email only, so it cannot be switched on while
    // SMTP is unconfigured — even with everything else filled in correctly.
    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "email must be set up before enabling"
    );
    configure_unreachable_smtp(&app).await;

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
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
        },
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

/// Absence rows must be grouped by category first, then by employee name
/// within a category, then chronologically within one employee — never by
/// employee name or date across categories. Also covers German category
/// translation, since the report is emailed to a German-speaking accountant.
#[tokio::test]
async fn payroll_report_absence_rows_are_grouped_by_category_then_name_then_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Two independent employees whose last names sort "b" before "z", so an
    // (incorrect) name-first sort would put employee B's row ahead of
    // employee Z's row regardless of category.
    let (_, _, emp_b_id, emp_b_pw, _, _) =
        bootstrap_team_with_suffix(&app, &admin, false, "grp-b").await;
    let (_, _, emp_z_id, emp_z_pw, _, _) =
        bootstrap_team_with_suffix(&app, &admin, false, "grp-z").await;
    let _emp_b = login_change_pw(&app, "emp-grp-b@example.com", &emp_b_pw).await;
    let _emp_z = login_change_pw(&app, "emp-grp-z@example.com", &emp_z_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);

    // Determine the actual category rank at runtime instead of assuming one,
    // since the category order is driven by `sort_order`/name in the DB.
    let relevant = payroll_report::payroll_relevant_categories(&app.state.pool)
        .await
        .expect("relevant categories");
    let sick_rank = relevant
        .iter()
        .position(|c| c.slug == "sick")
        .expect("sick is payroll-relevant");
    let unpaid_rank = relevant
        .iter()
        .position(|c| c.slug == "unpaid")
        .expect("unpaid is payroll-relevant");
    let (low_cat, low_slug, high_cat, high_slug) = if sick_rank < unpaid_rank {
        (
            absence_cat(&app.state.pool, "sick").await,
            "sick",
            absence_cat(&app.state.pool, "unpaid").await,
            "unpaid",
        )
    } else {
        (
            absence_cat(&app.state.pool, "unpaid").await,
            "unpaid",
            absence_cat(&app.state.pool, "sick").await,
            "sick",
        )
    };

    // Employee B (alphabetically first) is booked in the HIGHER-ranked
    // category, so a correct category-first sort must still place employee
    // Z's row(s) before employee B's row.
    app.state
        .db
        .absences
        .create(
            emp_b_id,
            high_cat.id,
            true,
            monday + Duration::days(1),
            monday + Duration::days(1),
            None,
            "approved",
        )
        .await
        .expect("create employee B absence");

    // Employee Z gets two separate periods in the LOWER-ranked category,
    // created out of chronological order, so a correct sort must still print
    // the earlier one first purely by date, not by insertion order.
    app.state
        .db
        .absences
        .create(
            emp_z_id,
            low_cat.id,
            true,
            monday + Duration::days(2),
            monday + Duration::days(2),
            None,
            "approved",
        )
        .await
        .expect("create employee Z later absence");
    app.state
        .db
        .absences
        .create(emp_z_id, low_cat.id, true, monday, monday, None, "approved")
        .await
        .expect("create employee Z earlier absence");

    let members = app
        .state
        .db
        .reports
        .timesheet_members_for_period(from, to)
        .await
        .expect("members");
    let language = zerf::i18n::Language::from_setting("de");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
        },
        &members,
        &config(false, false),
        &language,
        None,
    )
    .await
    .expect("build report data");

    let rows = data.absence_rows.as_ref().expect("absence section enabled");
    assert_eq!(rows.len(), 3, "one row for B, two for Z");

    let de_label = |slug: &str| match slug {
        "sick" => "Krankmeldung",
        "unpaid" => "Unbezahlter Urlaub",
        other => panic!("unexpected slug {other}"),
    };

    // Both of employee Z's low-category rows must come first, in date order,
    // followed by employee B's high-category row.
    assert_eq!(rows[0].category, de_label(low_slug));
    assert!(rows[0].employee.contains("grp-z"), "{}", rows[0].employee);
    assert_eq!(rows[0].from, monday);

    assert_eq!(rows[1].category, de_label(low_slug));
    assert!(rows[1].employee.contains("grp-z"), "{}", rows[1].employee);
    assert_eq!(rows[1].from, monday + Duration::days(2));

    assert_eq!(rows[2].category, de_label(high_slug));
    assert!(rows[2].employee.contains("grp-b"), "{}", rows[2].employee);

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
            true,
            zerf::services::reports::PendingAbsences::Any,
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
    // The report cannot be switched on before email is set up.
    configure_unreachable_smtp(&app).await;
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

    let (status, card) = admin.get("/api/v1/reports/submission-status").await;
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

    let (status, card) = admin.get("/api/v1/reports/submission-status").await;
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

    let covered = payroll_report::payroll_members(&app.state, from, to, &[], false)
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
    let narrowed =
        payroll_report::payroll_members(&app.state, from, to, &[emp_id, assistant_id], false)
            .await
            .expect("narrowed payroll members");
    assert!(!narrowed.iter().any(|member| member.id == emp_id));
    assert!(!narrowed.iter().any(|member| member.id == assistant_id));
    assert!(
        narrowed.iter().any(|member| member.id == lead_id),
        "people who were not excluded stay in"
    );

    // An interim snapshot of the running month widens the "must have booked
    // something" rule from assistants to everybody: only the assistant who
    // actually recorded time survives, while the lead and employee — who have
    // an empty month but owe nothing yet — drop out.
    let snapshot = payroll_report::payroll_members(&app.state, from, to, &[], true)
        .await
        .expect("snapshot payroll members");
    assert!(
        snapshot.iter().any(|member| member.id == assistant_id),
        "somebody who booked time is in the snapshot"
    );
    for absent in [lead_id, emp_id] {
        assert!(
            !snapshot.iter().any(|member| member.id == absent),
            "user {absent} booked nothing this month and is not in the snapshot"
        );
    }

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
    // The report cannot be switched on before email is set up.
    configure_unreachable_smtp(&app).await;
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

    // Switched off: the payroll card has nothing to show and stays hidden.
    // The submissions card is independent of it and stays live.
    let (status, body) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["enabled"],
        json!(false),
        "disabled report hides the payroll card"
    );
    let (status, body) = admin.get("/api/v1/reports/submission-status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["total"].as_u64().is_some(),
        "the submissions card does not depend on the payroll report: {body}"
    );

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({ "payroll_report_enabled": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let (status, admin_view) = admin.get("/api/v1/reports/submission-status").await;
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

    let (status, lead_view) = lead.get("/api/v1/reports/submission-status").await;
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
    let (status, _) = employee.get("/api/v1/reports/submission-status").await;
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
    let (status, card) = admin.get("/api/v1/reports/submission-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["total"], json!(0), "nobody is covered");
    let (status, content) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content["sent"],
        json!(true),
        "a settled month greys the payroll card out"
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
    // The report cannot be switched on before email is set up.
    configure_unreachable_smtp(&app).await;
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
    let (_, card) = admin.get("/api/v1/reports/payroll-content").await;
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
    let (_, card) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(card["sent"], json!(false), "a queued month is outstanding");

    // Delivered: the scheduler drops the queue entry, and the card follows.
    app.state
        .db
        .payroll_queue
        .delete_entry(&period)
        .await
        .expect("delete entry");
    let (_, card) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(
        card["sent"],
        json!(true),
        "a delivered month greys the card out"
    );

    app.cleanup().await;
}

/// "Send now" names the month it will actually send, and that month follows
/// the same "already delivered" rule the dashboard tile uses: the previous
/// month while its report is still owed, the running month once it is done.
#[tokio::test]
async fn payroll_send_now_targets_the_running_month_once_the_previous_one_is_delivered() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    let current = zerf::background::schedule::current_period(today);

    // Nothing delivered yet: the owed month wins, even before the send day.
    let (_, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(
        settings["payroll_report_send_now_period"],
        json!(previous),
        "an undelivered previous month is what Send now targets"
    );

    // Mark the previous month delivered the way the scheduler does: it reached
    // the queue and was removed again once SMTP accepted it.
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &previous,
        )
        .await
        .expect("record queue period");

    let (_, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(
        settings["payroll_report_send_now_period"],
        json!(current),
        "with the previous month done, Send now moves to the running month"
    );

    // A month stuck in the queue behind a late submitter is still owed, even
    // though newer months went out. It must win over the running month, or the
    // button could never push it out: the run sends only the month it names.
    let stuck = zerf::background::schedule::period_before(&previous).expect("older period");
    app.state
        .db
        .payroll_queue
        .enqueue(&stuck)
        .await
        .expect("enqueue stuck period");
    let (_, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(
        settings["payroll_report_send_now_period"],
        json!(stuck),
        "an older undelivered month takes priority over the running month"
    );

    // A month that is owed but has not reached the queue yet counts too: the
    // send path backfills the queue before picking anything up, so the button
    // has to name what that backfill would produce. Rewinding the marker two
    // months makes the run reach for the older of them, not the previous one.
    app.state
        .db
        .payroll_queue
        .delete_entry(&stuck)
        .await
        .expect("clear stuck period");
    let two_back = zerf::background::schedule::period_before(&stuck).expect("older period");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &two_back,
        )
        .await
        .expect("rewind queue period");
    let (_, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(
        settings["payroll_report_send_now_period"],
        json!(stuck),
        "the oldest month the backfill would queue is what Send now names"
    );

    app.cleanup().await;
}

/// The dashboard tile's "show this month" peek (`?current=true`) must report
/// the current, in-progress calendar month instead of the tracked previous
/// period — and must never claim that month as already sent, since a period
/// still in progress can never have reached the delivery queue.
#[tokio::test]
async fn payroll_status_current_flag_reports_the_in_progress_month() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    // The report cannot be switched on before email is set up.
    configure_unreachable_smtp(&app).await;
    let (_lead_id, _lead_pw, _emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-current").await;

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
    let previous = zerf::background::schedule::previous_period(today);
    let current = zerf::background::schedule::current_period(today);

    // Mark the previous period as already delivered — the state the tile is
    // normally in for the rest of the month, which is exactly when the peek
    // button appears.
    app.state
        .db
        .payroll_queue
        .enqueue(&previous)
        .await
        .expect("enqueue");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &previous,
        )
        .await
        .expect("record queue period");
    app.state
        .db
        .payroll_queue
        .delete_entry(&previous)
        .await
        .expect("delete entry");

    // Both cards track the same period and follow the same peek flag.
    for path in [
        "/api/v1/reports/submission-status",
        "/api/v1/reports/payroll-content",
    ] {
        let (_, default_card) = admin.get(path).await;
        assert_eq!(
            default_card["period"],
            json!(previous),
            "{path} keeps tracking the delivered previous month"
        );
        let (_, current_card) = admin.get(&format!("{path}?current=true")).await;
        assert_eq!(current_card["period"], json!(current), "{path} peek");
    }

    // Only the payroll card carries a delivery state, and a month still in
    // progress can never have been delivered.
    let (_, delivered) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(delivered["sent"], json!(true));
    let (_, peek) = admin
        .get("/api/v1/reports/payroll-content?current=true")
        .await;
    assert_eq!(peek["sent"], json!(false));

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
    // The report cannot be switched on before email is set up.
    configure_unreachable_smtp(&app).await;
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

    let (status, card) = admin.get("/api/v1/reports/submission-status").await;
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

/// A weekday in the current month on or before today — the exact window a
/// current-month snapshot reports on. Walks backwards from today so it is
/// stable whichever day the suite runs on. `None` only when the month has not
/// reached a weekday yet (the 1st falling on a weekend), where a snapshot has
/// nothing to cover by definition.
fn current_month_workday_up_to_today(today: NaiveDate) -> Option<NaiveDate> {
    use chrono::Datelike;
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
    let mut day = today;
    while day >= first {
        if day.weekday().num_days_from_monday() < 5 {
            return Some(day);
        }
        day = day.pred_opt()?;
    }
    None
}

/// The interim snapshot of the running month must never go out empty.
///
/// Booking time is not the same as having something to report: only approved
/// entries reach the tables, and mid-month the current week is normally still
/// unapproved. Without a guard the tax office receives a document containing
/// nothing but headings, announced as covering N people.
///
/// SMTP points at a closed port throughout, which is what makes the two states
/// distinguishable: refusing to send returns 200 with `sent: 0`, while getting
/// as far as delivery fails loudly — so a 400 proves a non-empty document was
/// actually built and handed to the mailer.
#[tokio::test]
async fn payroll_snapshot_of_the_running_month_is_never_sent_empty() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-snapshot").await;
    let lead = login_change_pw(&app, "lead-payroll-snapshot@example.com", &lead_pw).await;
    let (_assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "snapshot").await;
    let assistant = login_change_pw(&app, "aushilfe-snapshot@example.com", &assistant_pw).await;

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

    // Mark the previous month delivered so "Send now" moves on to the running
    // one — that is the only way to reach the snapshot path.
    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &previous,
        )
        .await
        .expect("record queue period");
    let current = zerf::background::schedule::current_period(today);
    let (_, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(
        settings["payroll_report_send_now_period"],
        json!(current),
        "the snapshot path is the one under test"
    );

    // Nobody has booked anything in the running month yet.
    let (status, body) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "nothing to send is not an error");
    assert_eq!(body["sent"], json!(0), "an empty month sends nothing: {body}");

    let Some(workday) = current_month_workday_up_to_today(today) else {
        // The month has not reached a weekday yet; there is nothing further to
        // assert, and the run above already covered the empty case.
        app.cleanup().await;
        return;
    };
    let day = workday.format("%Y-%m-%d").to_string();

    // Booked but NOT approved: the member set is now non-empty while every
    // table stays empty. This is the case the guard exists for — before it,
    // this sent a document with nothing in it.
    let entry_id = create_and_submit_entry(&assistant, &day, cat_id).await;
    let (status, body) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "still not an error");
    assert_eq!(
        body["sent"],
        json!(0),
        "booked but unapproved time must not produce a report: {body}"
    );
    assert_eq!(
        body["skipped"],
        json!("nothing_approved"),
        "and the admin is told why: {body}"
    );

    // Approving it gives the document real content, so the send is attempted
    // for real — and fails only because SMTP points nowhere.
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the entry");

    let (status, body) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "approved time makes a real document that is actually sent: {body}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("PAYROLL_SEND_FAILED:")),
        "the delivery failure reaches the admin verbatim: {body}"
    );

    // A snapshot must never settle the running month: it is not owed yet, so
    // nothing may enter or leave the delivery queue on its behalf.
    let queued = app
        .state
        .db
        .payroll_queue
        .list_pending()
        .await
        .expect("queue");
    assert!(
        !queued.contains(&current),
        "the running month is never queued by a snapshot: {queued:?}"
    );

    app.cleanup().await;
}

/// The payroll report is delivered by email and nothing else, so the two
/// settings are coupled in both directions. Only the frontend covered this,
/// against a mock that simulated the cascade itself — so the server could have
/// stopped doing it entirely without a single test noticing, leaving
/// `payroll_report_enabled = true` with SMTP off: the exact state the nightly
/// run then hits forever, silently sending nothing.
#[tokio::test]
async fn disabling_smtp_switches_the_payroll_report_off() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    // Turning SMTP off must take the report with it. Saving SMTP *disabled*
    // skips the server-side connection re-test, so the unreachable host here
    // is not what is being exercised.
    let (status, body) = admin
        .put(
            "/api/v1/settings/smtp",
            &json!({
                "smtp_enabled": false,
                "smtp_host": "127.0.0.1",
                "smtp_port": 1,
                "smtp_from": "zerf@example.com",
                "smtp_encryption": "none",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "disable smtp: {body}");
    assert_eq!(
        body["payroll_report_enabled"],
        json!(false),
        "disabling email must switch automatic delivery off: {body}"
    );

    // And it stays off: re-enabling email is a deliberate decision, resuming
    // delivery to the tax office is another.
    configure_unreachable_smtp(&app).await;
    let (_, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(
        settings["payroll_report_enabled"],
        json!(false),
        "re-enabling email must not silently resume delivery: {settings}"
    );

    // Nothing can be sent while it is off, and the admin is told why.
    let (status, body) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "send-now is refused: {body}");

    app.cleanup().await;
}

/// The config combination production does not use is where both empty-report
/// defects lived: with employees' hours switched on, every employee who merely
/// *booked* something produces a row, and for a month still running that row
/// reads "0 days, 0:00".
///
/// Two things must hold. A document made only of such rows is not sent at all,
/// and once something is approved the notice's headline count must match the
/// number of people the tables actually list — a report claiming to cover one
/// person above a table naming ten would misrepresent itself to the tax office.
#[tokio::test]
async fn payroll_snapshot_with_employee_hours_never_reports_empty_rows() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-emp-hours").await;
    let lead = login_change_pw(&app, "lead-payroll-emp-hours@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-payroll-emp-hours@example.com", &emp_pw).await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                // The toggle that used to defeat the emptiness guard.
                "payroll_report_include_employee_hours": true,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &previous,
        )
        .await
        .expect("record queue period");

    let Some(workday) = current_month_workday_up_to_today(today) else {
        app.cleanup().await;
        return;
    };
    let day = workday.format("%Y-%m-%d").to_string();

    // Booked but unapproved: with employees' hours on, this produces a
    // "0 days, 0:00" row. It is not content, so nothing may be sent.
    let entry_id = create_and_submit_entry(&employee, &day, cat_id).await;
    let (status, body) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "not an error: {body}");
    assert_eq!(
        body["sent"],
        json!(0),
        "a table of zero rows is not a report: {body}"
    );
    assert_eq!(body["skipped"], json!("nothing_approved"), "{body}");

    // Approving it turns the same person into real content.
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the entry");

    // The document now has figures, so delivery is genuinely attempted and
    // fails only because SMTP points at a closed port.
    let (status, body) = admin
        .post("/api/v1/settings/payroll-report/send-now", &json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "approved time is sent for real: {body}"
    );

    // The headline count must equal the people the tables name. Assemble the
    // same interim document the send path builds and compare the two.
    let (from, _month_end) = zerf::background::schedule::period_bounds(
        &zerf::background::schedule::current_period(today),
    )
    .expect("period bounds");
    let window = payroll_report::ReportWindow {
        from,
        to: today,
        interim: true,
        created_on: today,
    };
    let members = payroll_report::payroll_members(&app.state, from, today, &[], true)
        .await
        .expect("members");
    let data = payroll_report::build_report_data(
        &app.state,
        window,
        &members,
        &config(true, true),
        &zerf::i18n::Language::default(),
        None,
    )
    .await
    .expect("report data");

    let listed: std::collections::HashSet<&str> = data
        .hours_sections
        .iter()
        .flat_map(|section| section.rows.iter())
        .map(|row| row.employee.as_str())
        .chain(
            data.absence_rows
                .iter()
                .flatten()
                .map(|row| row.employee.as_str()),
        )
        .collect();
    assert_eq!(
        payroll_report::people_in_report(&data),
        listed.len(),
        "the announced count must equal the names in the tables"
    );
    // The document states when it was assembled: the same month can be sent
    // again later as the final report, so the recipient has to be able to tell
    // the two copies apart and see how current the figures are.
    assert_eq!(
        data.created_on, today,
        "the report carries its creation date"
    );
    assert!(
        data.hours_sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .all(|row| row.work_days > 0 || row.minutes > 0),
        "an interim report never lists somebody as having done nothing"
    );
    // The employee who booked and got approved is in; nobody else in the team
    // booked anything, so the interim window leaves them out entirely.
    assert_eq!(listed.len(), 1, "only the approved employee is listed");
    let _ = (lead_id, emp_id);

    app.cleanup().await;
}

/// A finished month whose covered people produced no rows at all must not be
/// mailed as a document of bare headings — and must not be retried every night
/// forever either, because the data cannot appear later: the readiness gate has
/// already declared everyone final.
///
/// Reaching that state takes a person who is *final* yet contributes nothing.
/// An employee on zero weekly hours is exactly that: exempt from the week
/// submission gate, so their empty month is complete rather than outstanding.
/// With only the assistants' table switched on and no assistant in the
/// installation, the assembled document ends up with no rows at all.
#[tokio::test]
async fn scheduled_run_settles_a_month_with_nothing_to_report() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-nothing").await;

    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "zero-payroll-nothing@example.com",
                "first_name": "Zoe",
                "last_name": "Zero",
                "role": "employee",
                "weekly_hours": 0,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create zero-hours employee: {body}");

    // Everyone with a submission obligation is excluded, so the only person
    // the month covers is the one who is final by definition.
    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 1,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
                "payroll_report_excluded_user_ids": [lead_id, emp_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    app.state
        .db
        .payroll_queue
        .enqueue(&previous)
        .await
        .expect("enqueue previous");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &previous,
        )
        .await
        .expect("record queue period");

    // Guard the premise: the month must genuinely cover somebody, or this
    // would only be re-testing the older "covers nobody" path.
    let (from, to) = zerf::background::schedule::period_bounds(&previous).expect("bounds");
    let covered = payroll_report::payroll_members(&app.state, from, to, &[lead_id, emp_id], false)
        .await
        .expect("members");
    assert!(
        !covered.is_empty(),
        "the month has to cover the zero-hours employee for this test to mean anything"
    );

    zerf::background::payroll_report::run_once(&app.state)
        .await
        .expect("scheduled run");

    let queued = app
        .state
        .db
        .payroll_queue
        .list_pending()
        .await
        .expect("queue");
    assert!(
        !queued.contains(&previous),
        "a month with nothing to report is settled, not retried forever: {queued:?}"
    );

    app.cleanup().await;
}

/// Two sick notes filed back to back are one illness, and the certificate
/// verdict is computed over that whole period. Printed as separate rows the
/// verdict reads as a contradiction — the exact report a user queried: a
/// two-day row marked "certificate required" under a four-day threshold,
/// because the second note ran on for another eight days.
///
/// The row therefore has to be the illness period, not the filing.
#[tokio::test]
async fn payroll_report_merges_back_to_back_sick_notes_into_one_row() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-chain").await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let sick = absence_cat(&app.state.pool, "sick").await;

    // Mon-Tue, then Wed-Thu: adjacent, so one continuous illness period.
    app.state
        .db
        .absences
        .create(emp_id, sick.id, true, monday, monday + Duration::days(1), None, "approved")
        .await
        .expect("first sick note");
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            monday + Duration::days(2),
            monday + Duration::days(3),
            None,
            "approved",
        )
        .await
        .expect("second sick note");

    let members = payroll_report::payroll_members(&app.state, from, to, &[], false)
        .await
        .expect("members");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
        },
        &members,
        &config(false, false),
        &zerf::i18n::Language::default(),
        None,
    )
    .await
    .expect("report data");

    let rows: Vec<_> = data
        .absence_rows
        .expect("absence rows")
        .into_iter()
        .filter(|row| row.employee.contains("payroll-chain"))
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "back-to-back sick notes are one illness, so one row: {:?}",
        rows.iter().map(|r| (r.from, r.to, r.days)).collect::<Vec<_>>()
    );
    assert_eq!(rows[0].from, monday, "the row starts where the illness did");
    assert_eq!(
        rows[0].to,
        monday + Duration::days(3),
        "and ends where it did"
    );
    assert_eq!(rows[0].days, 4.0, "days are the whole period, not one filing");
    assert_eq!(
        rows[0].medical_certificate_required,
        Some(true),
        "4 continuous days reaches the default threshold"
    );

    // A separate illness later the same month stays its own row and, being
    // short, keeps its own verdict — merging must not swallow unrelated
    // absences just because they share a person and a category.
    let later = monday + Duration::days(14);
    app.state
        .db
        .absences
        .create(emp_id, sick.id, true, later, later, None, "approved")
        .await
        .expect("unrelated later sick note");

    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
        },
        &members,
        &config(false, false),
        &zerf::i18n::Language::default(),
        None,
    )
    .await
    .expect("report data");
    let rows: Vec<_> = data
        .absence_rows
        .expect("absence rows")
        .into_iter()
        .filter(|row| row.employee.contains("payroll-chain"))
        .collect();
    assert_eq!(rows.len(), 2, "a separate illness is a separate row");
    assert_eq!(rows[1].days, 1.0);
    assert_eq!(
        rows[1].medical_certificate_required,
        Some(false),
        "one day on its own stays below the threshold"
    );

    app.cleanup().await;
}


/// A month held back because somebody has not finished it is not an error, so
/// the nightly run stays quiet about it — but the send day has passed by then,
/// and nobody would learn that the tax office is still waiting. The
/// administrators are told once, and only once, however many nights the month
/// stays open.
#[tokio::test]
async fn a_held_back_scheduled_report_tells_the_admins_once() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-hold").await;

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
    let previous = zerf::background::schedule::previous_period(today);
    app.state
        .db
        .payroll_queue
        .enqueue(&previous)
        .await
        .expect("enqueue previous");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &previous,
        )
        .await
        .expect("record queue period");

    // An undecided absence request in the reported month: sick days belong in
    // the document but only once they are approved, so this genuinely holds the
    // report back — unlike an unhanded-in week, which proves nothing.
    let (from, _to) = zerf::background::schedule::period_bounds(&previous).expect("bounds");
    let sick = absence_cat(&app.state.pool, "sick").await;
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            from + Duration::days(7),
            from + Duration::days(8),
            None,
            "requested",
        )
        .await
        .expect("pending sick note");

    let (status, _) = admin.delete("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK);

    // The undecided request holds the month up, so the period stays queued.
    zerf::background::payroll_report::run_once(&app.state)
        .await
        .expect("scheduled run");

    let queued = app
        .state
        .db
        .payroll_queue
        .list_pending()
        .await
        .expect("queue");
    assert!(
        queued.contains(&previous),
        "an unfinished month stays queued: {queued:?}"
    );

    let (status, body) = admin.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK);
    let notices: Vec<_> = body
        .as_array()
        .expect("notifications array")
        .iter()
        .filter(|item| item["kind"] == "payroll_report_blocked")
        .collect();
    assert_eq!(
        notices.len(),
        1,
        "the admin has to learn that the report is on hold: {body}"
    );

    // Every following night reaches the same period again; the warning must
    // not be repeated.
    zerf::background::payroll_report::run_once(&app.state)
        .await
        .expect("second scheduled run");
    let (status, body) = admin.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.as_array()
            .expect("notifications array")
            .iter()
            .filter(|item| item["kind"] == "payroll_report_blocked")
            .count(),
        1,
        "a nightly retry must not re-warn about the same month"
    );

    app.cleanup().await;
}


/// The payroll card is assembled by the very code that builds the document, so
/// what it lists is what the report prints: an employee's sick note and an
/// assistant's working hours, and nothing else.
#[tokio::test]
async fn the_payroll_card_lists_what_the_report_will_print() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-card").await;
    let lead = login_change_pw(&app, "lead-payroll-card@example.com", &lead_pw).await;
    let (_assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "payroll-card").await;
    let assistant = login_change_pw(
        &app,
        "aushilfe-payroll-card@example.com",
        &assistant_pw,
    )
    .await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 5,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    let (from, _to) = zerf::background::schedule::period_bounds(&previous).expect("bounds");

    // The assistant works a day in the reported month, and it gets approved.
    let day = (from + Duration::days(9)).format("%Y-%m-%d").to_string();
    let entry_id = create_and_submit_entry(&assistant, &day, cat_id).await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the assistant's day");

    // The employee is off sick, decided.
    let sick = absence_cat(&app.state.pool, "sick").await;
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            // Tuesday and Wednesday: a weekend sick note would carry no
            // workdays and say nothing.
            from + Duration::days(10),
            from + Duration::days(11),
            None,
            "approved",
        )
        .await
        .expect("approved sick note");

    let (status, card) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["enabled"], json!(true));
    assert_eq!(card["period"], json!(previous));
    assert_eq!(card["absence_count"], json!(1), "the sick note: {card}");
    assert_eq!(
        card["people_with_hours"],
        json!(1),
        "only the assistant's hours are printed: {card}"
    );
    assert!(
        card["minutes"].as_i64().unwrap_or(0) > 0,
        "the approved day carries minutes: {card}"
    );

    let rows = card["rows"].as_array().expect("rows");
    assert!(
        rows.iter()
            .any(|row| row["kind"] == "absence" && row["name"].is_string()),
        "the absence row names the employee: {card}"
    );
    assert!(
        rows.iter()
            .any(|row| row["kind"] == "hours" && row["minutes"].as_i64().unwrap_or(0) > 0),
        "the hours row carries the assistant's minutes: {card}"
    );

    app.cleanup().await;
}


/// The rule the payroll report now lives by: a week nobody handed in proves
/// nothing and holds nothing back, while a booking that exists and is not
/// approved yet is proof that hours are missing from the document.
#[tokio::test]
async fn only_provable_gaps_hold_the_payroll_report_back() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-gate").await;
    let lead = login_change_pw(&app, "lead-payroll-gate@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "payroll-gate").await;
    let assistant =
        login_change_pw(&app, "aushilfe-payroll-gate@example.com", &assistant_pw).await;

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&previous).expect("bounds");

    let pool = app.state.pool.clone();
    let users = app.state.db.users.clone();
    let readiness_of = move |user_id: i64, require_full_approval: bool| {
        let pool = pool.clone();
        let users = users.clone();
        async move {
            let user = users
                .find_by_id(user_id)
                .await
                .expect("load user")
                .expect("user exists");
            zerf::services::reports::month_export_readiness(
                &pool,
                &user,
                from,
                to,
                require_full_approval,
                false,
                zerf::services::reports::PendingAbsences::PayrollRelevant,
            )
            .await
            .expect("readiness")
        }
    };

    // The employee booked nothing at all in the reported month. Their hours are
    // not printed anyway, so nothing about the document is missing.
    assert!(
        readiness_of(emp_id, false).await.is_ready(),
        "an unhanded-in month must not hold the report back"
    );

    // The assistant worked and handed the day in; nobody has decided it yet.
    let day = (from + Duration::days(9)).format("%Y-%m-%d").to_string();
    let entry_id = create_and_submit_entry(&assistant, &day, cat_id).await;
    assert!(
        !readiness_of(assistant_id, true).await.is_ready(),
        "hours that exist but are not approved are provably missing from the report"
    );

    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the assistant's day");
    assert!(
        readiness_of(assistant_id, true).await.is_ready(),
        "once decided, the month is final for them"
    );

    // An undecided holiday request never reaches the document, so it cannot
    // change it — and must not delay it either.
    let vacation = absence_cat(&app.state.pool, "vacation").await;
    app.state
        .db
        .absences
        .create(
            emp_id,
            vacation.id,
            true,
            from + Duration::days(10),
            from + Duration::days(11),
            None,
            "requested",
        )
        .await
        .expect("pending holiday request");
    assert!(
        readiness_of(emp_id, false).await.is_ready(),
        "an undecided holiday request is not in the report and must not hold it"
    );

    // An undecided sick note is: those days are printed, and only once decided.
    let sick = absence_cat(&app.state.pool, "sick").await;
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            from + Duration::days(17),
            from + Duration::days(18),
            None,
            "requested",
        )
        .await
        .expect("pending sick note");
    assert!(
        !readiness_of(emp_id, false).await.is_ready(),
        "an undecided sick note changes the document and has to be waited for"
    );

    // The same sick note from an assistant does not: they are paid by the
    // hour, so their absences are none of payroll's business.
    app.state
        .db
        .absences
        .create(
            assistant_id,
            sick.id,
            true,
            from + Duration::days(17),
            from + Duration::days(18),
            None,
            "requested",
        )
        .await
        .expect("assistant sick note");
    assert!(
        readiness_of(assistant_id, true).await.is_ready(),
        "an assistant's absence is not in the report and must not hold it"
    );

    app.cleanup().await;
}

/// A month is judged on its own days. The week carrying December's last day
/// runs into January, and what is booked there belongs to January's month —
/// handing in the December part settles December, whatever follows it.
#[tokio::test]
async fn the_month_card_is_settled_by_the_months_own_days() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "month-days").await;
    let lead = login_change_pw(&app, "lead-month-days@example.com", &lead_pw).await;

    // Starting on the month's last day leaves exactly one week to judge: the
    // one that reaches into the new month.
    let (status, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "boundary-month-days@example.com",
                "first_name": "Bo",
                "last_name": "Boundary",
                "role": "employee",
                "weekly_hours": 39,
                "start_date": "2029-12-31",
                "approver_ids": [lead_id],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create boundary employee: {body}");
    let boundary_id = id(&body);
    let boundary = login_change_pw(
        &app,
        "boundary-month-days@example.com",
        &temp_pw(&body),
    )
    .await;

    let december_status = || async {
        let (status, card) = admin.get("/api/v1/reports/submission-status").await;
        assert_eq!(status, StatusCode::OK);
        card["members"]
            .as_array()
            .expect("members")
            .iter()
            .find(|member| member["user_id"].as_i64() == Some(boundary_id))
            .map(|member| member["status"].as_str().unwrap_or_default().to_string())
            .unwrap_or_else(|| panic!("boundary employee missing from the card: {card}"))
    };

    assert_eq!(
        december_status().await,
        "not_submitted",
        "the month's last day has not been handed in yet"
    );

    // Hand in exactly that day.
    let entry_id = create_and_submit_entry(&boundary, "2029-12-31", cat_id).await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve it");
    assert_eq!(
        december_status().await,
        "ready",
        "December is settled by December's own day"
    );

    // A draft in the new month sits in the very same calendar week. It is
    // January's business and must not reopen December.
    let (status, _) = boundary
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": "2030-01-02",
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "book a day in the new month");
    assert_eq!(
        december_status().await,
        "ready",
        "what is booked in January must not make December look unfinished"
    );

    app.cleanup().await;
}
