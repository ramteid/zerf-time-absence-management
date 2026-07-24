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
        !regions.as_array().expect("regions array").is_empty(),
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

    // Authorization must be checked before input validation: a non-admin
    // sending an invalid name still gets 403, not 400 — the admin check runs
    // first in services::holidays::create_manual, so an unauthorized caller
    // never learns anything about validation rules.
    let (st, _) = emp
        .post(
            "/api/v1/holidays",
            &json!({"holiday_date": new_holiday_date, "name": ""}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "non-admin with an invalid name is still forbidden, not a validation error"
    );

    let (st, _) = admin
        .post(
            "/api/v1/holidays",
            &json!({"holiday_date": new_holiday_date, "name": ""}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "holiday name must be non-empty"
    );

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
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "deleting missing holiday returns 404"
    );

    let (st, body) = admin
        .delete(&format!("/api/v1/holidays/{inserted_id}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["ok"], true);

    app.cleanup().await;
}

#[tokio::test]
async fn recurring_holidays_http_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let current_year = reference_date().year();

    // recurrence_end_year without recurring is rejected.
    let (st, body) = admin
        .post(
            "/api/v1/holidays",
            &json!({
                "holiday_date": format!("{current_year}-12-24"),
                "name": "Bad Combo",
                "recurring": false,
                "recurrence_end_year": current_year + 5,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "an end year requires recurring to be enabled"
    );
    assert!(body
        .to_string()
        .contains("An end year requires the recurring option to be enabled"));

    // An end year before the holiday's own year is rejected.
    let (st, body) = admin
        .post(
            "/api/v1/holidays",
            &json!({
                "holiday_date": format!("{current_year}-12-24"),
                "name": "Bad End Year",
                "recurring": true,
                "recurrence_end_year": current_year - 1,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(body
        .to_string()
        .contains("The recurrence end year cannot be before the holiday's year"));

    // A recurring holiday with no end must show up years into the future.
    let (st, body) = admin
        .post(
            "/api/v1/holidays",
            &json!({
                "holiday_date": format!("{current_year}-12-24"),
                "name": "Recurring Heiligabend",
                "recurring": true,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can create a recurring holiday");
    assert_eq!(body["ok"], true);

    let future_year = current_year + 10;
    let (st, list) = admin
        .get(&format!("/api/v1/holidays?year={future_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    let future_date = format!("{future_year}-12-24");
    let projected = list
        .as_array()
        .expect("holiday list")
        .iter()
        .find(|row| row["holiday_date"] == future_date)
        .expect("recurring holiday should be projected into a future year");
    assert_eq!(projected["recurring"], true);
    let recurring_id = projected["id"].as_i64().expect("id");

    // A bounded recurring holiday disappears after its end year.
    let end_year = current_year + 2;
    let (st, _) = admin
        .post(
            "/api/v1/holidays",
            &json!({
                "holiday_date": format!("{current_year}-08-20"),
                "name": "Bounded Recurring Holiday",
                "recurring": true,
                "recurrence_end_year": end_year,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);

    let (st, list_at_end) = admin
        .get(&format!("/api/v1/holidays?year={end_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        list_at_end
            .as_array()
            .expect("holiday list")
            .iter()
            .any(|row| row["holiday_date"] == format!("{end_year}-08-20")),
        "bounded recurring holiday must still show in its end year"
    );

    let (st, list_after_end) = admin
        .get(&format!("/api/v1/holidays?year={}", end_year + 1))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !list_after_end
            .as_array()
            .expect("holiday list")
            .iter()
            .any(|row| row["name"] == "Bounded Recurring Holiday"),
        "bounded recurring holiday must not show after its end year"
    );

    // Deleting the recurring holiday (found via a future year's listing)
    // removes it from every year's listing, not only the one it was viewed
    // from -- there is no per-year row to delete separately.
    let (st, _) = admin
        .delete(&format!("/api/v1/holidays/{recurring_id}"))
        .await;
    assert_eq!(st, StatusCode::OK);

    let (st, list_current) = admin
        .get(&format!("/api/v1/holidays?year={current_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(!list_current
        .as_array()
        .expect("holiday list")
        .iter()
        .any(|row| row["id"] == recurring_id));

    let (st, list_future) = admin
        .get(&format!("/api/v1/holidays?year={future_year}"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !list_future
            .as_array()
            .expect("holiday list")
            .iter()
            .any(|row| row["id"] == recurring_id),
        "deleting a recurring holiday must remove it from every projected year too"
    );

    app.cleanup().await;
}
