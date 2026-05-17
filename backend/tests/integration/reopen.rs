//! End-to-end reopen request workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn reopen_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Auto approve also reopens non-crediting submitted entries --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, true, "0").await;
        let emp = login_change_pw(&app, "emp-0@example.com", &emp_pw).await;

        let (st, categories_body) = emp.get("/api/v1/categories").await;
        assert_eq!(st, StatusCode::OK, "load categories");
        let non_crediting_category_id = categories_body
            .as_array()
            .unwrap()
            .iter()
            .find(|category| category["counts_as_work"].as_bool() == Some(false))
            .and_then(|category| category["id"].as_i64())
            .expect("non-crediting category exists");

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday_iso,
                    "start_time":"08:00",
                    "end_time":"12:00",
                    "category_id": non_crediting_category_id,
                    "comment":"flextime reduction"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create non-crediting entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit non-crediting entry");

        let (st, body) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "auto reopen non-crediting week");
        assert_eq!(body["status"], "auto_approved");
        assert_eq!(body["entries_reopened"], 1);

        let (st, body) = emp.get("/api/v1/time-entries").await;
        assert_eq!(st, StatusCode::OK);
        let entry = find_by_id(&body, entry_id).expect("entry present after reopen");
        assert_eq!(entry["status"], "draft");
    }

    // -- Auto approve when policy set --
    {
        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "de",
                    "time_format": "24h",
                    "country": "DE",
                    "region": "DE-BW",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "set German UI language");

        let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, true, "1").await;
        let emp = login_change_pw(&app, "emp-1@example.com", &emp_pw).await;
        let _eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

        let (st, body) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "auto reopen");
        assert_eq!(body["status"], "auto_approved");
        assert_eq!(body["entries_reopened"], 1);

        let (st, body) = emp.get("/api/v1/time-entries").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body[0]["status"], "draft");

        let (_, body) = emp.get("/api/v1/notifications").await;
        let notification = body
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["kind"] == "reopen_auto_approved")
            .expect("notification created");
        assert_eq!(
            notification["title"].as_str(),
            Some("Bearbeitungsanfrage genehmigt")
        );
        let body = notification["body"].as_str().unwrap_or("");
        // Body is now structured JSON for frontend rendering.
        let parsed: serde_json::Value =
            serde_json::from_str(body).expect("notification body must be valid JSON");
        assert!(
            parsed["week"].as_str().is_some(),
            "JSON body should include 'week' field: {body}"
        );
        assert!(!notification["title"].as_str().unwrap().contains(" / "));
    }

    // -- Pending then approve --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;
        let emp = login_change_pw(&app, "emp-2@example.com", &emp_pw).await;
        let lead = login_change_pw(&app, "lead-2@example.com", &lead_pw).await;

        let _eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

        let (st, body) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["status"], "pending");
        let req_id = id(&body);

        // Duplicate pending rejected.
        let (st, _) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        assert_eq!(st, StatusCode::CONFLICT, "duplicate rejected");

        // Lead sees it in their queue.
        let (_, body) = lead.get("/api/v1/reopen-requests/pending").await;
        assert!(has_id(&body, req_id), "lead sees request: {body:?}");

        let (st, body) = lead
            .post(
                &format!("/api/v1/reopen-requests/{}/approve", req_id),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve");
        assert_eq!(body["entries_reopened"], 1);

        let (_, body) = emp.get("/api/v1/time-entries").await;
        assert_eq!(body[0]["status"], "draft");

        let (_, body) = emp.get("/api/v1/notifications").await;
        assert!(body
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "reopen_approved"));
    }

    // -- Pending then reject --
    {
        let (lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3").await;
        let emp = login_change_pw(&app, "emp-3@example.com", &emp_pw).await;
        let lead = login_change_pw(&app, "lead-3@example.com", &lead_pw).await;

        let eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

        let (_, body) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        let req_id = id(&body);

        // Reject without reason -> 400.
        let (st, _) = lead
            .post(
                &format!("/api/v1/reopen-requests/{}/reject", req_id),
                &json!({"reason": ""}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        let (st, _) = lead
            .post(
                &format!("/api/v1/reopen-requests/{}/reject", req_id),
                &json!({"reason": "Not necessary"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);

        let (st, body) = emp.get("/api/v1/reopen-requests").await;
        assert_eq!(st, StatusCode::OK);
        let request = find_by_id(&body, req_id).expect("reopen request present");
        assert_eq!(request["status"], "rejected");
        assert_eq!(request["reviewed_by"], lead_id);

        let (_, body) = emp.get("/api/v1/time-entries").await;
        assert_eq!(body[0]["status"], "submitted", "entry stays submitted");
        let _ = eid;

        let (_, body) = emp.get("/api/v1/notifications").await;
        assert!(body
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "reopen_rejected"));
    }

    // -- Empty week rejected --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, true, "4").await;
        let emp = login_change_pw(&app, "emp-4@example.com", &emp_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "empty week rejected");
        assert!(body["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("no submitted, approved, or rejected entries"));
    }

    // -- Not monday rejected --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, true, "5").await;
        let emp = login_change_pw(&app, "emp-5@example.com", &emp_pw).await;

        let tuesday = (next_monday(-14) + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let (st, _) = emp
            .post("/api/v1/reopen-requests", &json!({"week_start": tuesday}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "tuesday rejected");
    }

    // -- Lead self-service --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"solo@example.com","first_name":"Sol","last_name":"O",
                    "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        let lead_id = id(&body);
        let pw = temp_pw(&body);
        let lead = login_change_pw(&app, "solo@example.com", &pw).await;

        let (st, _) = admin
            .put(
                &format!("/api/v1/team-settings/{}", lead_id),
                &json!({"allow_reopen_without_approval": true}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "set lead policy auto");

        let (_, body) = admin.get("/api/v1/categories").await;
        let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();

        let monday_iso = next_monday(-14).format("%Y-%m-%d").to_string();
        let _ = create_and_submit_entry(&lead, &monday_iso, cat_id).await;

        let (st, body) = lead
            .post(
                "/api/v1/reopen-requests",
                &json!({"week_start": monday_iso}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead self-service auto");
        assert_eq!(body["status"], "auto_approved");
    }

    app.cleanup().await;
}
