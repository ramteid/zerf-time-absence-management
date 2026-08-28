//! "AU" (medical certificate) threshold feature: category flag round-trip,
//! the live preview endpoint, and the payroll report's AU column — verified
//! end to end against a real database, not just the pure chain-building
//! logic covered by the unit tests in `services::medical_certificate`.

use chrono::Duration;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;
use zerf::services::payroll_report;

#[tokio::test]
async fn sick_category_is_seeded_as_medical_certificate_relevant() {
    let app = TestApp::spawn().await;
    let sick = absence_cat(&app.state.pool, "sick").await;
    assert!(
        sick.medical_certificate_relevant,
        "migration 042 should mark the seeded 'sick' category relevant"
    );
    app.cleanup().await;
}

#[tokio::test]
async fn medical_certificate_category_flag_round_trips_through_the_api() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/absence-categories",
            &json!({
                "name": "Custom Sick",
                "color": "#ff0000",
                "cost_type": "none",
                "auto_approve_past": true,
                "medical_certificate_relevant": true,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create category: {body}");
    assert_eq!(body["medical_certificate_relevant"], json!(true));
    let cat_id = id(&body);

    let (st, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{cat_id}"),
            &json!({"medical_certificate_relevant": false}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "update category: {body}");
    assert_eq!(body["medical_certificate_relevant"], json!(false));

    app.cleanup().await;
}

/// The scenario from the feature request: three separate one-day sick
/// requests on consecutive workdays must be treated as one continuous
/// three-day illness period once all three exist, while a fourth, isolated
/// sick day elsewhere in the same month must not be swept into that chain.
///
/// The connected days also have to arrive as a single row, so the period the
/// reader sees is the period the verdict was computed over.
#[tokio::test]
async fn payroll_report_marks_absences_required_once_a_connected_chain_crosses_the_threshold() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "au-chain").await;

    // Threshold = 3 consecutive calendar days.
    {
        let mut tx = app.state.db.settings.begin().await.expect("begin tx");
        zerf::services::settings::save_setting_tx(
            &mut tx,
            "medical_certificate_threshold_days",
            "3",
        )
        .await
        .expect("save threshold");
        tx.commit().await.expect("commit threshold");
    }

    let monday = anchor_monday_for_medical_certificate();
    let tuesday = monday + Duration::days(1);
    let wednesday = monday + Duration::days(2);
    // Thursday (monday + 3) is deliberately left uncovered so it breaks the
    // chain: Friday's sick day must stand on its own.
    let friday = monday + Duration::days(4);

    let sick = absence_cat(&app.state.pool, "sick").await;
    assert!(sick.medical_certificate_relevant);

    for day in [monday, tuesday, wednesday, friday] {
        app.state
            .db
            .absences
            .create(emp_id, sick.id, true, day, day, None, "approved")
            .await
            .unwrap_or_else(|e| panic!("create sick absence for {day}: {e}"));
    }

    let (from, to) = month_bounds_for_medical_certificate(monday);
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
        &payroll_report_config(),
        &language,
        None,
    )
    .await
    .expect("build report data");

    // One row per continuous illness, not per sick note: the three connected
    // days were filed separately but are one illness, and the certificate
    // verdict was computed over exactly that period. Printing them apart is
    // what made a short row carrying a "required" verdict look wrong.
    let rows = data.absence_rows.as_ref().expect("absence section enabled");
    assert_eq!(
        rows.len(),
        2,
        "the connected days are one row, the isolated day another: {}",
        row_summaries(rows)
    );

    assert_eq!(rows[0].from, monday, "the chain row starts on Monday");
    assert_eq!(
        rows[0].to, wednesday,
        "and runs to Wednesday, the span the verdict was judged on"
    );
    assert_eq!(rows[0].days, 3.0, "carrying the whole chain's days");
    assert_eq!(
        rows[0].medical_certificate_required,
        Some(true),
        "3 connected days reaches the threshold"
    );
    // Tuesday is inside that row rather than being one of its own.
    assert!(
        !rows.iter().any(|row| row.from == tuesday),
        "no separate row starts mid-chain: {}",
        row_summaries(rows)
    );

    assert_eq!(
        rows[1].from, friday,
        "Thursday's real gap breaks the chain, so Friday stands alone"
    );
    assert_eq!(rows[1].to, friday);
    assert_eq!(
        rows[1].medical_certificate_required,
        Some(false),
        "one day on its own stays below the threshold"
    );

    app.cleanup().await;
}

fn row_summaries(rows: &[zerf::report_pdf::PayrollAbsenceRow]) -> String {
    rows.iter()
        .map(|r| format!("{}..{}={:?}", r.from, r.to, r.medical_certificate_required))
        .collect::<Vec<_>>()
        .join(", ")
}

