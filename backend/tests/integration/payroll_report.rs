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
            carried: None,
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
            carried: None,
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
            zerf::services::reports::UnapprovedEntries::NotRequired,
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

    let covered = payroll_report::payroll_members(&app.state, from, to, &[], false, None)
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
        payroll_report::payroll_members(&app.state, from, to, &[emp_id, assistant_id], false, None)
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
    let snapshot = payroll_report::payroll_members(&app.state, from, to, &[], true, None)
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
    assert_eq!(
        body["sent"],
        json!(0),
        "an empty month sends nothing: {body}"
    );

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
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "send-now is refused: {body}"
    );

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
        carried: None,
    };
    let members = payroll_report::payroll_members(&app.state, from, today, &[], true, None)
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
    let covered =
        payroll_report::payroll_members(&app.state, from, to, &[lead_id, emp_id], false, None)
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
        .create(
            emp_id,
            sick.id,
            true,
            monday,
            monday + Duration::days(1),
            None,
            "approved",
        )
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

    let members = payroll_report::payroll_members(&app.state, from, to, &[], false, None)
        .await
        .expect("members");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
            carried: None,
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
        rows.iter()
            .map(|r| (r.from, r.to, r.days))
            .collect::<Vec<_>>()
    );
    assert_eq!(rows[0].from, monday, "the row starts where the illness did");
    assert_eq!(
        rows[0].to,
        monday + Duration::days(3),
        "and ends where it did"
    );
    assert_eq!(
        rows[0].days, 4.0,
        "days are the whole period, not one filing"
    );
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
            carried: None,
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
    let assistant = login_change_pw(&app, "aushilfe-payroll-card@example.com", &assistant_pw).await;

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
    let assistant = login_change_pw(&app, "aushilfe-payroll-gate@example.com", &assistant_pw).await;

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let previous = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&previous).expect("bounds");

    let pool = app.state.pool.clone();
    let users = app.state.db.users.clone();
    let readiness_of =
        move |user_id: i64, unapproved: zerf::services::reports::UnapprovedEntries| {
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
                    unapproved,
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
        readiness_of(
            emp_id,
            zerf::services::reports::UnapprovedEntries::NotRequired
        )
        .await
        .is_ready(),
        "an unhanded-in month must not hold the report back"
    );

    // The assistant worked and handed the day in; nobody has decided it yet.
    let day = (from + Duration::days(9)).format("%Y-%m-%d").to_string();
    let entry_id = create_and_submit_entry(&assistant, &day, cat_id).await;
    assert!(
        !readiness_of(
            assistant_id,
            zerf::services::reports::UnapprovedEntries::AnyUnsettled
        )
        .await
        .is_ready(),
        "a booking that exists and is not approved is proof of hours the report would miss"
    );

    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the assistant's day");
    assert!(
        readiness_of(
            assistant_id,
            zerf::services::reports::UnapprovedEntries::AnyUnsettled
        )
        .await
        .is_ready(),
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
        readiness_of(
            emp_id,
            zerf::services::reports::UnapprovedEntries::NotRequired
        )
        .await
        .is_ready(),
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
        !readiness_of(
            emp_id,
            zerf::services::reports::UnapprovedEntries::NotRequired
        )
        .await
        .is_ready(),
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
        readiness_of(
            assistant_id,
            zerf::services::reports::UnapprovedEntries::AnyUnsettled
        )
        .await
        .is_ready(),
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
    let boundary = login_change_pw(&app, "boundary-month-days@example.com", &temp_pw(&body)).await;

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

/// Hours an assistant books only after their month has already been reported
/// have to reach the payroll accountant with the day they were worked on — the
/// month they belong to is closed, and nothing else would ever pay them.
#[tokio::test]
async fn hours_booked_after_a_month_was_reported_reach_the_next_report() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-late").await;
    let lead = login_change_pw(&app, "lead-payroll-late@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "late").await;
    let assistant = login_change_pw(&app, "aushilfe-late@example.com", &assistant_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let period = from.format("%Y-%m").to_string();

    // A day the month's own report carried.
    let on_time =
        create_and_submit_entry(&assistant, &monday.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[on_time]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the on-time day");

    // That report goes out: everything it accounted for is recorded as sent,
    // and the period leaves the queue.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(&period, from, to, Default::default())
        .await
        .expect("mark the reported month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    // Only afterwards does the assistant remember a second day of that month.
    let late_day = monday + Duration::days(1);
    let late =
        create_and_submit_entry(&assistant, &late_day.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({"ids":[late]}))
        .await;
    assert_eq!(status, StatusCode::OK, "approve the late day");

    // And they have left since: an assistant who is gone is exactly who forgets
    // a shift, and the month now being reported knows nothing about them.
    let (status, body) = admin
        .post(&format!("/api/v1/users/{assistant_id}/archive"), &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "archive the assistant: {body}");

    let (next_from, next_to) = month_bounds(to + Duration::days(1));
    let next_period = next_from.format("%Y-%m").to_string();
    let carried = payroll_report::carry_over_boundary(&app.state.pool, next_from)
        .await
        .expect("carry-over boundary");
    assert_eq!(
        carried.as_ref().map(|c| c.before),
        Some(next_from),
        "the reported month is closed, so any of its days may still be carried"
    );

    let members = payroll_report::payroll_members(
        &app.state,
        next_from,
        next_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");
    assert!(
        members.iter().any(|member| member.id == assistant_id),
        "an assistant who has left still has to be paid for the day they booked"
    );

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: carried.clone(),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the next month's report");

    assert_eq!(
        data.late_entry_rows.len(),
        1,
        "only the day booked after the report counts as a catch-up"
    );
    assert_eq!(
        data.late_entry_rows[0].date, late_day,
        "the day that was worked, not the month that reports it"
    );
    assert_eq!(data.late_entry_rows[0].minutes, 240);
    assert!(
        data.declared_work_days.is_empty(),
        "a pre-ledger period has no reconstructible zero-day baseline"
    );
    assert_eq!(
        data.carried_work_days
            .iter()
            .map(|day| (day.user_id, day.date))
            .collect::<Vec<_>>(),
        vec![(assistant_id, late_day)],
        "the legacy row still travels to the exact marker step"
    );
    assert!(
        data.hours_sections
            .iter()
            .all(|section| section.rows.is_empty()),
        "nobody worked in the reporting month itself"
    );

    // This report goes out in turn and records what it carried.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(
            &next_period,
            next_from,
            next_to,
            zerf::repository::PayrollCarryScope {
                since: carried.as_ref().map(|c| c.since),
                before: carried.as_ref().map(|c| c.before),
                owed_periods: carried
                    .as_ref()
                    .map(|c| c.owed_periods.as_slice())
                    .unwrap_or(&[]),
                days: &[(assistant_id, late_day)],
            },
        )
        .await
        .expect("mark the carried day");
    let again = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: carried.clone(),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("rebuild after sending");
    assert!(
        again.late_entry_rows.is_empty(),
        "a day already carried must never be paid a second time"
    );

    app.cleanup().await;
}

/// A month whose own report is still outstanding must not have its days pulled
/// into a later one: that report is still coming, and the hours would be
/// reported twice.
#[tokio::test]
async fn a_month_still_awaiting_its_own_report_is_not_carried_into_the_next() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-wait").await;
    let lead = login_change_pw(&app, "lead-payroll-wait@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "wait").await;
    let assistant = login_change_pw(&app, "aushilfe-wait@example.com", &assistant_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let period = from.format("%Y-%m").to_string();

    let entry =
        create_and_submit_entry(&assistant, &monday.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[entry]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the day");

    // The month reached the queue but its report has not gone out yet.
    app.state
        .db
        .payroll_queue
        .enqueue(&period)
        .await
        .expect("queue the period");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    let (next_from, next_to) = month_bounds(to + Duration::days(1));
    let carried = payroll_report::carry_over_boundary(&app.state.pool, next_from)
        .await
        .expect("carry-over boundary");
    // The owed month is skipped by name, not by lowering the upper bound —
    // see `CarriedDays::owed_periods`.
    assert_eq!(
        carried.as_ref().map(|c| c.before),
        Some(next_from),
        "the bound is the reported month's own start"
    );

    let members = payroll_report::payroll_members(
        &app.state,
        next_from,
        next_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");
    assert!(
        !members.iter().any(|member| member.id == assistant_id),
        "an unreported day of a month still owed says nothing about the next one"
    );

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: carried.clone(),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the next month's report");
    assert!(
        data.late_entry_rows.is_empty(),
        "the day belongs to the report that is still to come"
    );

    app.cleanup().await;
}

/// A day belonging to somebody whose hours the report does not print must not
/// be recorded as reported. No report ever contained it, and marking it would
/// destroy the only evidence of that: were *List employees' working hours*
/// switched on later, the day could never be caught up.
#[tokio::test]
async fn a_day_no_report_printed_is_not_recorded_as_reported() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-mark").await;
    let lead = login_change_pw(&app, "lead-payroll-mark@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-payroll-mark@example.com", &emp_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let period = from.format("%Y-%m").to_string();

    // A day the employee booked while their month was still open.
    let on_time =
        create_and_submit_entry(&employee, &monday.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[on_time]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the on-time day");

    // The report for that month goes out. Employees' hours are not printed, so
    // nobody's older day was carried — but the month itself has been reported.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(&period, from, to, Default::default())
        .await
        .expect("mark the reported month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    // Only afterwards does the employee book a second day of that month.
    let late_day = monday + Duration::days(1);
    let late =
        create_and_submit_entry(&employee, &late_day.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({"ids":[late]}))
        .await;
    assert_eq!(status, StatusCode::OK, "approve the late day");

    let (next_from, next_to) = month_bounds(to + Duration::days(1));
    let next_period = next_from.format("%Y-%m").to_string();
    let carried = payroll_report::carry_over_boundary(&app.state.pool, next_from)
        .await
        .expect("carry-over boundary");

    // With employee hours switched on, the day is a catch-up like any other.
    let language = zerf::i18n::Language::from_setting("en");
    let members = payroll_report::payroll_members(
        &app.state,
        next_from,
        next_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");
    let window = payroll_report::ReportWindow {
        from: next_from,
        to: next_to,
        interim: false,
        created_on: next_to,
        carried: carried.clone(),
    };
    let with_employee_hours = payroll_report::build_report_data(
        &app.state,
        window.clone(),
        &members,
        &config(true, true),
        &language,
        None,
    )
    .await
    .expect("build with employee hours");
    assert_eq!(
        with_employee_hours
            .late_entry_rows
            .iter()
            .map(|row| row.date)
            .collect::<Vec<_>>(),
        vec![late_day],
        "an employee's late day is carried once their hours are printed"
    );

    // The month is actually sent with employee hours off, so the document
    // printed nobody's hours and carried nobody's day.
    let sent_without_employee_hours = payroll_report::build_report_data(
        &app.state,
        window,
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build without employee hours");
    assert!(
        sent_without_employee_hours.late_entry_rows.is_empty(),
        "a report that does not print employee hours carries no employee day"
    );
    app.state
        .db
        .time_entries
        .mark_payroll_reported(
            &next_period,
            next_from,
            next_to,
            zerf::repository::PayrollCarryScope {
                since: carried.as_ref().map(|c| c.since),
                before: carried.as_ref().map(|c| c.before),
                owed_periods: carried
                    .as_ref()
                    .map(|c| c.owed_periods.as_slice())
                    .unwrap_or(&[]),
                days: &[],
            },
        )
        .await
        .expect("mark what that report accounted for");

    // The decisive assertion: the day is still catchable. Marking it here would
    // have lost it for good.
    let still_outstanding = app
        .state
        .db
        .reports
        .carried_time_entries_before(
            None,
            NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
            next_from,
            &[],
        )
        .await
        .expect("outstanding days");
    assert!(
        still_outstanding
            .iter()
            .any(|(user_id, date, _, _)| *user_id == emp_id && *date == late_day),
        "a day no report printed must stay outstanding, whoever it belongs to"
    );
    assert!(
        !still_outstanding
            .iter()
            .any(|(_, date, _, _)| *date == monday),
        "the day that was there when its month was reported is settled"
    );

    app.cleanup().await;
}

/// The card for a month that has already gone out has to show what that report
/// contained. A day booked after the send belongs to the *next* report, and
/// listing it here would tell the reader the tax office already has it.
#[tokio::test]
async fn a_sent_month_shows_the_days_it_carried_and_not_the_ones_since() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "payroll-history").await;
    let lead = login_change_pw(&app, "lead-payroll-history@example.com", &lead_pw).await;
    let (_assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "history").await;
    let assistant = login_change_pw(&app, "aushilfe-history@example.com", &assistant_pw).await;

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

    // The month the card reports on, and the one before it.
    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let period = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&period).expect("bounds");
    let (earlier_from, _earlier_to) = month_bounds(from - Duration::days(1));

    // A day from the earlier month that this month's report carried.
    let carried_day = earlier_from + Duration::days(9);
    let carried_id = create_and_submit_entry(
        &assistant,
        &carried_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[carried_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the carried day");

    // The report goes out: it prints the assistant's hours, so their carried
    // day is recorded as having gone out with this period.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(
            &period,
            from,
            to,
            zerf::repository::PayrollCarryScope {
                since: Some(earlier_from),
                before: Some(from),
                owed_periods: &[],
                days: &[(_assistant_id, carried_day)],
            },
        )
        .await
        .expect("mark the sent report");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    // Afterwards the assistant remembers one more day of that earlier month.
    let since_day = earlier_from + Duration::days(10);
    let since_id = create_and_submit_entry(
        &assistant,
        &since_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[since_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the day booked since");

    let (status, body) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {body}");
    assert_eq!(body["sent"], true, "the month has been delivered");
    let carried_dates: Vec<String> = body["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["kind"] == "late_hours")
        .map(|row| row["from"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        carried_dates,
        vec![carried_day.format("%Y-%m-%d").to_string()],
        "the sent report's own carried day, and not the one booked since"
    );

    app.cleanup().await;
}

/// A delivered month's card must show what its own report actually printed,
/// not the live state of the entries: a new entry approved afterwards for a
/// date inside that month is not in the mailed PDF and belongs to a future
/// report, so it must not inflate this month's hours or appear twice across
/// the two dashboard cards.
#[tokio::test]
async fn a_sent_months_hours_do_not_grow_after_the_fact() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "sent-stable").await;
    let lead = login_change_pw(&app, "lead-sent-stable@example.com", &lead_pw).await;
    let (_assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "sent-stable").await;
    let assistant = login_change_pw(&app, "aushilfe-sent-stable@example.com", &assistant_pw).await;

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
    assert_eq!(status, StatusCode::OK);

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let period = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&period).expect("bounds");

    // One approved day, present when the month is (simulated as) sent.
    let day1 = from + Duration::days(2);
    let id1 =
        create_and_submit_entry(&assistant, &day1.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({"ids":[id1]}))
        .await;
    assert_eq!(status, StatusCode::OK, "approve the original day");

    app.state
        .db
        .time_entries
        .mark_payroll_reported(&period, from, to, Default::default())
        .await
        .expect("mark what the send accounted for");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    let (status, body) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {body}");
    assert_eq!(body["sent"], true, "the month has been delivered");
    assert_eq!(
        body["minutes"], 240,
        "exactly the one day that was there at send time"
    );

    // A second day, approved only after the send — for a date inside the
    // SAME already-reported month.
    let day2 = from + Duration::days(3);
    let id2 =
        create_and_submit_entry(&assistant, &day2.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({"ids":[id2]}))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "approve the day booked after the send"
    );

    let (status, body) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "payroll content after the late approval: {body}"
    );
    assert_eq!(
        body["minutes"], 240,
        "a day approved after the send must not inflate what the sent report is shown to contain"
    );
    let hours_rows: Vec<&serde_json::Value> = body["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["kind"] == "hours")
        .collect();
    assert_eq!(
        hours_rows.len(),
        1,
        "one hours row for the one day the report actually sent"
    );

    app.cleanup().await;
}

/// The zero-row rule for a finished month must survive the switch to reading
/// a sent month back from its mark: an employee who did nothing in an
/// already-delivered month is still a real, printed "0 days" line once
/// employee hours are switched on, exactly as it would have been at send
/// time — the marker-based read must not silently drop people who have no
/// entry to key off of.
#[tokio::test]
async fn a_sent_months_employee_zero_row_survives_the_marker_based_read() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "sent-zero").await;
    let lead = login_change_pw(&app, "lead-sent-zero@example.com", &lead_pw).await;
    let (_assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "sent-zero").await;
    let assistant = login_change_pw(&app, "aushilfe-sent-zero@example.com", &assistant_pw).await;

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 5,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": true,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let period = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&period).expect("bounds");

    // The assistant books something so the month covers somebody; the
    // employee books nothing at all.
    let day1 = from + Duration::days(2);
    let id1 =
        create_and_submit_entry(&assistant, &day1.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({"ids":[id1]}))
        .await;
    assert_eq!(status, StatusCode::OK);

    app.state
        .db
        .time_entries
        .mark_payroll_reported(&period, from, to, Default::default())
        .await
        .expect("mark the reported month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    let (status, body) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {body}");
    assert_eq!(body["sent"], true);
    let hours_rows: Vec<&serde_json::Value> = body["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["kind"] == "hours")
        .collect();
    // The assistant's worked day, plus a zero row each for the employee and
    // the team lead — both non-assistants, both booked nothing.
    assert_eq!(
        hours_rows.len(),
        3,
        "one worked row and two zero rows: {hours_rows:?}"
    );
    let zero_rows = hours_rows
        .iter()
        .filter(|row| row["days"].as_f64() == Some(0.0) && row["minutes"].as_i64() == Some(0))
        .count();
    assert_eq!(
        zero_rows, 2,
        "the employee and the lead, both with no bookings, must still print as 0-day rows: {hours_rows:?}"
    );

    app.cleanup().await;
}

/// An installation that has been tracking time for a while and only *then*
/// switches the payroll report on must not have its entire back-catalogue
/// swept into the very first report as catch-up days.
///
/// The first scheduled run is the moment `payroll_report_queue_period` stops
/// being empty, so by the time anything asks "has a report ever been
/// delivered?" the answer is already yes. Without a floor recorded at that
/// same moment, the first report would treat every approved entry older than
/// its own month — however old — as a day that missed its report.
#[tokio::test]
async fn the_first_report_ever_sent_does_not_sweep_up_the_back_catalogue() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "first-run").await;
    let lead = login_change_pw(&app, "lead-first-run@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "first-run").await;
    let assistant = login_change_pw(&app, "aushilfe-first-run@example.com", &assistant_pw).await;

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let period = zerf::background::schedule::previous_period(today);
    let (from, _to) = zerf::background::schedule::period_bounds(&period).expect("bounds");
    // Two months before the first reported month: history from before payroll
    // reporting was ever enabled.
    let (older_from, _older_to) = month_bounds(from - Duration::days(1));
    let (oldest_from, _oldest_to) = month_bounds(older_from - Duration::days(1));

    // Approved work in that back-catalogue. Nothing has ever marked it,
    // because the payroll report has never run.
    let ancient_day = oldest_from + Duration::days(9);
    let ancient = create_and_submit_entry(
        &assistant,
        &ancient_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[ancient]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the historical day");

    // Sanity: nothing has ever been queued, so nothing can be a late booking.
    assert_eq!(
        zerf::services::settings::load_setting(
            &app.state.pool,
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            "",
        )
        .await
        .expect("queue marker"),
        "",
        "this installation has never queued a payroll period"
    );

    // Now the admin switches the payroll report on and the first scheduled run
    // happens — queueing the previous month and recording the floor.
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
    assert_eq!(status, StatusCode::OK, "enable the payroll report");
    zerf::background::payroll_report::run_once(&app.state)
        .await
        .expect("first scheduled run");

    // The floor is now the first period this installation ever queued.
    let floor = zerf::services::settings::load_setting(
        &app.state.pool,
        zerf::services::settings::PAYROLL_REPORT_FIRST_PERIOD_KEY,
        "",
    )
    .await
    .expect("floor");
    assert_eq!(floor, period, "the first queued period becomes the floor");

    // The decisive assertion: a report built now carries nothing from before
    // the floor, however much approved history is sitting there.
    let carried = payroll_report::carry_over_boundary(&app.state.pool, from)
        .await
        .expect("carry-over boundary")
        .expect("a period has been queued, so carrying is possible in principle");
    assert_eq!(
        carried.since, from,
        "nothing before the first ever reported month may be carried"
    );

    let members = payroll_report::payroll_members(
        &app.state,
        from,
        from + Duration::days(27),
        &[],
        false,
        Some(&carried),
    )
    .await
    .expect("members");
    assert!(
        !members.iter().any(|member| member.id == assistant_id),
        "a pre-history day must not drag its owner into the first report either"
    );

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to: from + Duration::days(27),
            interim: false,
            created_on: from + Duration::days(27),
            carried: Some(carried),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the first report");
    assert!(
        data.late_entry_rows.is_empty(),
        "the first report ever sent carries no pre-history: {:?}",
        data.late_entry_rows
            .iter()
            .map(|row| row.date)
            .collect::<Vec<_>>()
    );

    app.cleanup().await;
}

/// One month stuck behind a late approval must not freeze carry-over for the
/// months after it that were already delivered.
///
/// The queue can have gaps: March held up while April went out. A day booked
/// late in April is owed to whoever comes next, and holding it back until
/// March finally clears — which may be never — would leave those hours unpaid
/// for a reason that has nothing to do with them.
#[tokio::test]
async fn a_stuck_month_does_not_freeze_carry_over_for_the_months_after_it() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "stuck-gap").await;
    let lead = login_change_pw(&app, "lead-stuck-gap@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "stuck-gap").await;
    let assistant = login_change_pw(&app, "aushilfe-stuck-gap@example.com", &assistant_pw).await;

    // Three consecutive months: `stuck` is never delivered, `delivered` is,
    // and `reporting` is the one being assembled now.
    let reporting_monday = anchor_monday();
    let (reporting_from, _reporting_to) = month_bounds(reporting_monday);
    let (delivered_from, delivered_to) = month_bounds(reporting_from - Duration::days(1));
    let (stuck_from, _stuck_to) = month_bounds(delivered_from - Duration::days(1));
    let stuck_period = stuck_from.format("%Y-%m").to_string();
    let delivered_period = delivered_from.format("%Y-%m").to_string();

    // A day in the stuck month, whose own report is still owed.
    let stuck_day = stuck_from + Duration::days(9);
    let stuck_entry = create_and_submit_entry(
        &assistant,
        &stuck_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[stuck_entry]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the stuck month's day");

    // The delivered month goes out first, recording everything that existed
    // in it at that moment.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(
            &delivered_period,
            delivered_from,
            delivered_to,
            Default::default(),
        )
        .await
        .expect("mark the delivered month");

    // Only afterwards is a day of that month booked and approved — a genuine
    // catch-up, owed to whichever report comes next.
    let late_day = delivered_from + Duration::days(9);
    let late =
        create_and_submit_entry(&assistant, &late_day.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({"ids":[late]}))
        .await;
    assert_eq!(status, StatusCode::OK, "approve the late day");

    // The stuck month is still queued behind it.
    app.state
        .db
        .payroll_queue
        .enqueue(&stuck_period)
        .await
        .expect("the stuck month is still owed");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &delivered_period,
        )
        .await
        .expect("record the queued period");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_FIRST_PERIOD_KEY,
            &stuck_period,
        )
        .await
        .expect("record the floor");

    let carried = payroll_report::carry_over_boundary(&app.state.pool, reporting_from)
        .await
        .expect("carry-over boundary")
        .expect("a period has been queued");
    let language = zerf::i18n::Language::from_setting("en");
    let members = payroll_report::payroll_members(
        &app.state,
        reporting_from,
        reporting_from + Duration::days(27),
        &[],
        false,
        Some(&carried),
    )
    .await
    .expect("members");
    assert!(
        members.iter().any(|member| member.id == assistant_id),
        "the late day from the delivered month still brings its owner in"
    );

    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: reporting_from,
            to: reporting_from + Duration::days(27),
            interim: false,
            created_on: reporting_from + Duration::days(27),
            carried: Some(carried),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the report");

    let carried_dates: Vec<NaiveDate> = data.late_entry_rows.iter().map(|row| row.date).collect();
    assert_eq!(
        carried_dates,
        vec![late_day],
        "the delivered month's late day is carried; the stuck month's is not"
    );

    app.cleanup().await;
}

/// Moving an already-reported day into a different month must clear its
/// payroll mark. The mark names the month whose report accounted for the
/// entry; after the move that is no longer the month the entry is in, and a
/// stale mark makes the day invisible to both — the old month no longer
/// contains its date, the new month does not carry its mark.
#[tokio::test]
async fn moving_a_reported_day_to_another_month_makes_it_catchable_again() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "moved-day").await;
    let lead = login_change_pw(&app, "lead-moved-day@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "moved-day").await;
    let assistant = login_change_pw(&app, "aushilfe-moved-day@example.com", &assistant_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let period = from.format("%Y-%m").to_string();

    let entry_id =
        create_and_submit_entry(&assistant, &monday.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, _) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the day");

    // The month's report goes out and records the day.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(&period, from, to, Default::default())
        .await
        .expect("mark the reported month");
    let reported_as: Option<String> =
        sqlx::query_scalar("SELECT payroll_reported_period FROM time_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(&app.state.pool)
            .await
            .expect("read the mark");
    assert_eq!(
        reported_as,
        Some(period.clone()),
        "the day went out with its own month's report"
    );

    // An admin corrects the date into the previous month.
    let (earlier_from, _earlier_to) = month_bounds(from - Duration::days(1));
    let corrected_day = earlier_from + Duration::days(9);
    let (status, body) = admin
        .put(
            &format!("/api/v1/time-entries/{entry_id}"),
            &json!({
                "entry_date": corrected_day.format("%Y-%m-%d").to_string(),
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "moved",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "admin corrects the date: {body}");

    let reported_as: Option<String> =
        sqlx::query_scalar("SELECT payroll_reported_period FROM time_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(&app.state.pool)
            .await
            .expect("read the mark again");
    assert_eq!(
        reported_as, None,
        "a day moved out of its reported month is no longer accounted for by it"
    );

    // Which means it is a catch-up candidate again, rather than a day no
    // report will ever show.
    let outstanding = app
        .state
        .db
        .reports
        .carried_time_entries_before(
            None,
            NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
            from,
            &[],
        )
        .await
        .expect("outstanding days");
    assert!(
        outstanding
            .iter()
            .any(|(user_id, date, _, _)| *user_id == assistant_id && *date == corrected_day),
        "the moved day can be carried into a later report"
    );

    app.cleanup().await;
}

/// A delivered month's card reads its hours back from the payroll marker
/// instead of recomputing them live. That read-back is a second
/// implementation of the same arithmetic, so it has to agree with the live
/// one exactly — including the automatic break deduction, which is the part
/// most easily got wrong when a day is rebuilt from raw entries.
///
/// Marking changes only *how* the figures are obtained, never what they are,
/// so the card must not move by a single minute across the send.
#[tokio::test]
async fn a_sent_months_figures_match_what_the_live_path_produced() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "same-figs").await;
    let lead = login_change_pw(&app, "lead-same-figs@example.com", &lead_pw).await;
    let (_assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "same-figs").await;
    let assistant = login_change_pw(&app, "aushilfe-same-figs@example.com", &assistant_pw).await;

    // Automatic breaks on: the deduction is per day and merges adjacent
    // blocks, so it is exactly where a rebuilt-from-entries day can drift.
    for (key, value) in [
        (zerf::services::settings::AUTO_BREAK_ENABLED_KEY, "true"),
        (
            zerf::services::settings::AUTO_BREAK_THRESHOLD_HOURS_KEY,
            "6",
        ),
        (
            zerf::services::settings::AUTO_BREAK_DEDUCTION_MINUTES_KEY,
            "30",
        ),
    ] {
        app.state
            .db
            .settings
            .save_setting(key, value)
            .await
            .expect("configure auto break");
    }

    let (status, _) = admin
        .put(
            "/api/v1/settings/payroll-report",
            &json!({
                "payroll_report_enabled": true,
                "payroll_report_recipients": ["payroll@example.com"],
                "payroll_report_day_of_month": 5,
                "payroll_report_include_assistant_hours": true,
                "payroll_report_include_employee_hours": true,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable payroll report");

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let period = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&period).expect("bounds");

    // A long day that crosses the break threshold, plus a split day whose two
    // blocks together cross it — the case the deduction has to merge.
    let long_day = from + Duration::days(1);
    let split_day = from + Duration::days(2);
    let mut ids = Vec::new();
    for (day, start, end) in [
        (long_day, "08:00", "17:00"),
        (split_day, "08:00", "12:00"),
        (split_day, "12:30", "16:00"),
    ] {
        let (status, body) = assistant
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day.format("%Y-%m-%d").to_string(),
                    "start_time": start,
                    "end_time": end,
                    "category_id": cat_id,
                    "comment": "work",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "create entry: {body}");
        ids.push(id(&body));
    }
    let (status, _) = assistant
        .post("/api/v1/time-entries/submit", &json!({ "ids": ids }))
        .await;
    assert_eq!(status, StatusCode::OK, "submit");
    let (status, _) = lead
        .post("/api/v1/time-entries/batch-approve", &json!({ "ids": ids }))
        .await;
    assert_eq!(status, StatusCode::OK, "approve");

    // The card as the live path builds it, before anything is marked.
    let (status, before_send) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {before_send}");
    assert_eq!(before_send["sent"], false, "not delivered yet");
    assert!(
        before_send["minutes"].as_i64().unwrap_or(0) > 0,
        "the live path found the booked hours: {before_send}"
    );

    // The month goes out and records what it contained.
    app.state
        .db
        .time_entries
        .mark_payroll_reported(&period, from, to, Default::default())
        .await
        .expect("mark the sent month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    let (status, after_send) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {after_send}");
    assert_eq!(after_send["sent"], true, "now delivered");

    assert_eq!(
        after_send["minutes"], before_send["minutes"],
        "the marker read-back must report the same minutes as the live path"
    );
    assert_eq!(
        after_send["people_with_hours"], before_send["people_with_hours"],
        "and the same people"
    );

    let hours_rows = |body: &serde_json::Value| -> Vec<(String, f64, i64)> {
        let mut rows: Vec<(String, f64, i64)> = body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter(|row| row["kind"] == "hours")
            .map(|row| {
                (
                    row["name"].as_str().unwrap_or_default().to_string(),
                    row["days"].as_f64().unwrap_or(-1.0),
                    row["minutes"].as_i64().unwrap_or(-1),
                )
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    };
    assert_eq!(
        hours_rows(&after_send),
        hours_rows(&before_send),
        "every hours row must survive the send unchanged"
    );

    app.cleanup().await;
}

/// A sick note filed for an already-reported month must still reach the tax
/// office.
///
/// This is the absence half of the catch-up path, and it has a sharper cause
/// than the entries half. `AbsenceCategory::is_payroll_relevant` is
/// `auto_approve_past OR unpaid`, so a sick-like absence entered for *past*
/// dates is approved on the spot: it never sits in `requested`, never trips
/// the readiness gate, and so cannot hold its own month's report back. It
/// simply turns up after that month has been filed — and without carry-over
/// those days would be in no document at all, with continued pay never
/// claimed for them.
#[tokio::test]
async fn a_sick_note_filed_after_its_month_was_reported_reaches_the_next_report() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "late-sick").await;
    let _lead = login_change_pw(&app, "lead-late-sick@example.com", &lead_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let period = from.format("%Y-%m").to_string();
    let sick = absence_cat(&app.state.pool, "sick").await;

    // A sick note that was there when the month's report went out.
    app.state
        .db
        .absences
        .create(emp_id, sick.id, true, monday, monday, None, "approved")
        .await
        .expect("create the on-time sick note");

    // The report goes out and records what it showed.
    app.state
        .db
        .reports
        .mark_payroll_reported_absences(&period, from, to, &[emp_id], &[])
        .await
        .expect("mark the reported month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    // Only afterwards is a second sick note filed for that same, closed month.
    // A payroll-relevant category auto-approves a past absence, so this never
    // had a chance to hold the month open.
    let late_sick_from = monday + Duration::days(1);
    let late_sick_to = monday + Duration::days(2);
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            late_sick_from,
            late_sick_to,
            None,
            "approved",
        )
        .await
        .expect("create the late sick note");

    let (next_from, next_to) = month_bounds(to + Duration::days(1));
    let carried = payroll_report::carry_over_boundary(&app.state.pool, next_from)
        .await
        .expect("carry-over boundary");

    let members = payroll_report::payroll_members(
        &app.state,
        next_from,
        next_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: carried.clone(),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the next month's report");

    assert_eq!(
        data.late_absence_rows.len(),
        1,
        "only the sick note filed after the send is a catch-up: {:?}",
        data.late_absence_rows
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>()
    );
    let row = &data.late_absence_rows[0];
    assert_eq!(
        (row.from, row.to),
        (late_sick_from, late_sick_to),
        "the days actually taken, not a clamp to the reporting month"
    );
    assert!(
        row.days > 0.0,
        "the catch-up row carries payroll-relevant days"
    );

    // The decisive part: this report goes out in turn and records what it
    // carried, so the same sick days are never declared a second time. A
    // carried absence ends before the reported month, so the mark has to reach
    // outside that month to find it — otherwise it would be re-reported every
    // month for ever.
    let next_period = next_from.format("%Y-%m").to_string();
    app.state
        .db
        .reports
        .mark_payroll_reported_absences(
            &next_period,
            next_from,
            next_to,
            &[emp_id],
            // Exactly what the document above declared — the send path takes
            // this list straight off the assembled report.
            &data.late_absence_ids,
        )
        .await
        .expect("mark what this report carried");

    let again = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried,
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("rebuild after sending");
    assert!(
        again.late_absence_rows.is_empty(),
        "a sick note already declared must never be declared again: {:?}",
        again
            .late_absence_rows
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>()
    );

    app.cleanup().await;
}

/// A delivered month's card must show the absences that report contained, not
/// the ones filed since. A sick note entered for an already-reported month is
/// approved on the spot and waits for the *next* report; showing it on the
/// month it covers would say the tax office already has it, while it also
/// appears as "Reported later" on a later card — the same days twice.
#[tokio::test]
async fn a_sent_months_absences_do_not_grow_after_the_fact() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    configure_unreachable_smtp(&app).await;
    let (lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "sent-abs").await;
    let lead = login_change_pw(&app, "lead-sent-abs@example.com", &lead_pw).await;
    let _ = lead_id;

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
    let period = zerf::background::schedule::previous_period(today);
    let (from, to) = zerf::background::schedule::period_bounds(&period).expect("bounds");
    let sick = absence_cat(&app.state.pool, "sick").await;

    // A sick note present when the month's report went out.
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            from + Duration::days(8),
            from + Duration::days(9),
            None,
            "approved",
        )
        .await
        .expect("on-time sick note");

    app.state
        .db
        .reports
        .mark_payroll_reported_absences(&period, from, to, &[emp_id], &[])
        .await
        .expect("mark the sent report");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    let (status, before) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {before}");
    assert_eq!(before["sent"], true, "the month has been delivered");
    assert_eq!(
        before["absence_count"], 1,
        "the one sick note the report actually contained"
    );

    // A second sick note entered afterwards, for days inside that same closed
    // month. auto_approve_past means it is approved immediately.
    app.state
        .db
        .absences
        .create(
            emp_id,
            sick.id,
            true,
            from + Duration::days(15),
            from + Duration::days(16),
            None,
            "approved",
        )
        .await
        .expect("late sick note");

    let (status, after) = lead.get("/api/v1/reports/payroll-content").await;
    assert_eq!(status, StatusCode::OK, "payroll content: {after}");
    assert_eq!(
        after["absence_count"], before["absence_count"],
        "a sick note filed after the send must not join the month it was filed for"
    );

    app.cleanup().await;
}

/// A sick note filed for somebody who has since left must still reach the tax
/// office. They are no longer active and have nothing in the month now being
/// reported, so the period's own member query cannot see them — and a last
/// sick note arriving after someone leaves is exactly when this happens.
#[tokio::test]
async fn a_departed_employees_late_sick_note_is_still_reported() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "gone-sick").await;
    let _lead = login_change_pw(&app, "lead-gone-sick@example.com", &lead_pw).await;

    let monday = anchor_monday();
    let (from, to) = month_bounds(monday);
    let period = from.format("%Y-%m").to_string();
    let sick = absence_cat(&app.state.pool, "sick").await;

    // That month's report goes out — the employee had nothing in it.
    app.state
        .db
        .reports
        .mark_payroll_reported_absences(&period, from, to, &[emp_id], &[])
        .await
        .expect("mark the reported month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the queued period");

    // Afterwards a sick note turns up for that closed month — auto-approved,
    // because a payroll-relevant category approves past dates on the spot.
    let sick_from = monday + Duration::days(1);
    let sick_to = monday + Duration::days(2);
    app.state
        .db
        .absences
        .create(emp_id, sick.id, true, sick_from, sick_to, None, "approved")
        .await
        .expect("late sick note");

    // And the employee has left since.
    let (status, body) = admin
        .post(&format!("/api/v1/users/{emp_id}/archive"), &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "archive the employee: {body}");

    let (next_from, next_to) = month_bounds(to + Duration::days(1));
    let carried = payroll_report::carry_over_boundary(&app.state.pool, next_from)
        .await
        .expect("carry-over boundary");
    let members = payroll_report::payroll_members(
        &app.state,
        next_from,
        next_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");
    assert!(
        members.iter().any(|member| member.id == emp_id),
        "somebody who has left is still covered by the report that owes them"
    );
    assert_eq!(
        members.iter().filter(|member| member.id == emp_id).count(),
        1,
        "and appears exactly once, however many kinds of catch-up they hold"
    );

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried,
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the next month's report");
    assert_eq!(
        data.late_absence_rows
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>(),
        vec![(sick_from, sick_to)],
        "their sick days are declared under the dates they actually cover"
    );

    app.cleanup().await;
}

/// A sick note straddling the month boundary, filed after the earlier month
/// was reported, must have its earlier days declared too.
///
/// The ordinary path clamps such an absence to the month being reported and
/// marks it as shown, so the days before that month get exactly one chance to
/// be declared — as a catch-up. Selecting catch-ups on the absence's *end*
/// date drops the whole row and loses them permanently.
#[tokio::test]
async fn a_sick_note_spanning_the_month_boundary_declares_its_earlier_days() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "span-sick").await;
    let _lead = login_change_pw(&app, "lead-span-sick@example.com", &lead_pw).await;

    // The month that has been reported, and the one now being reported.
    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let reporting = zerf::background::schedule::previous_period(today);
    let (reporting_from, reporting_to) =
        zerf::background::schedule::period_bounds(&reporting).expect("bounds");
    let (earlier_from, earlier_to) = month_bounds(reporting_from - Duration::days(1));
    let earlier_period = earlier_from.format("%Y-%m").to_string();
    let sick = absence_cat(&app.state.pool, "sick").await;

    // The earlier month's report goes out with nothing in it for this person.
    app.state
        .db
        .reports
        .mark_payroll_reported_absences(
            &earlier_period,
            earlier_from,
            earlier_to,
            &[emp_id],
            &[],
        )
        .await
        .expect("mark the earlier month");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &earlier_period,
        )
        .await
        .expect("record the queued period");

    // Only afterwards is a sick note filed that straddles the boundary: the
    // last three days of the closed month and the first two of the new one.
    let sick_from = earlier_to - Duration::days(2);
    // Far enough into the new month to cover workdays: the first days of a
    // month can be a weekend, and a weekend-only stretch has no payroll days.
    let sick_to = reporting_from + Duration::days(4);
    app.state
        .db
        .absences
        .create(emp_id, sick.id, true, sick_from, sick_to, None, "approved")
        .await
        .expect("straddling sick note");

    let carried = payroll_report::carry_over_boundary(&app.state.pool, reporting_from)
        .await
        .expect("carry-over boundary");
    let members = payroll_report::payroll_members(
        &app.state,
        reporting_from,
        reporting_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: reporting_from,
            to: reporting_to,
            interim: false,
            created_on: reporting_to,
            carried,
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the report");

    // The earlier days are declared as a catch-up, ending at the last day of
    // the closed month — not running on into the month being reported, whose
    // days the ordinary absence table already shows.
    assert_eq!(
        data.late_absence_rows
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>(),
        vec![(sick_from, earlier_to)],
        "the days that predate this report, and only those"
    );

    // And the ordinary table shows the part inside the reported month, so
    // between them every day of the absence is declared exactly once.
    let ordinary = data.absence_rows.as_ref().expect("absence section");
    assert_eq!(
        ordinary
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>(),
        vec![(reporting_from, sick_to)],
        "the in-month part, clamped as usual"
    );

    app.cleanup().await;
}

/// A month can be owed without being queued yet, and carry-over has to treat it
/// as owed all the same.
///
/// The queue is backfilled at the start of a run, so for the first days of every
/// month — before the configured send day — the month just finished is due but
/// not yet in it. Reading "owed" off the queue alone made the running month's
/// payroll card offer the whole previous month as catch-up days, hours and all,
/// while that month's own report was still to come: the same days on two cards.
#[tokio::test]
async fn a_month_due_but_not_yet_queued_is_not_raided_for_catch_up_days() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "not-queued").await;
    let lead = login_change_pw(&app, "lead-not-queued@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "not-queued").await;
    let assistant = login_change_pw(&app, "aushilfe-not-queued@example.com", &assistant_pw).await;

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let (current_from, _current_to) = month_bounds(today);
    let (previous_from, _previous_to) = month_bounds(current_from - Duration::days(1));
    let previous_period = previous_from.format("%Y-%m").to_string();
    let (two_back_from, _) = month_bounds(previous_from - Duration::days(1));
    let two_back_period = two_back_from.format("%Y-%m").to_string();

    // A Monday in the middle of the month just finished — every month has one
    // in this window, whatever day of the week it starts on.
    let mut worked = previous_from + Duration::days(9);
    while worked.weekday() != chrono::Weekday::Mon {
        worked += Duration::days(1);
    }
    let entry_id =
        create_and_submit_entry(&assistant, &worked.format("%Y-%m-%d").to_string(), cat_id).await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids":[entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the day: {body}");

    // Reports are up to date through the month before last. The month just
    // finished is due, but no run has queued it yet.
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &two_back_period,
        )
        .await
        .expect("record the queued period");
    assert!(
        app.state
            .db
            .payroll_queue
            .list_pending()
            .await
            .expect("queue")
            .is_empty(),
        "nothing is queued yet — that is the whole point"
    );

    let carried = payroll_report::carry_over_boundary(&app.state.pool, current_from)
        .await
        .expect("carry-over boundary");
    assert!(
        carried
            .as_ref()
            .is_some_and(|c| c.owed_periods.contains(&previous_period)),
        "a month whose report is still to come is owed, queued or not: {:?}",
        carried.as_ref().map(|c| c.owed_periods.clone())
    );

    let members = payroll_report::payroll_members(
        &app.state,
        current_from,
        today,
        &[],
        true,
        carried.as_ref(),
    )
    .await
    .expect("members");

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: current_from,
            to: today,
            interim: true,
            created_on: today,
            carried,
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the running month's snapshot");

    assert!(
        data.late_entry_rows.is_empty(),
        "the previous month's own report will print these days: {:?}",
        data.late_entry_rows
            .iter()
            .map(|row| (row.date, row.minutes))
            .collect::<Vec<_>>()
    );
    assert!(
        !members.iter().any(|member| member.id == assistant_id),
        "and nothing about that month makes the assistant part of this one"
    );

    app.cleanup().await;
}

/// An assistant's own absence never reaches the payroll report — they are paid
/// by the hour, so continued pay does not apply — and a late-filed one must not
/// drag them into the report's covered set either.
///
/// Nothing would ever get them back out again: only non-assistants' absences
/// are marked as declared, so the note stays unmarked and would pull them into
/// every future month's covered set for ever, to produce no row in any of them.
#[tokio::test]
async fn an_assistants_late_sick_note_does_not_pull_them_into_the_report() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "aux-sick").await;
    let _lead = login_change_pw(&app, "lead-aux-sick@example.com", &lead_pw).await;
    let (assistant_id, _assistant_pw) = create_assistant(&admin, lead_id, "aux-sick").await;

    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let reporting = zerf::background::schedule::previous_period(today);
    let (reporting_from, reporting_to) =
        zerf::background::schedule::period_bounds(&reporting).expect("bounds");
    let (earlier_from, earlier_to) = month_bounds(reporting_from - Duration::days(1));
    let earlier_period = earlier_from.format("%Y-%m").to_string();
    let sick = absence_cat(&app.state.pool, "sick").await;

    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &earlier_period,
        )
        .await
        .expect("record the queued period");

    // Filed after the earlier month's report went out, and entirely inside it:
    // the assistant booked no time at all, in either month.
    app.state
        .db
        .absences
        .create(
            assistant_id,
            sick.id,
            true,
            earlier_to - Duration::days(3),
            earlier_to,
            None,
            "approved",
        )
        .await
        .expect("the assistant's sick note");

    let carried = payroll_report::carry_over_boundary(&app.state.pool, reporting_from)
        .await
        .expect("carry-over boundary");
    let members = payroll_report::payroll_members(
        &app.state,
        reporting_from,
        reporting_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");

    assert!(
        !members.iter().any(|member| member.id == assistant_id),
        "an assistant with nothing but an absence is not covered by the report"
    );

    app.cleanup().await;
}

