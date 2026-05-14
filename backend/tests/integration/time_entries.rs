//! End-to-end time entries workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn time_entries_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Non-crediting entries still block overlaps, but don't consume 14h cap --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "0").await;
        let emp = login_change_pw(&app, "emp-0@example.com", &emp_pw).await;

        let (st, categories_body) = emp.get("/api/v1/categories").await;
        assert_eq!(st, StatusCode::OK, "load categories");
        let category_rows = categories_body.as_array().expect("categories array");
        let crediting_category_id = category_rows
            .iter()
            .find(|row| row["counts_as_work"].as_bool().unwrap_or(true))
            .and_then(|row| row["id"].as_i64())
            .expect("crediting category exists");
        let non_crediting_category_id = category_rows
            .iter()
            .find(|row| row["counts_as_work"].as_bool() == Some(false))
            .and_then(|row| row["id"].as_i64())
            .expect("non-crediting category exists");
        let day = monday_iso;

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day,
                    "start_time": "00:00",
                    "end_time": "10:00",
                    "category_id": non_crediting_category_id,
                    "comment": "flextime reduction"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create non-crediting entry");

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day,
                    "start_time": "09:00",
                    "end_time": "11:00",
                    "category_id": crediting_category_id,
                    "comment": "must be blocked by overlap"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "overlap with non-crediting entry rejected"
        );

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day,
                    "start_time": "00:00",
                    "end_time": "10:00",
                    "category_id": non_crediting_category_id,
                    "comment": "non-crediting part"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "duplicate overlapping non-crediting entry rejected"
        );

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day,
                    "start_time": "10:00",
                    "end_time": "23:00",
                    "category_id": crediting_category_id,
                    "comment": "13h crediting should still be allowed"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "14h cap ignores non-crediting minutes");
    }

    // -- Invalid category rejected --
    {
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
    }

    // -- Reject requires reason before ownership check --
    {
        let (_lead_id, lead_pw, _emp_id, _emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "1").await;
        let lead = login_change_pw(&app, "lead-1@example.com", &lead_pw).await;

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
                "/api/v1/time-entries/batch-reject",
                &json!({"ids": [entry_id], "reason": "   "}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "blank reason rejected before processing"
        );
    }

    // -- Blocks time entry when absence cancellation pending --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;
        let lead = login_change_pw(&app, "lead-2@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-2@example.com", &emp_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": monday_iso,
                    "end_date": monday_iso,
                    "comment": "day off"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create requested absence");
        let absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve absence");

        let (st, body) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
        assert_eq!(st, StatusCode::OK, "request cancellation");
        assert_eq!(body["pending"], true);

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday_iso,
                    "start_time": "08:00",
                    "end_time": "10:00",
                    "category_id": cat_id,
                    "comment": "should be blocked"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cancellation-pending absence must still block time logging"
        );
    }

    // -- Admin can batch reject own submitted entry --
    {
        let monday_iso = today();
        let (_, categories_body) = admin.get("/api/v1/categories").await;
        let category_id = categories_body.as_array().unwrap()[0]["id"]
            .as_i64()
            .unwrap();

        let (st, body) = admin
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday_iso,
                    "start_time": "00:00",
                    "end_time": "00:01",
                    "category_id": category_id,
                    "comment": "admin entry"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin creates own entry");

        let entry_id = id(&body);

        let (st, _) = admin
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit admin entry");

        let (st, body) = admin
            .post(
                "/api/v1/time-entries/batch-reject",
                &json!({"ids": [entry_id], "reason": "needs correction"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "batch reject succeeds");
        assert_eq!(body["count"], 1);

        let (_, entries) = admin.get("/api/v1/time-entries").await;
        let entry = find_by_id(&entries, entry_id).expect("entry exists");
        assert_eq!(entry["status"], "rejected");
    }

    app.cleanup().await;
}
