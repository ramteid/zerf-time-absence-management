use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn range_csv_and_category_totals_for_booked_entries() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (lead_id, lead_pw, emp_id, emp_pw, monday, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

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

    // Draft entries are booked time and should appear in category totals.
    let (st, body) = lead
        .get(&format!(
            "/api/v1/reports/categories?user_id={}&from={}&to={}",
            emp_id, monday, monday
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "category report with only draft");
    assert_eq!(body.as_array().unwrap()[0]["minutes"], 240);

    // Submit and approve the entry
    let (st, _) = emp
        .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit entry");
    let (st, _) = lead
        .post(
            &format!("/api/v1/time-entries/{}/approve", entry_id),
            &json!({}),
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
        + chrono::Duration::days(366))
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

    app.cleanup().await;
}

#[tokio::test]
async fn partial_sick_day_counts_worked_time_and_removes_target() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, emp_pw, monday, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

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
            &format!("/api/v1/time-entries/{}/approve", entry_id),
            &json!({}),
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

    app.cleanup().await;
}

#[tokio::test]
async fn reports_exclude_current_day_from_hours_and_categories() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let today = today();

    let (st, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": today,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "today should not report"
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
            &format!("/api/v1/time-entries/{}/approve", entry_id),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve today entry");

    let month = &today[..7];
    let (st, body) = emp
        .get(&format!("/api/v1/reports/month?month={month}"))
        .await;
    assert_eq!(st, StatusCode::OK, "month report");
    assert_eq!(body["actual_min"], 0);
    assert!(body["category_totals"].as_object().unwrap().is_empty());
    let today_row = body["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["date"] == today)
        .unwrap();
    assert!(today_row["entries"].as_array().unwrap().is_empty());

    let (st, body) = emp
        .get(&format!(
            "/api/v1/reports/categories?user_id={}&from={}&to={}",
            emp_id, today, today
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "category report for today");
    assert!(body.as_array().unwrap().is_empty());

    app.cleanup().await;
}

#[tokio::test]
async fn reports_ignore_legacy_time_before_user_start_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
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

    app.cleanup().await;
}