/// A sick note running out of a month that has been reported and into one whose
/// own report is still stuck must have its earlier days declared now.
///
/// The month still owed will print its own days when it finally goes out, so
/// they must not be taken here. But dropping the whole absence for that reason
/// loses the earlier days outright: the owed month's report marks the absence
/// as declared the moment it prints its half, and the catch-up path never looks
/// at it again. Nobody ever claims continued pay for those days.
///
/// The queue really can have such a gap — one month held up behind a late
/// approval while later ones are delivered is exactly why carry-over skips
/// owed months by name rather than by a single cut-off date.
#[tokio::test]
async fn a_sick_note_running_into_a_month_still_owed_still_declares_the_reported_part() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "owed-span").await;
    let _lead = login_change_pw(&app, "lead-owed-span@example.com", &lead_pw).await;

    // Three months: the one being reported, the one before it whose report is
    // still queued, and the one before that, which has been delivered.
    let today = zerf::services::settings::app_today(&app.state.pool).await;
    let reporting = zerf::background::schedule::previous_period(today);
    let (reporting_from, reporting_to) =
        zerf::background::schedule::period_bounds(&reporting).expect("bounds");
    let (owed_from, _owed_to) = month_bounds(reporting_from - Duration::days(1));
    let owed_period = owed_from.format("%Y-%m").to_string();
    let (_delivered_from, delivered_to) = month_bounds(owed_from - Duration::days(1));
    let sick = absence_cat(&app.state.pool, "sick").await;

    // The older month has been delivered; the one after it is still queued.
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &reporting,
        )
        .await
        .expect("record the queued period");
    app.state
        .db
        .payroll_queue
        .enqueue(&owed_period)
        .await
        .expect("the middle month is still owed");

    // The sick note straddles the two: the last three days of the delivered
    // month and the first days of the one still owed.
    let sick_from = delivered_to - Duration::days(2);
    let sick_to = owed_from + Duration::days(4);
    let absence = app
        .state
        .db
        .absences
        .create(emp_id, sick.id, true, sick_from, sick_to, None, "approved")
        .await
        .expect("straddling sick note");

    let carried = payroll_report::carry_over_boundary(&app.state.pool, reporting_from)
        .await
        .expect("carry-over boundary");
    assert!(
        carried
            .as_ref()
            .is_some_and(|c| c.owed_periods.contains(&owed_period)),
        "the middle month has to be owed for this test to mean anything"
    );
    let members = payroll_report::payroll_members(
        &app.state,
        reporting_from,
        reporting_to,
        &[],
        false,
        carried.as_ref(),
    )
    .await
    .expect("members");

    let language = zerf::i18n::Language::from_setting("en");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: reporting_from,
            to: reporting_to,
            interim: false,
            created_on: reporting_to,
            carried: carried.clone(),
        },
        &members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the report");

    // Only the delivered month's days. The owed month's days are left to its
    // own report, which has not gone out yet.
    assert_eq!(
        data.late_absence_rows
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>(),
        vec![(sick_from, delivered_to)],
        "the days no report will ever print unless this one does"
    );
    assert_eq!(
        data.late_absence_ids,
        vec![absence.id],
        "and the document records the absence it declared, so the send marks \
         exactly that"
    );

    // Marking is what the send does next. It must not stop the month that is
    // still owed from printing its own half through the ordinary path.
    app.state
        .db
        .reports
        .mark_payroll_reported_absences(
            &reporting,
            reporting_from,
            reporting_to,
            &[emp_id],
            &data.late_absence_ids,
        )
        .await
        .expect("mark what this report declared");

    let owed_members = payroll_report::payroll_members(
        &app.state,
        owed_from,
        sick_to.max(owed_from),
        &[],
        false,
        None,
    )
    .await
    .expect("members of the owed month");
    let owed_data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: owed_from,
            to: month_bounds(owed_from).1,
            interim: false,
            created_on: reporting_to,
            carried: None,
        },
        &owed_members,
        &config(true, false),
        &language,
        None,
    )
    .await
    .expect("build the owed month's report");
    assert_eq!(
        owed_data
            .absence_rows
            .as_ref()
            .expect("absence section")
            .iter()
            .map(|row| (row.from, row.to))
            .collect::<Vec<_>>(),
        vec![(owed_from, sick_to)],
        "the owed month still prints its own half, so between the two reports \
         every day is declared exactly once"
    );

    app.cleanup().await;
}

