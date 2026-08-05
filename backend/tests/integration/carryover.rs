use chrono::{Datelike, Duration, NaiveDate, Weekday};
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team, id, login_change_pw, reference_date, temp_pw};

fn json_f64(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .or_else(|| value[key].as_i64().map(|number| number as f64))
        .unwrap_or_else(|| panic!("missing numeric field {key}: {value}"))
}

fn json_i64(value: &Value, key: &str) -> i64 {
    value[key]
        .as_i64()
        .unwrap_or_else(|| panic!("missing integer field {key}: {value}"))
}

fn leave_account_values(
    category_id: i64,
    base_days: i64,
    current_year_days: i64,
    next_year_days: i64,
) -> Value {
    json!([{
        "category_id": category_id,
        "base_days": base_days,
        "current_year_days": current_year_days,
        "next_year_days": next_year_days,
    }])
}

async fn vacation_leave_account_category_id(admin: &crate::common::TestClient) -> i64 {
    let (st, categories) = admin.get("/api/v1/absence-categories/all").await;
    assert_eq!(st, StatusCode::OK, "load absence categories");
    categories
        .as_array()
        .and_then(|items| items.iter().find(|category| category["slug"] == "vacation"))
        .and_then(|category| category["id"].as_i64())
        .expect("canonical vacation leave-account category")
}

async fn update_carryover_expiry(
    admin: &crate::common::TestClient,
    category_id: i64,
    expiry_mm_dd: &str,
) {
    let (st, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{category_id}"),
            &json!({"leave_account_carryover_expiry": expiry_mm_dd}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "update leave-account carryover expiry failed for {expiry_mm_dd}: {body}"
    );
}

async fn set_vacation_account_days_current_and_next(
    admin: &crate::common::TestClient,
    user_id: i64,
    category_id: i64,
    current_year_days: i64,
    next_year_days: i64,
) {
    let (st, accounts) = admin
        .get(&format!("/api/v1/users/{user_id}/leave-accounts"))
        .await;
    assert_eq!(st, StatusCode::OK, "load user leave accounts before update");
    let base_days = accounts
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|account| account["category_id"].as_i64() == Some(category_id))
        })
        .and_then(|account| account["base_days"].as_i64())
        .expect("user has canonical vacation leave account");

    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{user_id}"),
            &json!({
                "leave_accounts": [{
                    "category_id": category_id,
                    "base_days": base_days,
                    "current_year_days": current_year_days,
                    "next_year_days": next_year_days
                }]
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "set current/next leave-account days failed: {body}"
    );
}

async fn set_vacation_account_days_for_year(
    app: &TestApp,
    user_id: i64,
    category_id: i64,
    year: i32,
    days: i64,
) {
    sqlx::query(
        "INSERT INTO user_leave_account_year_overrides(user_id, category_id, year, days) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, category_id, year) DO UPDATE SET days = EXCLUDED.days",
    )
    .bind(user_id)
    .bind(category_id)
    .bind(year)
    .bind(days)
    .execute(&app.state.pool)
    .await
    .expect("set leave-account year override");
}

