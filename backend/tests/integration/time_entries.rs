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