/// A replacement row is not new money when payroll has already received the
/// same net person-day total. Reopening and deleting removes the entry marker,
/// so this exercises the declaration ledger as the durable baseline and
/// verifies that a zero correction does not leave a phantom assistant.
#[tokio::test]
async fn payroll_declared_days_exact_rebook_has_no_correction_or_phantom_member() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-rebook").await;
    let lead = login_change_pw(&app, "lead-ledger-rebook@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-rebook").await;
    let assistant =
        login_change_pw(&app, "aushilfe-ledger-rebook@example.com", &assistant_pw).await;

    let worked_day = anchor_monday();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();
    let original = create_and_submit_entry(
        &assistant,
        &worked_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [original]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve original day: {body}");

    let period_entries = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot reported entries");
    let recorded = app
        .state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &period_entries,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the report that paid the original row");
    assert_eq!(recorded.0, 1, "one declared person-day was recorded");
    assert_eq!(recorded.2, 1, "the original entry was marked as reported");

    let (status, body) = assistant
        .post(
            "/api/v1/reopen-requests",
            &json!({
                "week_start": worked_day.format("%Y-%m-%d").to_string(),
                "reason": "replace the booking"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "request reopen: {body}");
    assert_eq!(body["status"], "pending");
    let reopen_id = id(&body);
    let (status, body) = lead
        .post(
            &format!("/api/v1/reopen-requests/{reopen_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve reopen: {body}");
    assert_eq!(body["entries_reopened"], 1);

    let (status, body) = assistant
        .delete(&format!("/api/v1/time-entries/{original}"))
        .await;
    assert_eq!(status, StatusCode::OK, "delete original draft: {body}");
    let replacement = create_and_submit_entry(
        &assistant,
        &worked_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [replacement]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve replacement day: {body}");

    let next_from = to + Duration::days(1);
    let (_next_month_start, next_to) = month_bounds(next_from);
    let carried = payroll_report::CarriedDays {
        since: from,
        before: next_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let members =
        payroll_report::payroll_members(&app.state, next_from, next_to, &[], false, Some(&carried))
            .await
            .expect("load next report members");
    assert!(
        !members.iter().any(|member| member.id == assistant_id),
        "an exact rebook has no correction and must not create a phantom member"
    );

    let assistant_member = app
        .state
        .db
        .users
        .find_by_id(assistant_id)
        .await
        .expect("load assistant")
        .expect("assistant exists");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: Some(carried),
        },
        std::slice::from_ref(&assistant_member),
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build next report with the assistant forced into scope");
    assert!(
        data.late_entry_rows.is_empty(),
        "the replacement's 240 minutes equal the 240 minutes already declared"
    );
    assert!(
        data.declared_work_days.is_empty(),
        "a zero difference must not be written as a new declaration"
    );

    app.cleanup().await;
}

/// A newly added shift changes the value of its whole day. The automatic
/// break therefore has to be calculated over the original and new shifts
/// together before subtracting the amount payroll already received.
#[tokio::test]
async fn payroll_declared_days_second_shift_recomputes_the_whole_days_break() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-break").await;
    let lead = login_change_pw(&app, "lead-ledger-break@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-break").await;
    let assistant = login_change_pw(&app, "aushilfe-ledger-break@example.com", &assistant_pw).await;

    for (key, value) in [
        (zerf::services::settings::AUTO_BREAK_ENABLED_KEY, "true"),
        (
            zerf::services::settings::AUTO_BREAK_THRESHOLD_HOURS_KEY,
            "6",
        ),
        (
            zerf::services::settings::AUTO_BREAK_DEDUCTION_MINUTES_KEY,
            "30",
        ),
    ] {
        app.state
            .db
            .settings
            .save_setting(key, value)
            .await
            .expect("configure automatic break");
    }

    let worked_day = anchor_monday();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();
    let first_shift = create_and_submit_entry(
        &assistant,
        &worked_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [first_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve first shift: {body}");
    let period_entries = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot reported entries");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &period_entries,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the first shift's report");

    let (status, body) = assistant
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": worked_day.format("%Y-%m-%d").to_string(),
                "start_time": "12:00",
                "end_time": "15:30",
                "category_id": cat_id,
                "comment": "late second shift"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create second shift: {body}");
    let second_shift = id(&body);
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [second_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "submit second shift: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [second_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve second shift: {body}");

    let next_from = to + Duration::days(1);
    let (_next_month_start, next_to) = month_bounds(next_from);
    let carried = payroll_report::CarriedDays {
        since: from,
        before: next_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let members =
        payroll_report::payroll_members(&app.state, next_from, next_to, &[], false, Some(&carried))
            .await
            .expect("load correction report members");
    assert!(
        members.iter().any(|member| member.id == assistant_id),
        "the positive day correction brings the assistant into the report"
    );
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: Some(carried),
        },
        &members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build the correction report");

    assert_eq!(data.late_entry_rows.len(), 1);
    assert_eq!(data.late_entry_rows[0].date, worked_day);
    assert_eq!(
        data.late_entry_rows[0].minutes, 180,
        "450 raw minutes minus the 30-minute whole-day break, less 240 already declared"
    );
    assert_eq!(
        data.declared_work_days
            .iter()
            .map(|day| (day.user_id, day.date, day.minutes))
            .collect::<Vec<_>>(),
        vec![(assistant_id, worked_day, 180)],
        "the ledger receives exactly the signed correction printed in the document"
    );

    app.cleanup().await;
}

/// Moving work after it was declared changes two person-days: the former day
/// is reduced to zero and the corrected day gains the time. Both signed rows
/// must be delivered, while the original report remains an immutable record
/// of what payroll received at that time.
#[tokio::test]
async fn payroll_declared_days_cross_month_move_is_signed_and_history_stays_fixed() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-move").await;
    let lead = login_change_pw(&app, "lead-ledger-move@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-move").await;
    let assistant = login_change_pw(&app, "aushilfe-ledger-move@example.com", &assistant_pw).await;

    let original_day = anchor_monday();
    let (from, to) = month_bounds(original_day);
    let period = from.format("%Y-%m").to_string();
    let entry_id = create_and_submit_entry(
        &assistant,
        &original_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve original day: {body}");
    let original_snapshot = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot original report entries");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &original_snapshot,
            &[(assistant_id, original_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the original report");

    let (earlier_from, earlier_to) = month_bounds(from - Duration::days(1));
    let earlier_period = earlier_from.format("%Y-%m").to_string();
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &earlier_period,
            earlier_from,
            earlier_to,
            &[],
            &[],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the target month's known zero baseline");
    let corrected_day = earlier_from + Duration::days(9);
    let (status, body) = admin
        .put(
            &format!("/api/v1/time-entries/{entry_id}"),
            &json!({
                "entry_date": corrected_day.format("%Y-%m-%d").to_string(),
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "corrected to its real month"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "move the approved entry: {body}");

    let reporting_from = to + Duration::days(1);
    let (_reporting_month_start, reporting_to) = month_bounds(reporting_from);
    let reporting_period = reporting_from.format("%Y-%m").to_string();
    let carried = payroll_report::CarriedDays {
        since: earlier_from,
        before: reporting_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let members = payroll_report::payroll_members(
        &app.state,
        reporting_from,
        reporting_to,
        &[],
        false,
        Some(&carried),
    )
    .await
    .expect("load correction report members");
    assert!(
        members.iter().any(|member| member.id == assistant_id),
        "the signed move corrections bring the assistant into the report"
    );
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: reporting_from,
            to: reporting_to,
            interim: false,
            created_on: reporting_to,
            carried: Some(carried.clone()),
        },
        &members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build correction report");
    assert_eq!(
        data.late_entry_rows
            .iter()
            .map(|row| (row.date, row.minutes))
            .collect::<Vec<_>>(),
        vec![(corrected_day, 240), (original_day, -240)],
        "the corrected date gains the hours and the previously paid date loses them"
    );
    assert_eq!(
        data.declared_work_days
            .iter()
            .map(|day| (day.user_id, day.date, day.minutes))
            .collect::<Vec<_>>(),
        vec![
            (assistant_id, corrected_day, 240),
            (assistant_id, original_day, -240),
        ],
        "the send path receives the same signed days that the document prints"
    );

    let historical_scope = payroll_report::CarriedDays {
        since: earlier_from,
        before: from,
        owed_periods: Vec::new(),
        reported_as: Some(period.clone()),
    };
    let historical_members =
        payroll_report::payroll_members(&app.state, from, to, &[], false, Some(&historical_scope))
            .await
            .expect("load the original report's members");
    assert!(
        historical_members
            .iter()
            .any(|member| member.id == assistant_id),
        "the declaration ledger preserves the original report's member set"
    );
    let historical = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
            carried: Some(historical_scope),
        },
        &historical_members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("read the original report back from its ledger");
    assert_eq!(
        historical
            .hours_sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .map(|row| (row.work_days, row.minutes))
            .collect::<Vec<_>>(),
        vec![(1, 240)],
        "moving the live row must not rewrite the report that originally paid it"
    );
    assert!(historical.late_entry_rows.is_empty());

    let declarations: Vec<(i64, NaiveDate, i64)> = data
        .declared_work_days
        .iter()
        .map(|day| (day.user_id, day.date, day.minutes))
        .collect();
    let carried_days: Vec<(i64, NaiveDate)> = data
        .carried_work_days
        .iter()
        .map(|day| (day.user_id, day.date))
        .collect();
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &reporting_period,
            reporting_from,
            reporting_to,
            &[],
            &declarations,
            zerf::repository::PayrollCarryScope {
                since: Some(earlier_from),
                before: Some(reporting_from),
                owed_periods: &[],
                days: &carried_days,
            },
            &[],
            &[],
            &[],
        )
        .await
        .expect("record both signed corrections");

    let after_delivery = payroll_report::payroll_members(
        &app.state,
        reporting_from,
        reporting_to,
        &[],
        false,
        Some(&carried),
    )
    .await
    .expect("load members after recording the correction");
    assert!(
        !after_delivery
            .iter()
            .any(|member| member.id == assistant_id),
        "the two corrected days are settled and must not recur"
    );
    assert_eq!(
        app.state
            .db
            .reports
            .declared_days_for_period(&period)
            .await
            .expect("read the original period after recording the correction")
            .into_iter()
            .map(|day| (day.user_id, day.day, day.minutes))
            .collect::<Vec<_>>(),
        vec![(assistant_id, original_day, 240)],
        "a later signed correction never mutates the original document ledger"
    );

    app.cleanup().await;
}

/// The ledger write is runtime-checked SQL. Exercise the parallel arrays with
/// multiple users, dates, duplicate input pairs and a later period, then verify
/// both per-document readback and the sum used by correction calculations.
#[tokio::test]
async fn payroll_declared_days_multi_array_write_groups_and_sums_by_period() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, employee_id, _employee_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-arrays").await;
    let day_one = anchor_monday();
    let day_two = day_one + Duration::days(1);

    let inserted = app
        .state
        .db
        .reports
        .record_declared_days(
            "2029-01",
            &[
                (lead_id, day_one, 120),
                (lead_id, day_one, 30),
                (employee_id, day_two, 240),
            ],
        )
        .await
        .expect("execute the multi-array insert");
    assert_eq!(
        inserted, 2,
        "duplicate input pairs are grouped into two person-day rows"
    );

    let updated = app
        .state
        .db
        .reports
        .record_declared_days(
            "2029-01",
            &[(lead_id, day_one, 175), (employee_id, day_two, 200)],
        )
        .await
        .expect("repeat the same period idempotently");
    assert_eq!(updated, 2, "both existing period rows were replaced");
    let second_period = app
        .state
        .db
        .reports
        .record_declared_days(
            "2029-02",
            &[(lead_id, day_one, -25), (employee_id, day_two, 40)],
        )
        .await
        .expect("append declarations from another period");
    assert_eq!(second_period, 2);

    assert_eq!(
        app.state
            .db
            .reports
            .declared_days_for_period("2029-01")
            .await
            .expect("read one document's declarations")
            .into_iter()
            .map(|day| (day.user_id, day.day, day.minutes))
            .collect::<Vec<_>>(),
        vec![(lead_id, day_one, 175), (employee_id, day_two, 200)],
        "same-period retries replace only that document's rows"
    );
    let totals = app
        .state
        .db
        .reports
        .declared_minutes_for_days(&[
            (lead_id, day_one),
            (employee_id, day_two),
            (lead_id, day_one),
        ])
        .await
        .expect("sum declarations for parallel person-day arrays");
    assert_eq!(
        totals.len(),
        2,
        "duplicate requested pairs are deduplicated"
    );
    assert_eq!(totals.get(&(lead_id, day_one)), Some(&150));
    assert_eq!(totals.get(&(employee_id, day_two)), Some(&240));

    app.cleanup().await;
}

/// Accounting uses the entry identities and versions captured before report
/// assembly. A row added afterwards must remain unmarked even though its date
/// lies inside the reported month, or no later report could recover it.
#[tokio::test]
async fn payroll_declared_days_delivery_marks_only_the_assembled_entry_snapshot() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-snapshot").await;
    let lead = login_change_pw(&app, "lead-ledger-snapshot@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-snapshot").await;
    let assistant =
        login_change_pw(&app, "aushilfe-ledger-snapshot@example.com", &assistant_pw).await;

    let first_day = anchor_monday();
    let second_day = first_day + Duration::days(1);
    let (from, to) = month_bounds(first_day);
    let period = from.format("%Y-%m").to_string();
    let first_id = create_and_submit_entry(
        &assistant,
        &first_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [first_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve assembled entry: {body}");
    let snapshot = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("capture report entry snapshot");
    let members = payroll_report::payroll_members(&app.state, from, to, &[], false, None)
        .await
        .expect("load assembled report members");
    let assembled_data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from,
            to,
            interim: false,
            created_on: to,
            carried: None,
        },
        &members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build the assembled document");
    let assembled_content = payroll_report::reported_content_rows(&assembled_data);
    assert_eq!(
        assembled_content.len(),
        1,
        "the assembled document contains the first assistant day"
    );

    let second_id = create_and_submit_entry(
        &assistant,
        &second_day.format("%Y-%m-%d").to_string(),
        cat_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [second_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve concurrent entry: {body}");
    app.state
        .db
        .payroll_queue
        .enqueue(&period)
        .await
        .expect("queue period before accounting");
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
            &period,
        )
        .await
        .expect("record the period as having reached the queue");

    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &snapshot,
            &[(assistant_id, first_day, 240)],
            Default::default(),
            &[],
            &[],
            &assembled_content,
        )
        .await
        .expect("record delivery from the assembled snapshot");

    let markers: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, payroll_reported_period FROM time_entries WHERE id = ANY($1) ORDER BY id",
    )
    .bind([first_id, second_id])
    .fetch_all(&app.state.pool)
    .await
    .expect("read entry markers");
    assert_eq!(
        markers,
        vec![(first_id, Some(period.clone())), (second_id, None)],
        "only the unchanged row captured before assembly may be marked"
    );
    assert!(
        !app.state
            .db
            .payroll_queue
            .list_pending()
            .await
            .expect("read settled queue")
            .contains(&period),
        "ledger, markers, and queue settlement commit together"
    );

    for (key, value) in [
        (
            zerf::services::settings::PAYROLL_REPORT_ENABLED_KEY,
            "true".to_string(),
        ),
        (
            zerf::services::settings::PAYROLL_REPORT_ASSISTANT_HOURS_KEY,
            "false".to_string(),
        ),
        (
            zerf::services::settings::PAYROLL_REPORT_EXCLUDED_USERS_KEY,
            assistant_id.to_string(),
        ),
    ] {
        app.state
            .db
            .settings
            .save_setting(key, &value)
            .await
            .expect("change current payroll settings");
    }
    let (status, historical) = admin.get("/api/v1/reports/payroll-content").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "load delivered content: {historical}"
    );
    assert_eq!(historical["sent"], true);
    assert_eq!(
        historical["minutes"], 240,
        "later settings and exclusions must not rewrite the delivered document"
    );
    assert_eq!(
        historical["rows"].as_array().map(Vec::len),
        Some(1),
        "the delivered card reads its exact stored row set"
    );

    app.cleanup().await;
}

/// A failure in the final content-snapshot write must roll back the earlier
/// period marker, day declarations, live-row markers, and queue deletion. The
/// next scheduler pass then retries a complete report instead of continuing
/// from a partially accounted state.
#[tokio::test]
async fn payroll_delivery_accounting_is_atomic() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _employee_id, _employee_pw, _monday, category_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-atomic").await;
    let lead = login_change_pw(&app, "lead-ledger-atomic@example.com", &lead_pw).await;
    let (assistant_id, assistant_password) =
        create_assistant(&admin, lead_id, "ledger-atomic").await;
    let assistant = login_change_pw(
        &app,
        "aushilfe-ledger-atomic@example.com",
        &assistant_password,
    )
    .await;

    let worked_day = anchor_monday();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();
    let entry_id = create_and_submit_entry(
        &assistant,
        &worked_day.format("%Y-%m-%d").to_string(),
        category_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve reported shift: {body}");
    let snapshot = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot reported shift");
    app.state
        .db
        .payroll_queue
        .enqueue(&period)
        .await
        .expect("queue period");

    let invalid_content = zerf::repository::PayrollReportedContentRow {
        user_id: assistant_id,
        employee: "Atomic, Alex".to_string(),
        kind: "invalid".to_string(),
        category: None,
        from_date: None,
        to_date: None,
        days: 1.0,
        minutes: Some(240),
        medical_certificate_required: None,
    };
    let result = app
        .state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &snapshot,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[invalid_content],
        )
        .await;
    assert!(
        result.is_err(),
        "the database kind constraint must reject invalid rendered content"
    );
    assert!(
        !app
            .state
            .db
            .reports
            .payroll_period_accounted(&period)
            .await
            .expect("check period marker after rollback"),
        "the period-level marker must roll back"
    );
    assert!(
        app.state
            .db
            .reports
            .declared_days_for_period(&period)
            .await
            .expect("check declarations after rollback")
            .is_empty(),
        "day declarations must roll back"
    );
    let entry_marker: Option<String> =
        sqlx::query_scalar("SELECT payroll_reported_period FROM time_entries WHERE id=$1")
            .bind(entry_id)
            .fetch_one(&app.state.pool)
            .await
            .expect("check entry marker after rollback");
    assert_eq!(
        entry_marker, None,
        "the live entry marker must roll back with the ledger"
    );
    assert!(
        app.state
            .db
            .payroll_queue
            .list_pending()
            .await
            .expect("check queue after rollback")
            .contains(&period),
        "the failed accounting transaction must leave the period queued"
    );
    assert!(
        app.state
            .db
            .reports
            .payroll_reported_content(&period)
            .await
            .expect("check rendered content after rollback")
            .is_empty(),
        "no partial content snapshot may survive"
    );

    app.cleanup().await;
}

/// Reopening a reported day is not itself a deletion. Corrections wait while
/// any older entry for that person is unsettled, then compare the final whole
/// day even when a surviving shift still carries its original row marker.
#[tokio::test]
async fn payroll_declared_days_partial_deletion_waits_for_the_reopen_to_settle() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-delete").await;
    let lead = login_change_pw(&app, "lead-ledger-delete@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-delete").await;
    let assistant =
        login_change_pw(&app, "aushilfe-ledger-delete@example.com", &assistant_pw).await;

    let worked_day = anchor_monday();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();
    let mut entry_ids = Vec::new();
    for (start, end) in [("08:00", "10:00"), ("10:00", "12:00")] {
        let (status, body) = assistant
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": worked_day.format("%Y-%m-%d").to_string(),
                    "start_time": start,
                    "end_time": end,
                    "category_id": cat_id,
                    "comment": "reported split shift"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "create reported shift: {body}");
        entry_ids.push(id(&body));
    }
    let (status, body) = assistant
        .post("/api/v1/time-entries/submit", &json!({"ids": entry_ids}))
        .await;
    assert_eq!(status, StatusCode::OK, "submit reported shifts: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": entry_ids}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve reported shifts: {body}");
    let snapshot = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot reported shifts");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &snapshot,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record split day report");

    let (status, body) = assistant
        .post(
            "/api/v1/reopen-requests",
            &json!({
                "week_start": worked_day.format("%Y-%m-%d").to_string(),
                "reason": "remove duplicate shift"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "request reopen: {body}");
    let reopen_id = id(&body);
    let (status, body) = lead
        .post(
            &format!("/api/v1/reopen-requests/{reopen_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve reopen: {body}");
    let removed_id = entry_ids[1];
    let (status, body) = assistant
        .delete(&format!("/api/v1/time-entries/{removed_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "delete duplicate shift: {body}");

    let next_from = to + Duration::days(1);
    let (_next_start, next_to) = month_bounds(next_from);
    let carried = payroll_report::CarriedDays {
        since: from,
        before: next_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let assistant_member = app
        .state
        .db
        .users
        .find_by_id(assistant_id)
        .await
        .expect("load assistant")
        .expect("assistant exists");
    let report_config = config(true, false);
    let language = zerf::i18n::Language::from_setting("en");
    let build = || {
        payroll_report::build_report_data(
            &app.state,
            payroll_report::ReportWindow {
                from: next_from,
                to: next_to,
                interim: false,
                created_on: next_to,
                carried: Some(carried.clone()),
            },
            std::slice::from_ref(&assistant_member),
            &report_config,
            &language,
            None,
        )
    };
    let unsettled = build().await.expect("build while reopen is unfinished");
    assert!(
        unsettled.late_entry_rows.is_empty(),
        "a draft survivor must not make the reported day look deleted"
    );

    let surviving_id = entry_ids[0];
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [surviving_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "resubmit surviving shift: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [surviving_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve surviving shift: {body}");

    let settled = build().await.expect("build after reopen settles");
    assert_eq!(settled.late_entry_rows.len(), 1);
    assert_eq!(settled.late_entry_rows[0].date, worked_day);
    assert_eq!(
        settled.late_entry_rows[0].minutes, -120,
        "the remaining marked shift is compared with the full 240 minutes already declared"
    );

    app.cleanup().await;
}

/// A day without a declaration row stays on the legacy entry-marker fallback,
/// even when its own month was settled after the ledger migration. Once a late
/// shift on such a mixed day has been carried, editing it inside the same month
/// must not clear its marker and pay the entire edited shift again.
#[tokio::test]
async fn payroll_no_ledger_fallback_edit_does_not_repeat_a_carried_shift() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _employee_id, _employee_pw, _monday, category_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-fallback-edit").await;
    let lead = login_change_pw(&app, "lead-ledger-fallback-edit@example.com", &lead_pw).await;
    let (assistant_id, assistant_password) =
        create_assistant(&admin, lead_id, "ledger-fallback-edit").await;
    let assistant = login_change_pw(
        &app,
        "aushilfe-ledger-fallback-edit@example.com",
        &assistant_password,
    )
    .await;

    let worked_day = anchor_monday();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();
    let original = create_and_submit_entry(
        &assistant,
        &worked_day.format("%Y-%m-%d").to_string(),
        category_id,
    )
    .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [original]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve original shift: {body}");

    let original_snapshot = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot original month");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &original_snapshot,
            &[],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("settle original month without a day declaration");

    let (status, body) = assistant
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": worked_day.format("%Y-%m-%d").to_string(),
                "start_time": "12:00",
                "end_time": "14:00",
                "category_id": category_id,
                "comment": "late fallback shift"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create fallback shift: {body}");
    let fallback_entry = id(&body);
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [fallback_entry]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "submit fallback shift: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [fallback_entry]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve fallback shift: {body}");

    let reporting_from = to + Duration::days(1);
    let (_, reporting_to) = month_bounds(reporting_from);
    let reporting_period = reporting_from.format("%Y-%m").to_string();
    let carried = payroll_report::CarriedDays {
        since: from,
        before: reporting_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let assistant_member = app
        .state
        .db
        .users
        .find_by_id(assistant_id)
        .await
        .expect("load assistant")
        .expect("assistant exists");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: reporting_from,
            to: reporting_to,
            interim: false,
            created_on: reporting_to,
            carried: Some(carried.clone()),
        },
        std::slice::from_ref(&assistant_member),
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build fallback correction report");
    assert_eq!(
        data.late_entry_rows
            .iter()
            .map(|row| (row.date, row.minutes))
            .collect::<Vec<_>>(),
        vec![(worked_day, 120)],
        "only the new unmarked shift is carried on a mixed no-ledger day"
    );
    assert!(
        data.declared_work_days.is_empty(),
        "an unknowable historical baseline must not be converted into a partial ledger baseline"
    );

    let carried_days: Vec<(i64, NaiveDate)> = data
        .carried_work_days
        .iter()
        .map(|day| (day.user_id, day.date))
        .collect();
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &reporting_period,
            reporting_from,
            reporting_to,
            &[],
            &[],
            zerf::repository::PayrollCarryScope {
                since: Some(from),
                before: Some(reporting_from),
                owed_periods: &[],
                days: &carried_days,
            },
            &[],
            &[],
            &[],
        )
        .await
        .expect("mark the fallback shift as carried");

    let (status, body) = admin
        .put(
            &format!("/api/v1/time-entries/{fallback_entry}"),
            &json!({
                "entry_date": worked_day.format("%Y-%m-%d").to_string(),
                "start_time": "12:00",
                "end_time": "15:00",
                "category_id": category_id,
                "comment": "edited legacy fallback"
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "edit carried fallback shift: {body}"
    );

    let marker: Option<String> =
        sqlx::query_scalar("SELECT payroll_reported_period FROM time_entries WHERE id=$1")
            .bind(fallback_entry)
            .fetch_one(&app.state.pool)
            .await
            .expect("read fallback marker");
    assert_eq!(
        marker,
        Some(reporting_period),
        "a same-month edit cannot clear a no-ledger fallback marker"
    );

    let next_from = reporting_to + Duration::days(1);
    let (_, next_to) = month_bounds(next_from);
    let after_edit = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: Some(payroll_report::CarriedDays {
                since: from,
                before: next_from,
                owed_periods: Vec::new(),
                reported_as: None,
            }),
        },
        std::slice::from_ref(&assistant_member),
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build report after fallback edit");
    assert!(
        after_edit.late_entry_rows.is_empty(),
        "the edited fallback shift must not be paid again in full"
    );

    app.cleanup().await;
}

/// Delivery accounting must mark the absence ids captured by the assembled
/// document, not every currently matching absence from a second broad query.
#[tokio::test]
async fn payroll_delivery_marks_only_the_absences_in_the_assembled_document() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, employee_id, _employee_pw, _monday, _category_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-absence-snapshot").await;

    let first_day = anchor_monday();
    let second_day = first_day + Duration::days(1);
    let (from, to) = month_bounds(first_day);
    let period = from.format("%Y-%m").to_string();
    let sick = absence_cat(&app.state.pool, "sick").await;
    let assembled = app
        .state
        .db
        .absences
        .create(
            employee_id,
            sick.id,
            true,
            first_day,
            first_day,
            None,
            "approved",
        )
        .await
        .expect("create absence included in the document");
    let concurrent = app
        .state
        .db
        .absences
        .create(
            employee_id,
            sick.id,
            true,
            second_day,
            second_day,
            None,
            "approved",
        )
        .await
        .expect("create absence after document assembly");

    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &[],
            &[],
            Default::default(),
            &[assembled.id],
            &[],
            &[],
        )
        .await
        .expect("record the assembled report");

    let markers: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, payroll_reported_period FROM absences WHERE id = ANY($1) ORDER BY id",
    )
    .bind([assembled.id, concurrent.id])
    .fetch_all(&app.state.pool)
    .await
    .expect("read absence markers");
    assert_eq!(
        markers,
        vec![(assembled.id, Some(period)), (concurrent.id, None),],
        "only the absence that produced a document row may be marked"
    );

    app.cleanup().await;
}

