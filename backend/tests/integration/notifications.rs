//! End-to-end notification workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use chrono::{Duration, NaiveDate};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn notifications_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Notifications CRUD --
    {
        let (_lead_id, _lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, true, "1").await;
        let emp = login_change_pw(&app, "emp-1@example.com", &emp_pw).await;
        let _ = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
        emp.post(
            "/api/v1/reopen-requests",
            &json!({"week_start": monday_iso}),
        )
        .await;

        let (st, body) = emp.get("/api/v1/notifications/unread-count").await;
        assert_eq!(st, StatusCode::OK);
        assert!(body["count"].as_i64().unwrap() >= 1);

        let (st, list) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let nid = list[0]["id"].as_i64().unwrap();

        let (st, _) = emp
            .post(&format!("/api/v1/notifications/{}/read", nid), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK);

        let (st, _) = emp.post("/api/v1/notifications/read-all", &json!({})).await;
        assert_eq!(st, StatusCode::OK);

        let (st, body) = emp.get("/api/v1/notifications/unread-count").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["count"], 0);

        let (st, _) = emp.delete("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK);
        let (_, list) = emp.get("/api/v1/notifications").await;
        assert_eq!(list.as_array().unwrap().len(), 0);
    }

    // -- Absence request notifies approver --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;
        let future_monday_iso = next_monday(21).format("%Y-%m-%d").to_string();
        let emp = login_change_pw(&app, "emp-2@example.com", &emp_pw).await;
        let lead = login_change_pw(&app, "lead-2@example.com", &lead_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": future_monday_iso,
                    "end_date": future_monday_iso,
                    "comment": "need a day off"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create absence request");
        assert_eq!(body["status"], "requested");

        let (st, body) = lead.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK, "lead notifications");
        assert!(
            body.as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "absence_requested"),
            "lead received absence request notification"
        );
    }

    // -- Multiple approvers (including non-admin leads) all get notifications --
    {
        let (lead_one_id, lead_one_pw, emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "multi").await;
        let future_monday_iso = next_monday(21).format("%Y-%m-%d").to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"lead-multi-two@example.com",
                    "first_name":"LaraTwo",
                    "last_name":"Lead",
                    "role":"team_lead",
                    "weekly_hours":39,
                    "leave_days_current_year":30,
                    "leave_days_next_year":30,
                    "start_date":"2024-01-01",
                    "approver_ids":[1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create second lead approver");
        let lead_two_id = id(&body);
        let lead_two_pw = temp_pw(&body);

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"approver_ids": [lead_one_id, lead_two_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "assign two approvers to employee");

        let emp = login_change_pw(&app, "emp-multi@example.com", &emp_pw).await;
        let lead_one = login_change_pw(&app, "lead-multi@example.com", &lead_one_pw).await;
        let lead_two = login_change_pw(&app, "lead-multi-two@example.com", &lead_two_pw).await;

        let _ = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

        let (st, lead_one_notifications) = lead_one.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "lead one notifications after timesheet submit"
        );
        assert!(
            lead_one_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "timesheet_submitted"),
            "lead one received timesheet_submitted"
        );

        let (st, lead_two_notifications) = lead_two.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "lead two notifications after timesheet submit"
        );
        assert!(
            lead_two_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "timesheet_submitted"),
            "lead two received timesheet_submitted"
        );

        let (st, absence) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": future_monday_iso,
                    "end_date": future_monday_iso,
                    "comment": "multi approver test"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create absence for multi approver test");
        assert_eq!(absence["status"], "requested");

        let (st, lead_one_notifications) = lead_one.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "lead one notifications after absence request"
        );
        assert!(
            lead_one_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "absence_requested"),
            "lead one received absence_requested"
        );

        let (st, lead_two_notifications) = lead_two.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "lead two notifications after absence request"
        );
        assert!(
            lead_two_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "absence_requested"),
            "lead two received absence_requested"
        );
    }

    // -- Admin gets notifications only when explicitly assigned as approver --
    {
        let (_lead_id, _lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "admin-approver").await;
        let future_monday_iso = next_monday(21).format("%Y-%m-%d").to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"admin-a@example.com",
                    "first_name":"AdminA",
                    "last_name":"User",
                    "role":"admin",
                    "weekly_hours":39,
                    "leave_days_current_year":30,
                    "leave_days_next_year":30,
                    "start_date":"2024-01-01",
                    "approver_ids":[]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin A");
        let admin_a_id = id(&body);
        let admin_a_pw = temp_pw(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"admin-b@example.com",
                    "first_name":"AdminB",
                    "last_name":"User",
                    "role":"admin",
                    "weekly_hours":39,
                    "leave_days_current_year":30,
                    "leave_days_next_year":30,
                    "start_date":"2024-01-01",
                    "approver_ids":[]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin B");
        let admin_b_id = id(&body);
        let admin_b_pw = temp_pw(&body);

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"approver_ids": [admin_a_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "assign only admin A as approver");

        let emp = login_change_pw(&app, "emp-admin-approver@example.com", &emp_pw).await;
        let admin_a = login_change_pw(&app, "admin-a@example.com", &admin_a_pw).await;
        let admin_b = login_change_pw(&app, "admin-b@example.com", &admin_b_pw).await;

        let _ = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

        let (st, admin_a_notifications) = admin_a.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "admin A notifications after timesheet submit"
        );
        assert!(
            admin_a_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "timesheet_submitted"),
            "assigned admin received timesheet_submitted"
        );

        let (st, admin_b_notifications) = admin_b.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "admin B notifications after timesheet submit"
        );
        assert!(
            !admin_b_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "timesheet_submitted"),
            "unassigned admin did not receive timesheet_submitted"
        );

        let (st, absence) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": future_monday_iso,
                    "end_date": future_monday_iso,
                    "comment": "admin approver notification test"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "create absence for admin approver notification test"
        );
        assert_eq!(absence["status"], "requested");

        let (st, admin_a_notifications) = admin_a.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "admin A notifications after absence request"
        );
        assert!(
            admin_a_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "absence_requested"),
            "assigned admin received absence_requested"
        );

        let (st, admin_b_notifications) = admin_b.get("/api/v1/notifications").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "admin B notifications after absence request"
        );
        assert!(
            !admin_b_notifications
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "absence_requested"),
            "unassigned admin did not receive absence_requested"
        );

        let _ = admin_b_id;
    }

    // -- Notification in-app body has no URL even when public URL is set --
    // When public_url is configured, the app URL must appear in email bodies but
    // must NOT be stored in the in-app notification body (which is shown inside the
    // app where a bare URL would be redundant and ugly).
    {
        let app2 = TestApp::spawn_with_public_url("https://test.example.com").await;
        let admin2 = admin_login(&app2).await;

        let (_lead_id, lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
            bootstrap_team_with_suffix(&app2, &admin2, false, "3").await;
        let future_monday_iso = next_monday(21).format("%Y-%m-%d").to_string();
        let emp = login_change_pw(&app2, "emp-3@example.com", &emp_pw).await;
        let lead = login_change_pw(&app2, "lead-3@example.com", &lead_pw).await;

        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &serde_json::json!({
                    "kind": "vacation",
                    "start_date": future_monday_iso,
                    "end_date": future_monday_iso,
                }),
            )
            .await;
        assert_eq!(st, reqwest::StatusCode::OK);

        let (st, notifications) = lead.get("/api/v1/notifications").await;
        assert_eq!(st, reqwest::StatusCode::OK);

        let notification = notifications
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "absence_requested")
            .expect("lead must have received absence_requested notification");

        let body = notification["body"].as_str().unwrap_or("");
        assert!(
            !body.contains("https://test.example.com"),
            "in-app notification body must not contain the app URL, got: {body:?}"
        );
        assert!(
            !body.is_empty(),
            "in-app notification body must not be empty"
        );

        app2.cleanup().await;
    }

    // -- Timesheet batch approval notification counts weeks not entries --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "4").await;
        let emp = login_change_pw(&app, "emp-4@example.com", &emp_pw).await;
        let lead = login_change_pw(&app, "lead-4@example.com", &lead_pw).await;

        let monday = NaiveDate::parse_from_str(&monday_iso, "%Y-%m-%d").expect("valid monday date");
        let tuesday_iso = (monday + Duration::days(1)).format("%Y-%m-%d").to_string();

        let slots = [
            (&monday_iso, "08:00", "09:00"),
            (&monday_iso, "09:30", "10:30"),
            (&monday_iso, "11:00", "12:00"),
            (&tuesday_iso, "08:00", "09:00"),
            (&tuesday_iso, "09:30", "10:30"),
            (&tuesday_iso, "11:00", "12:00"),
        ];

        let mut ids = Vec::new();
        for (entry_date, start_time, end_time) in slots {
            let (st, body) = emp
                .post(
                    "/api/v1/time-entries",
                    &json!({
                        "entry_date": entry_date,
                        "start_time": start_time,
                        "end_time": end_time,
                        "category_id": cat_id,
                        "comment": "week test"
                    }),
                )
                .await;
            assert_eq!(
                st,
                StatusCode::OK,
                "create time entry for weekly notification test"
            );
            ids.push(id(&body));
        }

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": ids}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit weekly timesheet");

        let (st, body) = lead.get("/api/v1/time-entries/all?status=submitted").await;
        assert_eq!(st, StatusCode::OK, "lead fetches submitted entries");
        let approve_ids: Vec<i64> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_i64().expect("submitted entry id"))
            .collect();
        assert_eq!(approve_ids.len(), 6, "six submitted entries expected");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": approve_ids}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "batch approve submitted entries");

        let (st, notifications) = emp.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK, "employee notifications");

        let approval_notification = notifications
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "timesheet_approved")
            .expect("timesheet_approved notification must exist");

        assert_eq!(approval_notification["title"], "Week approved");
        let body = approval_notification["body"]
            .as_str()
            .expect("timesheet approval notification body must be string");
        // Body is now structured JSON for frontend rendering (contains week ISO dates).
        let parsed: serde_json::Value =
            serde_json::from_str(body).expect("notification body must be valid JSON");
        let weeks = parsed["weeks"]
            .as_array()
            .expect("JSON body must contain 'weeks' array");
        assert_eq!(weeks.len(), 1, "one distinct week expected");
    }

    app.cleanup().await;
}
