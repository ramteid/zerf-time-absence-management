use chrono::Datelike;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team_with_suffix, login_change_pw, reference_date};

#[tokio::test]
async fn holidays_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "hol").await;
    let emp = login_change_pw(&app, "emp-hol@example.com", &emp_pw).await;

    let current_year = reference_date().year();

    let (st, countries) = admin.get("/api/v1/holidays/countries").await;
    assert_eq!(st, StatusCode::OK, "countries endpoint should be reachable");
    assert!(
        countries
            .as_array()
            .expect("countries array")
            .iter()
            .any(|row| row["countryCode"] == "DE"),
        "seed country DE should be available"
    );

    let (st, regions) = admin.get("/api/v1/holidays/regions/DE").await;
    assert_eq!(st, StatusCode::OK, "regions endpoint should be reachable");
    assert!(
        regions.as_array().expect("regions array").len() > 0,
        "DE should provide at least one region code"
    );

    let new_holiday_date = format!("{}-12-30", current_year + 1);

    let (st, _) = emp
        .post(
            "/api/v1/holidays",
            &json!({"holiday_date": new_holiday_date, "name": "Employee Holiday"}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins can create holidays");

    let (st, _) = admin
        .post(
            "/api/v1/holidays",
            &json!({"holiday_date": new_holiday_date, "name": ""}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "holiday name must be non-empty");

    let (st, body) = admin
        .post(
            "/api/v1/holidays",
            &json!({"holiday_date": new_holiday_date, "name": "Integration Holiday"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can create manual holiday");
    assert_eq!(body["ok"], true);

    let (st, list) = admin
        .get(&format!("/api/v1/holidays?year={}", current_year + 1))
        .await;
    assert_eq!(st, StatusCode::OK);
    let inserted = list
        .as_array()
        .expect("holiday list")
        .iter()
        .find(|row| row["holiday_date"] == new_holiday_date)
        .expect("inserted holiday should be listed");
    let inserted_id = inserted["id"].as_i64().expect("id");
    assert_eq!(inserted["is_auto"], false);

    let (st, body) = admin
        .post(
            "/api/v1/holidays",
            &json!({"holiday_date": new_holiday_date, "name": "Integration Holiday"}),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate date is rejected");
    assert!(body.to_string().contains("Holiday already exists"));

    let (st, _) = emp.delete(&format!("/api/v1/holidays/{inserted_id}")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins can delete holidays");

    let (st, _body) = admin.delete("/api/v1/holidays/99999999").await;
    assert_eq!(st, StatusCode::NOT_FOUND, "deleting missing holiday returns 404");

    let (st, body) = admin.delete(&format!("/api/v1/holidays/{inserted_id}")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["ok"], true);

    app.cleanup().await;
}