/// Reproduces a reported dashboard bug: on the first days of a new month, the
/// Submissions tile showed a person as fully "Done" for the just-finished
/// month even though the week straddling the boundary — whose Monday belongs
/// to that month — had nothing booked at all.
///
/// The straddling week counts for the finished month via its one in-month
/// day (`weeks_in_month_to_judge`: "a week belongs to a month as soon as any
/// of its days do"), and that day must itself carry a submitted/approved
/// status or be excused, or the week — and so the month — is not accounted
/// for. Only the fully-elapsed prior week is handled here; nothing is booked
/// for the straddling week itself.
#[tokio::test]
async fn submissions_tile_requires_the_straddling_weeks_in_month_day_too() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "straddle-submissions").await;
    let lead = login_change_pw(&app, "lead-straddle-submissions@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-straddle-submissions@example.com", &emp_pw).await;
    assert_ne!(lead_id, emp_id);

    // The last fully-elapsed week before the straddling one. A single
    // submitted+approved day is enough to carry this whole week.
    let prior_week_monday = next_monday(-14);
    // The straddling week's Monday: the only day of that week belonging to
    // the finished month being judged. Both Mondays must land in the same
    // month, or this test would not exercise the straddle at all.
    let straddling_monday = next_monday(-7);
    assert_eq!(
        straddling_monday.month(),
        prior_week_monday.month(),
        "the reference date must place both weeks in one month"
    );
    let entry_id =
        create_and_submit_entry(&employee, &prior_week_monday.format("%Y-%m-%d").to_string(), cat_id)
            .await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [entry_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the prior week's day: {body}");

    // The straddling week's Monday is left completely untouched: no entry,
    // no absence.
    let (status, card) = admin.get("/api/v1/reports/submission-status").await;
    assert_eq!(status, StatusCode::OK, "fetch submission status: {card}");
    let members = card["members"].as_array().expect("members");
    let member = members
        .iter()
        .find(|member| member["user_id"].as_i64() == Some(emp_id))
        .unwrap_or_else(|| panic!("employee missing from submission status: {card}"));
    assert_ne!(
        member["status"], "ready",
        "the straddling week's unbooked in-month day must keep the month open: {card}"
    );

    app.cleanup().await;
}