async fn leave_account_balance(
    client: &crate::common::TestClient,
    user_id: i64,
    category_id: i64,
    year: i32,
) -> Value {
    let (st, balances) = client
        .get(&format!("/api/v1/leave-balances/{user_id}?year={year}"))
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "load leave-account balances: {balances}"
    );
    balances
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|balance| balance["category_id"].as_i64() == Some(category_id))
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!("leave-account balance for category {category_id} was missing from {balances}")
        })
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
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;

    // Make the canonical leave-account entitlement deterministic for this scenario.
    set_vacation_account_days_current_and_next(&admin, emp_id, vacation_account_id, 6, 10).await;

    // Edge case 1: carryover_expired reflects expiry-date boundary in current year.
    update_carryover_expiry(&admin, vacation_account_id, "12-31").await;
    let bal_not_expired =
        leave_account_balance(&emp, emp_id, vacation_account_id, current_year).await;
    assert_eq!(
        bal_not_expired["carryover_expired"], false,
        "carryover should not be expired with 12-31 cutoff"
    );

    update_carryover_expiry(&admin, vacation_account_id, "01-01").await;
    let bal_expired = leave_account_balance(&emp, emp_id, vacation_account_id, current_year).await;
    assert_eq!(
        bal_expired["carryover_expired"], true,
        "carryover should be expired with 01-01 cutoff after Jan 1"
    );

    // Reset to default-like value so subsequent assertions stay intuitive.
    update_carryover_expiry(&admin, vacation_account_id, "03-31").await;

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

    // Edge case 2: next-year carryover uses pessimistic sourcing — requested and
    // cancellation_pending absences are counted as consumed in the source year,
    // not just approved ones. This prevents cross-year double-grants where a
    // pending December request reserves December's budget while simultaneously
    // leaving the carryover source untouched, allowing both it and an early-next-
    // year booking to be approved and together exceed the entitlement.
    let bal_next_year_initial =
        leave_account_balance(&emp, emp_id, vacation_account_id, next_year).await;
    assert_eq!(
        json_i64(&bal_next_year_initial, "annual_entitlement"),
        10,
        "next-year entitlement should match explicit leave-days setting"
    );
    assert_eq!(
        json_i64(&bal_next_year_initial, "carryover_days"),
        2,
        "carryover should be 6 - 2 approved - 2 requested = 2; \
         requested absences pessimistically reduce the carryover source \
         to prevent cross-year double-grants"
    );
    assert_eq!(
        json_f64(&bal_next_year_initial, "available"),
        12.0,
        "available should equal entitlement(10) + carryover(2) when no next-year absences exist"
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

    let bal_after_approved =
        leave_account_balance(&emp, emp_id, vacation_account_id, next_year).await;

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

    let bal_cancellation_pending =
        leave_account_balance(&emp, emp_id, vacation_account_id, next_year).await;

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

    // Edge case 5: post-expiry leave-account days must be covered by current-year
    // entitlement only; carryover can be used only up to the expiry date.
    // Here: next-year entitlement=2, carryover=2 (pessimistic: 6 - 2 approved - 2 requested)
    // => November bookings in next year may reserve at most 2 days (the annual entitlement).
    set_vacation_account_days_current_and_next(&admin, emp_id, vacation_account_id, 6, 2).await;

    let bal_next_year_small_entitlement =
        leave_account_balance(&emp, emp_id, vacation_account_id, next_year).await;
    assert_eq!(
        json_i64(&bal_next_year_small_entitlement, "annual_entitlement"),
        2,
        "next-year entitlement should be overridden to 2"
    );
    assert_eq!(
        json_i64(&bal_next_year_small_entitlement, "carryover_days"),
        2,
        "carryover is 2 from current-year entitlement 6 minus 2 approved minus 2 requested (pessimistic sourcing)"
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
        "third post-expiry day should be rejected; only annual account entitlement is usable after expiry"
    );
    assert!(
        body.to_string()
            .contains("Not enough remaining leave-account days"),
        "error should mention remaining leave-account days: {body}"
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
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;

    update_carryover_expiry(&admin, vacation_account_id, "03-31").await;
    // Build a large carryover into next year while keeping next-year entitlement tiny.
    set_vacation_account_days_current_and_next(&admin, emp_id, vacation_account_id, 90, 2).await;

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
            .contains("Not enough remaining leave-account days"),
        "error should mention remaining leave-account days: {body}"
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
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;

    // Ensure carryover exists for current year and expiry is already in the past.
    update_carryover_expiry(&admin, vacation_account_id, "01-31").await;
    set_vacation_account_days_for_year(&app, emp_id, vacation_account_id, prev_year, 2).await;
    set_vacation_account_days_current_and_next(&admin, emp_id, vacation_account_id, 1, 1).await;

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
async fn requested_days_reduce_cross_year_carryover_source() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let current_year = reference_date().year();
    let next_year = current_year + 1;
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;

    update_carryover_expiry(&admin, vacation_account_id, "03-31").await;
    // With next-year base entitlement set to 0, January days in next year must be
    // funded by carryover from current year.
    set_vacation_account_days_current_and_next(&admin, emp_id, vacation_account_id, 2, 0).await;

    // Consume one current-year day as requested only. It must reserve availability
    // and reduce the carryover source for next year; otherwise a pending
    // current-year request could grant carryover that is already reserved.
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

    // Request an absence crossing into January next year. The current-year part
    // consumes the last remaining current-year day, so no carryover is left to
    // fund the next-year day when next-year entitlement is zero.
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
        StatusCode::BAD_REQUEST,
        "cross-year request must be rejected when requested current-year days consume the carryover source: {body}"
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
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;
    update_carryover_expiry(&admin, vacation_account_id, "02-29").await;

    let balance = leave_account_balance(&emp, emp_id, vacation_account_id, current_year).await;

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

/// Covers the scenario `hire_date` exists for: onboarding an employee who
/// already worked the full year before adopting Zerf mid-year. Their Zerf
/// `start_date` is necessarily mid-year (that's when they started using the
/// system), which would normally pro-rate their entitlement — but their real
/// employment started long before, so they are owed the full entitlement.
/// Setting `hire_date` to their real employment start must yield the full
/// entitlement; clearing it again must fall back to `start_date`-based
/// proration (the pre-existing, still-correct behavior for everyone else).
#[tokio::test]
async fn hire_date_anchors_proration_independent_of_start_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let current_year = reference_date().year();
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;
    // Mid-year Zerf start date: by itself this would halve the entitlement
    // (6 of 12 months remaining from July onward).
    let start_date = format!("{current_year}-07-01");
    // Real employment start, years before the queried year — well outside it,
    // so it must produce the FULL entitlement rather than one pro-rated from
    // the (later) Zerf start_date.
    let hire_date = format!("{}-01-01", current_year - 5);

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "midyear-hire@example.com", "first_name": "Mira", "last_name": "Midyear",
                "role": "employee", "weekly_hours": 39,
                "leave_accounts": leave_account_values(vacation_account_id, 30, 30, 30),
                "start_date": start_date, "hire_date": hire_date,
                "approver_ids": [1],
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user with hire_date: {body}");
    let user_id = id(&body);

    // -- hire_date predates the queried year entirely: full entitlement, no proration --
    let balance = leave_account_balance(&admin, user_id, vacation_account_id, current_year).await;
    assert_eq!(
        json_i64(&balance, "annual_entitlement"),
        30,
        "hire_date predating the year should yield the full entitlement, not one pro-rated from start_date: {balance}"
    );

    // -- Clearing hire_date (explicit null, double-Option PATCH semantics) reverts to start_date-based proration --
    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{user_id}"),
            &json!({"hire_date": null}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "clear hire_date: {body}");
    assert!(
        body["hire_date"].is_null(),
        "cleared hire_date should be null in the response: {body}"
    );

    let balance = leave_account_balance(&admin, user_id, vacation_account_id, current_year).await;
    // start_date = July 1st → 6 of 12 months remaining → ceil(30 * 6 / 12) = 15
    assert_eq!(
        json_i64(&balance, "annual_entitlement"),
        15,
        "clearing hire_date should resume proration anchored on start_date: {balance}"
    );

    app.cleanup().await;
}

/// Regression test: `carryover_days_into_year` must iterate source years from
/// `start_date` onward, never from `leave_entitlement_anchor` (which may
/// resolve to `hire_date`, years before `start_date`). Zerf has no usage data
/// for the "phantom" years between `hire_date` and `start_date` — looping over
/// them would fabricate a full default entitlement with zero recorded usage for
/// each one, wildly inflating `incoming_carryover` and then swallowing the
/// user's *real* usage in their start_date year (`max(0, real_usage -
/// inflated_carryover) == 0`). `hire_date` must still anchor the entitlement
/// *within* the iterated years (so the start_date year correctly receives its
/// full, non-prorated entitlement) — only the loop's *range* must stay bounded
/// to years Zerf actually has data for.
#[tokio::test]
async fn hire_date_does_not_inflate_carryover_with_phantom_pre_start_date_years() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let current_year = reference_date().year();
    let next_year = current_year + 1;
    let vacation_account_id = vacation_leave_account_category_id(&admin).await;
    // Mid-year Zerf go-live: carryover into `next_year` must be derived starting
    // from this year (the only one Zerf has usage data for)...
    let start_date = format!("{current_year}-07-01");
    // ...not from here. Five "phantom" years with no recorded usage at all sit
    // between `hire_date` and `start_date` -- exactly what must NOT be iterated.
    let hire_date = format!("{}-01-01", current_year - 5);

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "phantom-years@example.com", "first_name": "Penny", "last_name": "Phantom",
                "role": "employee", "weekly_hours": 39,
                "leave_accounts": leave_account_values(vacation_account_id, 12, 12, 12),
                "start_date": start_date, "hire_date": hire_date,
                "approver_ids": [1],
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user with hire_date: {body}");
    let user_id = id(&body);
    let user = login_change_pw(&app, "phantom-years@example.com", &temp_pw(&body)).await;

    // A plain year-end cutoff keeps the carryover formula easy to hand-verify:
    // the whole approved-usage window falls before the expiry date.
    update_carryover_expiry(&admin, vacation_account_id, "12-31").await;

    // 4 approved vacation days, safely within the start_date year and after
    // start_date itself (August, vs. a July 1st start).
    let usage_days = pick_workdays(&user, current_year, 8, 4).await;
    for day in &usage_days {
        let absence_id = create_vacation(&user, *day).await;
        let (st, _) = admin
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve vacation on {day}");
    }

    let balance = leave_account_balance(&admin, user_id, vacation_account_id, next_year).await;

    // Correct: only the start_date year is iterated -- entitlement 12 minus the
    // 4 approved usage days = 8.
    //
    // Under the bug, the loop would also walk the 5 phantom years between
    // hire_date and start_date. Each one fabricates a full default entitlement
    // (e.g. 30) against zero recorded usage, so `incoming_carryover` reaches 30
    // before the start_date year is even considered. There, `base_usage` becomes
    // `max(0, real_usage(4) - incoming_carryover(30)) == 0` -- the real usage is
    // swallowed completely -- yielding an inflated `max(0, 12 - 0) == 12`.
    assert_eq!(
        json_i64(&balance, "carryover_days"),
        8,
        "carryover must derive only from the start_date year (entitlement 12 - 4 used = 8), \
         not be inflated by phantom pre-start_date years anchored on hire_date: {balance}"
    );

    app.cleanup().await;
}
