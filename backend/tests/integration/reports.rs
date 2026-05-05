use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn range_csv_and_category_totals_include_drafts() {
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
    let _entry_id = id(&body);

    let (st, body) = lead
        .get(&format!(
            "/api/v1/reports/categories?user_id={}&from={}&to={}",
            emp_id, monday, monday
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "category report with draft");
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
async fn partial_sick_day_preserves_actual_minutes() {
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
    assert_eq!(day["actual_min"], 240);

    let (st, body) = emp
        .get(&format!(
            "/api/v1/reports/flextime?from={}&to={}",
            monday, monday
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime report");
    assert_eq!(body.as_array().unwrap()[0]["actual_min"], 240);

    app.cleanup().await;
}
