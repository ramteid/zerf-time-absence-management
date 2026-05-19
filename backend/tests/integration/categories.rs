use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team_with_suffix, id, login_change_pw};

#[tokio::test]
async fn categories_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "cat").await;
    let emp = login_change_pw(&app, "emp-cat@example.com", &emp_pw).await;

    let (st, _) = emp.get("/api/v1/categories/all").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins can list all");

    let (st, _) = emp
        .post(
            "/api/v1/categories",
            &json!({"name": "Blocked", "color": "#112233"}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins can create categories");

    let (st, _) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "", "color": "#112233"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "name must be non-empty");

    let (st, _) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "Domain Focus", "color": "bad-color"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "color must be hex");

    let (st, body) = admin
        .post(
            "/api/v1/categories",
            &json!({
                "name": "Domain Focus",
                "description": "Used in integration tests",
                "color": "#112233",
                "sort_order": 99,
                "counts_as_work": true
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can create valid category");
    let category_id = id(&body);

    let (st, body) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "Domain Focus", "color": "#445566"}),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate category names are rejected");
    assert!(body.to_string().contains("Name already exists"));

    let (st, body) = admin
        .put(
            &format!("/api/v1/categories/{category_id}"),
            &json!({
                "name": " Domain Focus Updated ",
                "description": null,
                "color": "#a1B2c3",
                "sort_order": 7,
                "counts_as_work": false,
                "active": false
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can update category");
    assert_eq!(body["name"], "Domain Focus Updated");
    assert_eq!(body["active"], false);
    assert_eq!(body["counts_as_work"], false);

    let (st, active_list) = admin.get("/api/v1/categories").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        active_list
            .as_array()
            .expect("active list")
            .iter()
            .all(|c| c["id"].as_i64() != Some(category_id)),
        "inactive categories must not appear in active list"
    );

    let (st, all_list) = admin.get("/api/v1/categories/all").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        all_list
            .as_array()
            .expect("all list")
            .iter()
            .any(|c| c["id"].as_i64() == Some(category_id) && c["active"] == false),
        "admin all-list must include inactive categories"
    );

    app.cleanup().await;
}