/// Two corrections to the same day in successive reports must each declare
/// only their own increment.
///
/// The baseline for a day is the *sum* of every declaration ever made for it,
/// not the most recent one. Reading only the latest row would make the second
/// correction re-declare everything the first one already added, paying the
/// same minutes twice.
#[tokio::test]
async fn payroll_declared_days_successive_corrections_accumulate_the_baseline() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-twice").await;
    let lead = login_change_pw(&app, "lead-ledger-twice@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-twice").await;
    let assistant = login_change_pw(&app, "aushilfe-ledger-twice@example.com", &assistant_pw).await;
    let language = zerf::i18n::Language::from_setting("en");

    let worked_day = anchor_monday();
    let worked_iso = worked_day.format("%Y-%m-%d").to_string();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();

    // The day as its own month's report declared it: 08:00-12:00.
    let first_shift = create_and_submit_entry(&assistant, &worked_iso, cat_id).await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [first_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the first shift: {body}");
    let period_entries = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot the reported month");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &period_entries,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the original report");

    let add_shift = |start: &'static str, end: &'static str| {
        let assistant = &assistant;
        let lead = &lead;
        let worked_iso = worked_iso.clone();
        async move {
            let (status, body) = assistant
                .post(
                    "/api/v1/time-entries",
                    &json!({
                        "entry_date": worked_iso,
                        "start_time": start,
                        "end_time": end,
                        "category_id": cat_id,
                        "comment": "late shift"
                    }),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "create late shift: {body}");
            let entry_id = id(&body);
            let (status, body) = assistant
                .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
                .await;
            assert_eq!(status, StatusCode::OK, "submit late shift: {body}");
            let (status, body) = lead
                .post(
                    "/api/v1/time-entries/batch-approve",
                    &json!({"ids": [entry_id]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "approve late shift: {body}");
        }
    };

    // Each correcting report covers the month after the one before it, so the
    // two declarations land under different periods and must be summed.
    let correct = |report_from: NaiveDate, report_to: NaiveDate| {
        let app = &app;
        let language = &language;
        async move {
            let carried = payroll_report::CarriedDays {
                since: from,
                before: report_from,
                owed_periods: Vec::new(),
                reported_as: None,
            };
            let members = payroll_report::payroll_members(
                &app.state,
                report_from,
                report_to,
                &[],
                false,
                Some(&carried),
            )
            .await
            .expect("members of the correcting report");
            let data = payroll_report::build_report_data(
                &app.state,
                payroll_report::ReportWindow {
                    from: report_from,
                    to: report_to,
                    interim: false,
                    created_on: report_to,
                    carried: Some(carried.clone()),
                },
                &members,
                &config(true, false),
                language,
                None,
            )
            .await
            .expect("build the correcting report");
            let declared: Vec<(i64, NaiveDate, i64)> = data
                .declared_work_days
                .iter()
                .map(|day| (day.user_id, day.date, day.minutes))
                .collect();
            let carried_days: Vec<(i64, NaiveDate)> = data
                .carried_work_days
                .iter()
                .map(|day| (day.user_id, day.date))
                .collect();
            let report_period = report_from.format("%Y-%m").to_string();
            app.state
                .db
                .reports
                .record_payroll_report_delivery(
                    &report_period,
                    report_from,
                    report_to,
                    &[],
                    &declared,
                    zerf::repository::PayrollCarryScope {
                        since: Some(carried.since),
                        before: Some(carried.before),
                        owed_periods: &[],
                        days: &carried_days,
                    },
                    &[],
                    &data.late_absence_ids,
                    &[],
                )
                .await
                .expect("record the correcting report");
            data.late_entry_rows
                .iter()
                .map(|row| (row.date, row.minutes))
                .collect::<Vec<_>>()
        }
    };

    // First correction: a 13:00-15:00 shift adds two hours to a 240-minute day.
    add_shift("13:00", "15:00").await;
    let first_from = to + Duration::days(1);
    let (_, first_to) = month_bounds(first_from);
    assert_eq!(
        correct(first_from, first_to).await,
        vec![(worked_day, 120)],
        "the first correction declares only the added two hours"
    );

    // Second correction: another hour on the same day. The baseline is now
    // 240 + 120, so only the new hour may be declared.
    add_shift("16:00", "17:00").await;
    let second_from = first_to + Duration::days(1);
    let (_, second_to) = month_bounds(second_from);
    assert_eq!(
        correct(second_from, second_to).await,
        vec![(worked_day, 60)],
        "the second correction declares only the newly added hour, not the sum \
         of everything since the original report"
    );

    let total: i64 = app
        .state
        .db
        .reports
        .declared_minutes_for_days(&[(assistant_id, worked_day)])
        .await
        .expect("read the day's declared total")
        .get(&(assistant_id, worked_day))
        .copied()
        .unwrap_or_default();
    assert_eq!(
        total, 420,
        "the ledger sums to exactly the day's current worth: 240 + 120 + 60"
    );

    app.cleanup().await;
}

/// A correction for somebody who has since been archived still reaches the
/// report. Leaving is exactly when a forgotten shift surfaces, and the
/// period-scoped member query cannot see an archived account with no activity
/// in the month now being reported.
#[tokio::test]
async fn payroll_declared_days_correction_survives_the_person_being_archived() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-archived").await;
    let lead = login_change_pw(&app, "lead-ledger-archived@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-archived").await;
    let assistant =
        login_change_pw(&app, "aushilfe-ledger-archived@example.com", &assistant_pw).await;

    let worked_day = anchor_monday();
    let worked_iso = worked_day.format("%Y-%m-%d").to_string();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();

    let first_shift = create_and_submit_entry(&assistant, &worked_iso, cat_id).await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [first_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the reported shift: {body}");
    let period_entries = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot the reported month");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &period_entries,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the original report");

    // The forgotten shift arrives, is approved, and only then does the person
    // leave the organisation.
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": worked_iso,
                "start_time": "13:00",
                "end_time": "15:00",
                "category_id": cat_id,
                "comment": "remembered too late"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create the forgotten shift: {body}");
    let late_shift = id(&body);
    let (status, body) = assistant
        .post("/api/v1/time-entries/submit", &json!({"ids": [late_shift]}))
        .await;
    assert_eq!(status, StatusCode::OK, "submit the forgotten shift: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [late_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the forgotten shift: {body}");
    let (status, body) = admin
        .post(&format!("/api/v1/users/{assistant_id}/archive"), &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "archive the assistant: {body}");

    let next_from = to + Duration::days(1);
    let (_, next_to) = month_bounds(next_from);
    let carried = payroll_report::CarriedDays {
        since: from,
        before: next_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let members =
        payroll_report::payroll_members(&app.state, next_from, next_to, &[], false, Some(&carried))
            .await
            .expect("members of the correcting report");
    assert!(
        members.iter().any(|member| member.id == assistant_id),
        "an archived person still owed a correction belongs to the report"
    );

    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: Some(carried),
        },
        &members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build the correcting report");

    assert_eq!(
        data.late_entry_rows
            .iter()
            .map(|row| (row.date, row.minutes))
            .collect::<Vec<_>>(),
        vec![(worked_day, 120)],
        "the archived person's forgotten shift is still declared"
    );

    app.cleanup().await;
}

