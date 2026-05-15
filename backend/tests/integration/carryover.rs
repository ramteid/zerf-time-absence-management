use chrono::{Datelike, Duration, NaiveDate, Weekday};
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team, id, login_change_pw, reference_date};

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

async fn set_leave_days_for_year(app: &TestApp, user_id: i64, year: i32, days: i64) {
    sqlx::query(
        "INSERT INTO user_annual_leave(user_id, year, days) VALUES ($1,$2,$3) \
         ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days",
    )
    .bind(user_id)
    .bind(year)
    .bind(days)
    .execute(&app.state.pool)
    .await
    .expect("set annual leave row");
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
    for item in holidays_json
        .as_array()
        .expect("holidays should be an array")
    {
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

async fn holiday_set_for_year(
    client: &crate::common::TestClient,
    year: i32,
) -> std::collections::HashSet<NaiveDate> {
    let (st, holidays_json) = client.get(&format!("/api/v1/holidays?year={year}")).await;
    assert_eq!(st, StatusCode::OK, "load holidays for year {year}");

    holidays_json
        .as_array()
        .expect("holidays should be an array")
        .iter()
        .map(|item| {
            NaiveDate::parse_from_str(
                item["holiday_date"]
                    .as_str()
                    .expect("holiday_date should be string"),
                "%Y-%m-%d",
            )
            .expect("holiday_date should be ISO date")
        })
        .collect()
}

fn is_workday(date: NaiveDate, holiday_set: &std::collections::HashSet<NaiveDate>) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && !holiday_set.contains(&date)
}

async fn last_workday_in_year(client: &crate::common::TestClient, year: i32) -> NaiveDate {
    let holiday_set = holiday_set_for_year(client, year).await;
    let mut cursor = NaiveDate::from_ymd_opt(year, 12, 31).expect("valid year-end date");
    while cursor.year() == year {
        if is_workday(cursor, &holiday_set) {
            return cursor;
        }
        cursor -= Duration::days(1);
    }
    panic!("could not find a workday in year {year}");
}

async fn nth_workday_from(
    client: &crate::common::TestClient,
    start_inclusive: NaiveDate,
    n: usize,
) -> NaiveDate {
    assert!(n > 0, "n must be >= 1");
    let mut current_year = start_inclusive.year();
    let mut holiday_set = holiday_set_for_year(client, current_year).await;
    let mut cursor = start_inclusive;
    let mut seen = 0usize;
    loop {
        if cursor.year() != current_year {
            current_year = cursor.year();
            holiday_set = holiday_set_for_year(client, current_year).await;
        }
        if is_workday(cursor, &holiday_set) {
            seen += 1;
            if seen == n {
                return cursor;
            }
        }
        cursor += Duration::days(1);
    }
}

async fn create_vacation(client: &crate::common::TestClient, day: NaiveDate) -> i64 {
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

    let current_year = reference_date().year();
    let next_year = current_year + 1;

    // Make yearly entitlement deterministic for this scenario.
    set_leave_days_current_and_next(&admin, emp_id, 6, 10).await;

    // Edge case 1: carryover_expired reflects expiry-date boundary in current year.
    update_carryover_expiry(&admin, "12-31").await;
    let (st, bal_not_expired) = emp
        .get(&format!(
            "/api/v1/leave-balance/{emp_id}?year={current_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        bal_not_expired["carryover_expired"], false,
        "carryover should not be expired with 12-31 cutoff"
    );

    update_carryover_expiry(&admin, "01-01").await;
    let (st, bal_expired) = emp
        .get(&format!(
            "/api/v1/leave-balance/{emp_id}?year={current_year}"
        ))
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
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
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
    assert_eq!(
        st,
        StatusCode::OK,
        "request cancellation for next-year vacation"
    );
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
        body.to_string()
            .contains("Not enough remaining vacation days"),
        "error should mention remaining vacation days: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn cross_year_request_enforces_end_year_post_expiry_budget() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let current_year = reference_date().year();
    let next_year = current_year + 1;

    update_carryover_expiry(&admin, "03-31").await;
    // Build a large carryover into next year while keeping next-year entitlement tiny.
    set_leave_days_current_and_next(&admin, emp_id, 90, 2).await;

    let start = last_workday_in_year(&emp, current_year).await;
    let end = nth_workday_from(
        &emp,
        NaiveDate::from_ymd_opt(next_year, 4, 1).expect("valid date"),
        3,
    )
    .await;

    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "vacation",
                "start_date": start.format("%Y-%m-%d").to_string(),
                "end_date": end.format("%Y-%m-%d").to_string()
            }),
        )
        .await;

    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "cross-year request should be rejected when post-expiry part exceeds end-year base entitlement"
    );
    assert!(
        body.to_string()
            .contains("Not enough remaining vacation days"),
        "error should mention remaining vacation days: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn pre_expiry_days_can_be_requested_after_expiry() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let current_year = reference_date().year();
    let prev_year = current_year - 1;

    // Ensure carryover exists for current year and expiry is already in the past.
    update_carryover_expiry(&admin, "01-31").await;
    set_leave_days_for_year(&app, emp_id, prev_year, 2).await;
    set_leave_days_current_and_next(&admin, emp_id, 1, 1).await;

    let january_workdays = pick_workdays(&emp, current_year, 1, 2).await;
    for day in january_workdays {
        let iso = day.format("%Y-%m-%d").to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": iso, "end_date": iso}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "pre-expiry day should remain bookable after expiry date: {body}"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn requested_days_do_not_reduce_cross_year_carryover_source() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let current_year = reference_date().year();
    let next_year = current_year + 1;

    update_carryover_expiry(&admin, "03-31").await;
    // With next-year base entitlement set to 0, January days in next year must be
    // funded by carryover from current year.
    set_leave_days_current_and_next(&admin, emp_id, 2, 0).await;

    // Consume one current-year day as requested only; this must reserve availability
    // but must not reduce the carryover source for next year.
    let current_year_requested_day = pick_workdays(&emp, current_year, 6, 1).await[0];
    let day_iso = current_year_requested_day.format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": day_iso, "end_date": day_iso}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "create requested current-year day: {body}"
    );

    // Request an absence crossing into January next year. This should stay valid even
    // with a requested day in current year, because requested status must not reduce
    // the carryover source used for next-year validation.
    let cross_start = last_workday_in_year(&emp, current_year).await;
    let cross_end = nth_workday_from(
        &emp,
        NaiveDate::from_ymd_opt(next_year, 1, 1).expect("valid date"),
        1,
    )
    .await;
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({
                "kind":"vacation",
                "start_date": cross_start.format("%Y-%m-%d").to_string(),
                "end_date": cross_end.format("%Y-%m-%d").to_string()
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "cross-year request should stay allowed when only requested days exist in current year: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn carryover_expiry_allows_leap_day_and_normalizes_non_leap_years() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let current_year = reference_date().year();
    update_carryover_expiry(&admin, "02-29").await;

    let (st, balance) = emp
        .get(&format!(
            "/api/v1/leave-balance/{emp_id}?year={current_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "load leave balance with 02-29 expiry");

    let expected_expiry = if NaiveDate::from_ymd_opt(current_year, 2, 29).is_some() {
        format!("{current_year:04}-02-29")
    } else {
        format!("{current_year:04}-02-28")
    };
    assert_eq!(
        balance["carryover_expiry"], expected_expiry,
        "carryover expiry should be year-aware"
    );

    app.cleanup().await;
}
