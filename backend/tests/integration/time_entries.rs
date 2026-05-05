use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn invalid_category_rejected() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, _) = admin
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": today(),
                "start_time": "08:00",
                "end_time": "10:00",
                "category_id": 999_999_i64,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "nonexistent category -> 400");

    app.cleanup().await;
}

#[tokio::test]
async fn reject_requires_reason_before_ownership_check() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_lead_id, lead_pw, _emp_id, _emp_pw, monday_iso, cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let (st, body) = lead
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": cat_id,
                "comment": "lead work"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "lead creates own entry");
    let entry_id = id(&body);

    let (st, _) = lead
        .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
        .await;
    assert_eq!(st, StatusCode::OK, "lead submits own entry");

    let (st, _) = lead
        .post(
            &format!("/api/v1/time-entries/{}/reject", entry_id),
            &json!({"reason": "   "}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "blank reason wins over ownership check"
    );

    app.cleanup().await;
}