fn payroll_report_config() -> payroll_report::PayrollReportConfig {
    payroll_report::PayrollReportConfig {
        enabled: true,
        recipients: vec!["payroll@example.com".into()],
        day_of_month: 1,
        include_assistant_hours: false,
        include_employee_hours: false,
        excluded_user_ids: Vec::new(),
    }
}

fn anchor_monday_for_medical_certificate() -> chrono::NaiveDate {
    use chrono::Datelike;
    for weeks_back in [3, 4, 2] {
        let monday = next_monday(-7 * weeks_back);
        if (monday + Duration::days(4)).month() == monday.month() {
            return monday;
        }
    }
    panic!("no anchor monday with a Mon-Fri block inside one month");
}

fn month_bounds_for_medical_certificate(
    date: chrono::NaiveDate,
) -> (chrono::NaiveDate, chrono::NaiveDate) {
    use chrono::Datelike;
    let from = chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
    let last_day = zerf::time_calc::last_day_of_month(date.year(), date.month());
    (
        from,
        chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), last_day).unwrap(),
    )
}

/// Live preview endpoint, exercised over real HTTP: creating real absences and
/// checking the running chain length and AU verdict before and after each new
/// request, plus the `exclude_absence_id` path used while editing.
#[tokio::test]
async fn medical_certificate_preview_endpoint_tracks_the_chain_over_http() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "au-preview").await;
    let emp = login_change_pw(&app, "emp-au-preview@example.com", &emp_pw).await;

    // Threshold = 4 (the default), set explicitly so the test does not
    // silently depend on whatever the default happens to be.
    let (st, _) = admin
        .put(
            "/api/v1/settings",
            &json!({
                "ui_language": "en",
                "time_format": "24h",
                "country": "DE",
                "region": "",
                "medical_certificate_threshold_days": 4
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "set threshold");

    let sick = absence_cat(&app.state.pool, "sick").await;
    let day1 = next_monday(-14);
    let day2 = day1 + Duration::days(1);
    let day3 = day1 + Duration::days(2);
    let day4 = day1 + Duration::days(3);

    // Before anything exists: previewing a single day is a 1-day chain, not required.
    let (st, body) = emp
        .get(&format!(
            "/api/v1/absences/medical-certificate-preview?category_id={}&start_date={}&end_date={}",
            sick.id, day1, day1
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "preview: {body}");
    assert_eq!(body["relevant"], json!(true));
    assert_eq!(body["chain_days"], json!(1));
    assert_eq!(body["required"], json!(false));
    assert_eq!(body["threshold_days"], json!(4));

    // Create days 1-2 for real (auto-approved, in the past).
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"category_id": sick.id, "start_date": day1, "end_date": day2}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create day1-2: {body}");
    assert_eq!(body["status"], "approved");
    let absence_1_2_id = id(&body);

    // Previewing day3 alone must see the existing 2-day range and report a
    // combined 3-day chain (not yet required at threshold 4).
    let (st, body) = emp
        .get(&format!(
            "/api/v1/absences/medical-certificate-preview?category_id={}&start_date={}&end_date={}",
            sick.id, day3, day3
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "preview day3: {body}");
    assert_eq!(body["chain_days"], json!(3), "day1-2 plus day3 = 3 days");
    assert_eq!(body["required"], json!(false), "3 < threshold 4");

    // Actually create day3 too, then day4 crosses the threshold.
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"category_id": sick.id, "start_date": day3, "end_date": day3}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create day3: {body}");

    let (st, body) = emp
        .get(&format!(
            "/api/v1/absences/medical-certificate-preview?category_id={}&start_date={}&end_date={}",
            sick.id, day4, day4
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "preview day4: {body}");
    assert_eq!(body["chain_days"], json!(4));
    assert_eq!(
        body["required"],
        json!(true),
        "4 >= threshold 4, certificate required: {body}"
    );

    // exclude_absence_id must stop the day1-2 absence from being double
    // counted against a hypothetical replacement covering the same days.
    let (st, body) = emp
        .get(&format!(
            "/api/v1/absences/medical-certificate-preview?category_id={}&start_date={}&end_date={}&exclude_absence_id={}",
            sick.id, day1, day2, absence_1_2_id
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "preview with exclude: {body}");
    // Without exclusion this would double the day1-2 span; with it, the
    // hypothetical day1-2 range merges with the real day3 absence into the
    // same 3-day chain as before, not more.
    assert_eq!(
        body["chain_days"],
        json!(3),
        "excluding its own prior self must not double-count day1-2: {body}"
    );

    app.cleanup().await;
}
