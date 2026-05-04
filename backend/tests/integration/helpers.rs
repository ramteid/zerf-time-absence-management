use chrono::Datelike;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::{TestApp, TestClient};

pub fn id(body: &Value) -> i64 {
    body["id"].as_i64().expect("missing 'id' in response")
}

pub fn temp_pw(body: &Value) -> String {
    body["temporary_password"]
        .as_str()
        .expect("missing temporary_password")
        .to_string()
}

/// Count elements in a JSON array.
pub fn count_ids(body: &Value) -> usize {
    match body {
        Value::Array(arr) => arr.len(),
        _ => 0,
    }
}

/// Check whether a JSON array contains an object with `"id": val`.
pub fn has_id(body: &Value, val: i64) -> bool {
    body.as_array()
        .map(|arr| arr.iter().any(|o| o["id"].as_i64() == Some(val)))
        .unwrap_or(false)
}

/// Find an object in a JSON array by id.
pub fn find_by_id(body: &Value, val: i64) -> Option<&Value> {
    body.as_array()
        .and_then(|arr| arr.iter().find(|o| o["id"].as_i64() == Some(val)))
}

/// Next Monday ≥ offset days from now.
pub fn next_monday(offset_days: i64) -> chrono::NaiveDate {
    let start = chrono::Utc::now().date_naive() + chrono::Duration::days(offset_days);
    let weekday = start.weekday().num_days_from_monday(); // 0=Mon
    if weekday == 0 {
        start
    } else {
        start + chrono::Duration::days((7 - weekday) as i64)
    }
}

pub fn today() -> String {
    chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

pub fn date_offset(days: i64) -> String {
    (chrono::Utc::now().date_naive() + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

pub fn year() -> i32 {
    chrono::Utc::now().date_naive().year()
}

/// Bootstrap admin (id 1, AdminPass!234), one lead, one employee.
/// Returns (lead_id, lead_pw, emp_id, emp_pw, monday_iso, cat_id).
pub async fn bootstrap_team(
    _app: &TestApp,
    admin: &TestClient,
    emp_policy_auto: bool,
) -> (i64, String, i64, String, String, i64) {
    let (_, body) = admin.get("/api/v1/categories").await;
    let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"lead-r@example.com","first_name":"Lara","last_name":"Lead",
                "role":"team_lead","weekly_hours":39,"annual_leave_days":30,
                "start_date":"2024-01-01"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create lead");
    let lead_id = id(&body);
    let lead_pw = temp_pw(&body);

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"emp-r@example.com","first_name":"Emil","last_name":"Emp",
                "role":"employee","weekly_hours":39,"annual_leave_days":30,
                "start_date":"2024-01-01","approver_id": lead_id}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create emp");
    let emp_id = id(&body);
    let emp_pw = temp_pw(&body);

    if emp_policy_auto {
        let (st, _) = admin
            .put(
                &format!("/api/v1/team-settings/{}", emp_id),
                &json!({"allow_reopen_without_approval": true}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "set emp policy auto");
    }

    let last_mon = next_monday(-14);
    let monday_iso = last_mon.format("%Y-%m-%d").to_string();

    (lead_id, lead_pw, emp_id, emp_pw, monday_iso, cat_id)
}

pub async fn login_change_pw(app: &TestApp, email: &str, temp: &str) -> TestClient {
    let c = app.client();
    let (st, _) = c.login(email, temp).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = c.change_password(temp, "GoodPass!234").await;
    assert_eq!(st, StatusCode::OK);
    c
}

pub async fn create_and_submit_entry(c: &TestClient, monday_iso: &str, cat_id: i64) -> i64 {
    let (st, body) = c
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso, "start_time":"08:00","end_time":"12:00",
                "category_id": cat_id, "comment":"work"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create entry");
    let eid = id(&body);
    let (st, _) = c
        .post("/api/v1/time-entries/submit", &json!({"ids":[eid]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit entry");
    eid
}

/// Login as admin and change the initial password.
pub async fn admin_login(app: &TestApp) -> TestClient {
    let admin = app.client();
    admin.login("admin@example.com", &app.admin_password).await;
    admin
        .change_password(&app.admin_password, "AdminPass!234")
        .await;
    admin
}