/// A stray, unrelated draft entry inside the carry-over window silently
/// defers *every* correction for that person — not just a correction that
/// touches the same day, and not just for as long as the draft is genuinely
/// "a reopen in progress". Nothing here is lost or double-paid, but the
/// deferral has no time bound and raises no notification, so an abandoned
/// draft nobody ever resubmits can suppress a person's corrections for good,
/// invisibly. Documents the actual scope of that trade-off with a concrete
/// case: a completely unrelated month's forgotten draft blocks a correction
/// on a day it has nothing to do with.
#[tokio::test]
async fn payroll_declared_days_unrelated_stray_draft_blocks_every_correction() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _emp_id, _emp_pw, _monday, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "ledger-stray").await;
    let lead = login_change_pw(&app, "lead-ledger-stray@example.com", &lead_pw).await;
    let (assistant_id, assistant_pw) = create_assistant(&admin, lead_id, "ledger-stray").await;
    let assistant = login_change_pw(&app, "aushilfe-ledger-stray@example.com", &assistant_pw).await;

    let worked_day = anchor_monday();
    let worked_iso = worked_day.format("%Y-%m-%d").to_string();
    let (from, to) = month_bounds(worked_day);
    let period = from.format("%Y-%m").to_string();

    // The day whose correction we actually want to observe.
    let first_shift = create_and_submit_entry(&assistant, &worked_iso, cat_id).await;
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [first_shift]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the reported shift: {body}");
    let period_entries = app
        .state
        .db
        .time_entries
        .payroll_entry_snapshot(from, to)
        .await
        .expect("snapshot the reported month");
    app.state
        .db
        .reports
        .record_payroll_report_delivery(
            &period,
            from,
            to,
            &period_entries,
            &[(assistant_id, worked_day, 240)],
            Default::default(),
            &[],
            &[],
            &[],
        )
        .await
        .expect("record the original report");

    // A genuine, forgotten correction on the reported day.
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": worked_iso,
                "start_time": "13:00",
                "end_time": "15:00",
                "category_id": cat_id,
                "comment": "forgotten shift"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create the forgotten shift: {body}");
    let forgotten_id = id(&body);
    let (status, body) = assistant
        .post("/api/v1/time-entries/submit", &json!({"ids": [forgotten_id]}))
        .await;
    assert_eq!(status, StatusCode::OK, "submit the forgotten shift: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [forgotten_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the forgotten shift: {body}");

    // An entirely unrelated draft, on a *different* already-reported day, that
    // nobody ever resubmits. Its own month is a different month than the one
    // being corrected above.
    let stray_day = worked_day - Duration::days(21);
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": stray_day.format("%Y-%m-%d").to_string(),
                "start_time": "08:00",
                "end_time": "09:00",
                "category_id": cat_id,
                "comment": "abandoned draft, never submitted"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create the stray draft: {body}");

    let next_from = to + Duration::days(1);
    let (_, next_to) = month_bounds(next_from);
    // Wide enough to cover both the stray draft's month and the reported day.
    let carried = payroll_report::CarriedDays {
        since: stray_day - Duration::days(31),
        before: next_from,
        owed_periods: Vec::new(),
        reported_as: None,
    };
    let members =
        payroll_report::payroll_members(&app.state, next_from, next_to, &[], false, Some(&carried))
            .await
            .expect("members while the stray draft exists");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: Some(carried.clone()),
        },
        &members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build while the stray draft exists");
    assert!(
        data.late_entry_rows.is_empty(),
        "the genuine, unrelated correction is suppressed by the stray draft: {:?}",
        data.late_entry_rows
            .iter()
            .map(|row| (row.date, row.minutes))
            .collect::<Vec<_>>()
    );

    // Resubmitting and approving the stray draft — completely incidental to
    // the correction we care about — is what unblocks it.
    let stray_entries: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM time_entries WHERE user_id=$1 AND entry_date=$2",
    )
    .bind(assistant_id)
    .bind(stray_day)
    .fetch_all(&app.state.pool)
    .await
    .expect("find the stray entry");
    let (status, body) = assistant
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": stray_entries}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "submit the stray draft: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": stray_entries}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "approve the stray draft: {body}");

    let members =
        payroll_report::payroll_members(&app.state, next_from, next_to, &[], false, Some(&carried))
            .await
            .expect("members after the stray draft settles");
    let data = payroll_report::build_report_data(
        &app.state,
        payroll_report::ReportWindow {
            from: next_from,
            to: next_to,
            interim: false,
            created_on: next_to,
            carried: Some(carried),
        },
        &members,
        &config(true, false),
        &zerf::i18n::Language::from_setting("en"),
        None,
    )
    .await
    .expect("build after the stray draft settles");
    let mut rows: Vec<(NaiveDate, i64)> = data
        .late_entry_rows
        .iter()
        .map(|row| (row.date, row.minutes))
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        vec![(stray_day, 60), (worked_day, 120)],
        "neither correction was lost: the genuine one on worked_day, and the \
         stray draft's own day, now a real booking with nothing declared for \
         it yet, both surface once the block clears"
    );

    app.cleanup().await;
}
