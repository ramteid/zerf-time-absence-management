use chrono::{Datelike, Duration, NaiveDate, Weekday};
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team, id, login_change_pw};

fn json_f64(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("missing numeric field {key}: {value}"))
}

fn json_i64(value: &Value, key: &str) -> i64 {
    value[key]
        .as_i64()
        .unwrap_or_else(|| panic!("missing integer field {key}: {value}"))
}

async fn update_carryover_expiry(admin: &crate::common::TestClient, expiry_mm_dd: &str) {
    let (st, settings) = admin.get("/api/v1/settings").await;
    assert_eq!(st, StatusCode::OK, "load settings before update");

    let default_weekly_hours = settings["default_weekly_hours"].as_f64().unwrap_or(39.0);
    let default_annual_leave_days = settings["default_annual_leave_days"].as_i64().unwrap_or(30);

    let (st, body) = admin
        .put(
            "/api/v1/settings",
            &json!({
                "ui_language": settings["ui_language"],
                "time_format": settings["time_format"],
                "country": settings["country"],
                "region": settings["region"],
                "default_weekly_hours": default_weekly_hours,
                "default_annual_leave_days": default_annual_leave_days,
                "carryover_expiry_date": expiry_mm_dd,
                "submission_deadline_day": settings["submission_deadline_day"],
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "update carryover expiry date failed for {expiry_mm_dd}: {body}"
    );
}

async fn set_leave_days_current_and_next(
    admin: &crate::common::TestClient,
    user_id: i64,
    current_year_days: i64,
    next_year_days: i64,
) {
    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{user_id}"),
            &json!({
                "leave_days_current_year": current_year_days,
                "leave_days_next_year": next_year_days
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "set current/next leave days failed: {body}"
    );
}

async fn pick_workdays(
    client: &crate::common::TestClient,
    year: i32,
    start_month: u32,
    wanted: usize,
) -> Vec<NaiveDate> {
    let (st, holidays_json) = client.get(&format!("/api/v1/holidays?year={year}")).await;
    assert_eq!(st, StatusCode::OK, "load holidays for year {year}");

    let mut holiday_set = std::collections::HashSet::<NaiveDate>::new();
    for item in holidays_json.as_array().expect("holidays should be an array") {
        let date_str = item["holiday_date"]
            .as_str()
            .expect("holiday_date should be string");
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .expect("holiday_date should be ISO date");
        holiday_set.insert(date);
    }

    let mut out = Vec::with_capacity(wanted);
    let mut cursor = NaiveDate::from_ymd_opt(year, start_month, 1).expect("valid month");
    let year_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();

    while cursor <= year_end && out.len() < wanted {
        let is_weekday = !matches!(cursor.weekday(), Weekday::Sat | Weekday::Sun);
        if is_weekday && !holiday_set.contains(&cursor) {
            out.push(cursor);
        }
        cursor += Duration::days(1);
    }

    assert_eq!(
        out.len(),
        wanted,
        "could not find enough workdays in {year}; got {} expected {wanted}",
        out.len()
    );
    out
}

async fn create_vacation(
    client: &crate::common::TestClient,
    day: NaiveDate,
) -> i64 {
    let date = day.format("%Y-%m-%d").to_string();
    let (st, body) = client
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": date, "end_date": date}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create vacation on {date}");
    id(&body)
}

#[tokio::test]
async fn carryover_policy_edge_cases() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let current_year = chrono::Local::now().year();
    let next_year = current_year + 1;

    // Make yearly entitlement deterministic for this scenario.
    set_leave_days_current_and_next(&admin, emp_id, 6, 10).await;

    // Edge case 1: carryover_expired reflects expiry-date boundary in current year.
    update_carryover_expiry(&admin, "12-31").await;
    let (st, bal_not_expired) = emp
        .get(&format!("/api/v1/leave-balance/{emp_id}?year={current_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        bal_not_expired["carryover_expired"], false,
        "carryover should not be expired with 12-31 cutoff"
    );

    update_carryover_expiry(&admin, "01-01").await;
    let (st, bal_expired) = emp
        .get(&format!("/api/v1/leave-balance/{emp_id}?year={current_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        bal_expired["carryover_expired"], true,
        "carryover should be expired with 01-01 cutoff after Jan 1"
    );

    // Reset to default-like value so subsequent assertions stay intuitive.
    update_carryover_expiry(&admin, "03-31").await;

    // Build previous-year usage for next-year carryover:
    // - 2 approved vacation days (consume carryover source)
    // - 2 requested vacation days (must NOT consume carryover source)
    let current_year_days = pick_workdays(&emp, current_year, 6, 4).await;
    for day in &current_year_days[0..2] {
        let absence_id = create_vacation(&emp, *day).await;
        let (st, _) = lead
            .post(&format!("/api/v1/absences/{absence_id}/approve"), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "approve current-year vacation");
    }
    for day in &current_year_days[2..4] {
        let _ = create_vacation(&emp, *day).await;
    }

    // Edge case 2: next-year carryover derives from approved/cancellation_pending only.
    let (st, bal_next_year_initial) = emp
        .get(&format!("/api/v1/leave-balance/{emp_id}?year={next_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        json_i64(&bal_next_year_initial, "annual_entitlement"),
        10,
        "next-year entitlement should match explicit leave-days setting"
    );
    assert_eq!(
        json_i64(&bal_next_year_initial, "carryover_days"),
        4,
        "carryover should be 6 - 2 approved days = 4; requested days do not reduce carryover source"
    );
    assert_eq!(
        json_f64(&bal_next_year_initial, "available"),
        14.0,
        "available should equal entitlement + carryover when no next-year absences exist"
    );

    // Prepare one approved next-year vacation day.
    let next_year_day = pick_workdays(&emp, next_year, 2, 1).await[0];
    let next_year_absence_id = create_vacation(&emp, next_year_day).await;
    let (st, _) = lead
        .post(
            &format!("/api/v1/absences/{next_year_absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve next-year vacation");

    let (st, bal_after_approved) = emp
        .get(&format!("/api/v1/leave-balance/{emp_id}?year={next_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);

    // Edge case 3: before next year starts, approved upcoming days do not consume
    // carryover_remaining yet (only already taken approved days do).
    assert_eq!(
        json_f64(&bal_after_approved, "carryover_remaining"),
        json_i64(&bal_after_approved, "carryover_days") as f64,
        "carryover remaining should stay full before any approved days are actually taken"
    );

    // Edge case 4: cancellation_pending stays budget-reserved while moving from
    // approved_upcoming to requested.
    let approved_before = json_f64(&bal_after_approved, "approved_upcoming");
    let requested_before = json_f64(&bal_after_approved, "requested");
    let available_before = json_f64(&bal_after_approved, "available");

    let (st, body) = emp
        .delete(&format!("/api/v1/absences/{next_year_absence_id}"))
        .await;
    assert_eq!(st, StatusCode::OK, "request cancellation for next-year vacation");
    assert_eq!(body["pending"], true, "must enter cancellation workflow");

    let (st, bal_cancellation_pending) = emp
        .get(&format!("/api/v1/leave-balance/{emp_id}?year={next_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);

    assert_eq!(
        json_f64(&bal_cancellation_pending, "approved_upcoming"),
        approved_before - 1.0,
        "approved_upcoming must drop by the cancelled day"
    );
    assert_eq!(
        json_f64(&bal_cancellation_pending, "requested"),
        requested_before + 1.0,
        "requested must include cancellation_pending day"
    );
    assert_eq!(
        json_f64(&bal_cancellation_pending, "available"),
        available_before,
        "available must stay unchanged while cancellation is pending"
    );

    // Edge case 5: post-expiry vacation days must be covered by current-year
    // entitlement only; carryover can be used only up to the expiry date.
    // Here: next-year entitlement=2, carryover=4 from current year => November
    // bookings in next year may reserve at most 2 days.
    set_leave_days_current_and_next(&admin, emp_id, 6, 2).await;

    let (st, bal_next_year_small_entitlement) = emp
        .get(&format!("/api/v1/leave-balance/{emp_id}?year={next_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        json_i64(&bal_next_year_small_entitlement, "annual_entitlement"),
        2,
        "next-year entitlement should be overridden to 2"
    );
    assert_eq!(
        json_i64(&bal_next_year_small_entitlement, "carryover_days"),
        4,
        "carryover remains 4 from current-year entitlement 6 minus 2 approved"
    );

    let nov_workdays = pick_workdays(&emp, next_year, 11, 3).await;
    for day in &nov_workdays[0..2] {
        let _ = create_vacation(&emp, *day).await;
    }
    let day3 = nov_workdays[2].format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": day3, "end_date": day3}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "third post-expiry day should be rejected; only annual entitlement is usable after expiry"
    );
    assert!(
        body.to_string().contains("Not enough remaining vacation days"),
        "error should mention remaining vacation days: {body}"
    );

    app.cleanup().await;
}
