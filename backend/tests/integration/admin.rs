use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn self_submission_is_visible_and_notifies_admin() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, body) = admin.get("/api/v1/categories").await;
    let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();
    let monday = next_monday(-14).format("%Y-%m-%d").to_string();
    let entry_id = create_and_submit_entry(&admin, &monday, cat_id).await;

    let (st, body) = admin.get("/api/v1/notifications").await;
    assert_eq!(st, StatusCode::OK, "admin notifications");
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "timesheet_submitted"),
        "admin received self-submission notification"
    );

    let (st, body) = admin.get("/api/v1/time-entries/all?status=submitted").await;
    assert_eq!(st, StatusCode::OK, "admin submitted entries visible");
    assert!(has_id(&body, entry_id));

    let (st, _) = admin
        .post(
            &format!("/api/v1/time-entries/{}/approve", entry_id),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can approve self-submitted entry");

    app.cleanup().await;
}

#[tokio::test]
async fn settings_validate_and_persist_user_defaults() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, _) = admin
        .put(
            "/api/v1/settings",
            &json!({
                "ui_language": "en",
                "country": "DE",
                "region": "DE-BW",
                "default_weekly_hours": 169,
                "default_annual_leave_days": 30
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "invalid default hours rejected"
    );

    let (st, _) = admin
        .put(
            "/api/v1/settings",
            &json!({
                "ui_language": "de",
                "country": "DE",
                "region": "DE-BW",
                "default_weekly_hours": 35.5,
                "default_annual_leave_days": 28
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "valid defaults saved");

    let anon = app.client();
    let (st, body) = anon.get("/api/v1/settings/public").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["ui_language"], "de");
    assert_eq!(body["default_weekly_hours"], 35.5);
    assert_eq!(body["default_annual_leave_days"], 28);

    app.cleanup().await;
}
