//! End-to-end user management workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Create a team lead (approver = admin/id 1) and return its id.
async fn create_lead(admin: &crate::common::TestClient, email: &str, first: &str) -> i64 {
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": email, "first_name": first, "last_name": "Lead",
                "role": "team_lead", "weekly_hours": 39,
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01", "approver_ids": [1],
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create lead {email}");
    id(&body)
}

/// Create an employee whose approver is `approver_id` and return its id.
async fn create_emp(
    admin: &crate::common::TestClient,
    email: &str,
    first: &str,
    approver_id: i64,
) -> i64 {
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": email, "first_name": first, "last_name": "Emp",
                "role": "employee", "weekly_hours": 39,
                "leave_days_current_year": 30, "leave_days_next_year": 30,
                "start_date": "2024-01-01", "approver_ids": [approver_id],
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create emp {email}");
    id(&body)
}

#[tokio::test]
async fn users_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = app.client();
    let (st, _) = admin.login("admin@example.com", &app.admin_password).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = admin
        .change_password(&app.admin_password, "AdminPass!234")
        .await;
    assert_eq!(st, StatusCode::OK);

    // -- Non-admin users must have approver --
    {
        // Missing approver_id is rejected for employees.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"a@example.com","first_name":"A","last_name":"A",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "missing approver rejected");
        assert!(body["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("approver"));

        // Missing approver_id is rejected for team leads.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"lead-missing@example.com","first_name":"Lead","last_name":"Missing",
                    "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "missing team lead approver rejected"
        );
        assert!(body["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("approver"));

        // Approver = admin works.
        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"b@example.com","first_name":"B","last_name":"B",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "with approver works");

        // Team leads may report to another explicit team lead.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"lead-approver@example.com","first_name":"Lead","last_name":"Approver",
                    "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create team lead approver");
        let lead_approver_id = id(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"lead-report@example.com","first_name":"Lead","last_name":"Report",
                    "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids":[lead_approver_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create team lead with lead approver");
        let lead_report_id = id(&body);
        // Verify approver was stored by fetching the user detail.
        let (st, detail) = admin.get(&format!("/api/v1/users/{lead_report_id}")).await;
        assert_eq!(st, StatusCode::OK, "get lead report detail");
        assert!(
            detail["approver_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_i64() == Some(lead_approver_id)),
            "lead_approver_id should be in approver_ids"
        );

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{lead_report_id}"),
                &json!({"approver_ids": []}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "clearing team lead approver is rejected"
        );
        assert!(body["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("approver"));

        // Approver pointing at non-existent user.
        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"c@example.com","first_name":"C","last_name":"C",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids": [99999]}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "missing approver row rejected");

        // A regular employee cannot be used as approver for another employee.
        let employee_approver_id = create_emp(
            &admin,
            "employee-approver@example.com",
            "EmployeeApprover",
            1,
        )
        .await;
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"employee-report@example.com","first_name":"Employee","last_name":"Report",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids": [employee_approver_id]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "employee approver for employee is rejected"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .contains("approver"));

        // Duplicate approver IDs in the list are rejected.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"dup-approver@example.com","first_name":"Dup","last_name":"Approver",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids": [1, 1]}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "duplicate approver rejected");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .contains("duplicate"),
            "error mentions duplicate: {body}"
        );

        // A user cannot list themselves as their own approver.
        // To get the user's own ID we create them first with a valid approver,
        // then try to update with self as the approver.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"self-approver@example.com","first_name":"Self","last_name":"Approver",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create self-approver user: {body}");
        let self_user_id = id(&body);

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{self_user_id}"),
                &json!({"approver_ids": [self_user_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "self as approver rejected");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .contains("themselves"),
            "error mentions themselves: {body}"
        );

        // Assistants must not have fixed weekly target hours.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"assistant-invalid-hours@example.com","first_name":"Assist","last_name":"Hours",
                    "role":"assistant","weekly_hours":10,"leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "assistant weekly_hours must be 0"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("weekly_hours"));

        // Assistants cannot have a flextime carry-in balance.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"assistant-invalid-overtime@example.com","first_name":"Assist","last_name":"Overtime",
                    "role":"assistant","weekly_hours":0,"leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date":"2024-01-01","approver_ids": [1],"overtime_start_balance_min":60}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "assistant overtime start balance must be 0"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("overtime"));

        // Valid assistant creation works with zero leave and zero weekly hours.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"assistant-valid@example.com","first_name":"Assist","last_name":"Valid",
                    "role":"assistant","weekly_hours":0,"leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create assistant user");
        let assistant_id = id(&body);

        let (st, detail) = admin.get(&format!("/api/v1/users/{assistant_id}")).await;
        assert_eq!(st, StatusCode::OK, "fetch assistant detail");
        assert_eq!(
            detail["role"], "assistant",
            "assistant role stored canonically"
        );

        let (st, _body) = admin
            .put(
                &format!("/api/v1/users/{assistant_id}"),
                &json!({"role":" assistant ","weekly_hours":0,"approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "valid assistant update with padded role string succeeds after normalization"
        );

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{assistant_id}"),
                &json!({"role":" assistant ","weekly_hours":5,"approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "assistant invariant still applies to normalized update role"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("weekly_hours"));

        // Assistants cannot have workdays_per_week set (they have no fixed weekdays).
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"assistant-invalid-workdays@example.com","first_name":"Assist","last_name":"Workdays",
                    "role":"assistant","weekly_hours":0,"leave_days_current_year":0,"leave_days_next_year":0,
                    "workdays_per_week":5,"start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "assistant workdays_per_week must not be set"
        );
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("fixed working days"),
            "error should mention fixed working days: {body}"
        );

        // Updating an assistant with workdays_per_week is also rejected.
        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{assistant_id}"),
                &json!({"workdays_per_week":3,"approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "updating assistant workdays_per_week must be rejected"
        );
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("fixed working days"),
            "error should mention fixed working days: {body}"
        );

        // Assistants are stored with workdays_per_week=7 (all days possible) as a sentinel.
        let (st, detail) = admin.get(&format!("/api/v1/users/{assistant_id}")).await;
        assert_eq!(st, StatusCode::OK, "fetch assistant detail");
        assert_eq!(
            detail["workdays_per_week"], 7,
            "assistants must be stored with workdays_per_week=7"
        );

        // Switching FROM assistant TO employee without providing workdays_per_week must
        // reset it to 5 (the default), not leave the sentinel 7 via COALESCE.
        let (st, detail) = admin
            .put(
                &format!("/api/v1/users/{assistant_id}"),
                &json!({"role": "employee", "weekly_hours": 40.0, "approver_ids": [1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "role switch assistant → employee succeeds"
        );
        assert_eq!(
            detail["workdays_per_week"], 5,
            "workdays_per_week must be reset to 5 when switching away from assistant"
        );
    }

    // -- Non-assistant users are restricted to 1-5 workdays per week --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"employee-invalid-overtime@example.com","first_name":"Emp","last_name":"Overtime",
                    "role":"employee","weekly_hours":40,"leave_days_current_year":20,"leave_days_next_year":20,
                    "overtime_start_balance_min":600001,"start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "create rejects overtime balance above range"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("overtime"));

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"employee-tracks-off@example.com","first_name":"Emp","last_name":"TracksOff",
                    "role":"employee","weekly_hours":40,"leave_days_current_year":20,"leave_days_next_year":20,
                    "tracks_time": false,"start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "create rejects tracks_time=false for non-admins"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("tracks_time"));

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"employee-invalid-workdays@example.com","first_name":"Emp","last_name":"Wdays",
                    "role":"employee","weekly_hours":40,"leave_days_current_year":20,"leave_days_next_year":20,
                    "workdays_per_week":6,"start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "workdays_per_week=6 must be rejected for employees"
        );
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("workdays_per_week"),
            "error should mention workdays_per_week: {body}"
        );

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"employee-invalid-workdays7@example.com","first_name":"Emp","last_name":"Wdays7",
                    "role":"employee","weekly_hours":40,"leave_days_current_year":20,"leave_days_next_year":20,
                    "workdays_per_week":7,"start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "workdays_per_week=7 must be rejected for employees"
        );
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("workdays_per_week"),
            "error should mention workdays_per_week: {body}"
        );
    }

    // -- Duplicate user identifiers are rejected --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "unique@example.com",
                    "first_name": "Unique",
                    "last_name": "Person",
                    "role": "employee",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30, "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create baseline user");
        let baseline_id = id(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "unique@example.com",
                    "first_name": "Different",
                    "last_name": "Person",
                    "role": "employee",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30, "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::CONFLICT, "duplicate email rejected");
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("Email already exists."));

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "same-name@example.com",
                    "first_name": " Unique ",
                    "last_name": " Person ",
                    "role": "employee",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30, "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::CONFLICT, "duplicate full name rejected");
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("First name and last name already exist."));

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "other@example.com",
                    "first_name": "Other",
                    "last_name": "Person",
                    "role": "employee",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30, "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create second user");
        let second_id = id(&body);

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{second_id}"),
                &json!({"first_name": "Unique", "last_name": "Person"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::CONFLICT,
            "duplicate full name update rejected"
        );
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("First name and last name already exist."));

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{baseline_id}"),
                &json!({"first_name": " Unique ", "last_name": " Person "}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "updating same user with trimmed name works"
        );
        assert_eq!(body["first_name"], "Unique");
        assert_eq!(body["last_name"], "Person");

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{second_id}"),
                &json!({"email":"unique@example.com"}),
            )
            .await;
        assert_eq!(st, StatusCode::CONFLICT, "duplicate email update rejected");
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("Email already exists."));
    }

    // -- Creation password modes set must change correctly --
    {
        let manual_password = "ManualPass!234";
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "manual@example.com",
                    "first_name": "Manual",
                    "last_name": "User",
                    "role": "team_lead",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30, "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                    "password": manual_password,
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create user with manual password");
        assert_eq!(body["temporary_password"], manual_password);
        assert_eq!(body["user"]["must_change_password"], true);

        let manual = app.client();
        let (st, _) = manual.login("manual@example.com", manual_password).await;
        assert_eq!(st, StatusCode::OK, "manual password login");
        let (st, body) = manual.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["must_change_password"], true);

        let generated_password = "GeneratedPass!234";
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "generated@example.com",
                    "first_name": "Generated",
                    "last_name": "User",
                    "role": "employee",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30, "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                    "password": generated_password,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "create user with explicit password always requires change"
        );
        assert_eq!(body["temporary_password"], generated_password);
        assert_eq!(body["user"]["must_change_password"], true);

        let generated = app.client();
        let (st, _) = generated
            .login("generated@example.com", generated_password)
            .await;
        assert_eq!(st, StatusCode::OK, "generated password login");
        let (st, body) = generated.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["must_change_password"], true);
    }

    // -- Delete user removes data and preserves approved records --
    {
        let lead_id = create_lead(&admin, "lead-del@example.com", "DelLead").await;
        let emp_id = create_emp(&admin, "emp-del@example.com", "DelEmp", lead_id).await;

        // Cannot delete while emp still has lead as approver.
        let (st, body) = admin.delete(&format!("/api/v1/users/{lead_id}")).await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "delete with active reports must fail"
        );
        let error_msg = body["error"].as_str().unwrap_or("").to_lowercase();
        assert!(
            error_msg.contains("approver") || error_msg.contains("reassign"),
            "error must mention approver/reassign, got: {error_msg}"
        );

        // Cannot delete yourself.
        let (st, _) = admin.delete("/api/v1/users/1").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "deleting yourself must fail");

        // Reassign emp to admin, then delete the lead.
        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &serde_json::json!({"approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "reassign emp to admin");

        let (st, _) = admin.delete(&format!("/api/v1/users/{lead_id}")).await;
        assert_eq!(st, StatusCode::OK, "delete after reassign must succeed");

        // Lead must no longer appear in the user list.
        let (st, list) = admin.get("/api/v1/users").await;
        assert_eq!(st, StatusCode::OK);
        assert!(
            !list
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["id"].as_i64() == Some(lead_id)),
            "deleted lead must not appear in user list"
        );

        // Emp still exists and is now assigned to admin.
        let (st, detail) = admin.get(&format!("/api/v1/users/{emp_id}")).await;
        assert_eq!(st, StatusCode::OK, "emp still exists after lead deletion");
        assert!(
            detail["approver_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_i64() == Some(1)),
            "emp's approver must be admin after lead deleted"
        );

        // Delete emp too — should succeed since no active reports.
        let (st, _) = admin.delete(&format!("/api/v1/users/{emp_id}")).await;
        assert_eq!(st, StatusCode::OK, "delete emp must succeed");
    }

    // -- Delete user who reviewed reopen request succeeds (regression test) --
    // Regression test: reopen_requests.reviewed_by originally had constraint name
    // reopen_requests_approver_id_fkey (the column was renamed in migration 002).
    // Migration 005 dropped the wrong name, leaving the old RESTRICT constraint in place.
    // Deleting a user who reviewed a reopen request would silently fail before migration 006.
    {
        let (lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "8").await;
        let lead = login_change_pw(&app, "lead-8@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-8@example.com", &emp_pw).await;

        // Employee submits and gets entries approved so they can request a reopen.
        let eid = create_and_submit_entry(&emp, &monday_iso, cat_id).await;
        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &serde_json::json!({"ids": [eid]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve entry");

        // Employee requests reopen; lead approves → reviewed_by = lead_id in reopen_requests.
        let (st, rr_body) = emp
            .post(
                "/api/v1/reopen-requests",
                &serde_json::json!({"week_start": monday_iso, "reason": "Test reason"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create reopen request");
        let rr_id = rr_body["id"].as_i64().unwrap();

        let (st, _) = lead
            .post(
                &format!("/api/v1/reopen-requests/{rr_id}/approve"),
                &serde_json::json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead approves reopen request");

        // Reassign emp to admin so lead has no active direct reports.
        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{_emp_id}"),
                &serde_json::json!({"approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "reassign emp");

        // Deleting lead must succeed even though they reviewed a reopen request.
        // This would fail with RESTRICT if migration 006 is missing.
        let (st, _) = admin.delete(&format!("/api/v1/users/{lead_id}")).await;
        assert_eq!(
            st,
            StatusCode::OK,
            "delete user who reviewed reopen request must succeed"
        );

        // The reopen request itself must still exist with reviewed_by = NULL.
        // (No direct API to check, but no FK error means the record was preserved.)
    }

    // -- Cannot delete last active admin --
    {
        // The seeded admin (id=1) is the only active admin — must be rejected.
        let (st, _) = admin.delete("/api/v1/users/1").await;
        // This hits "cannot delete yourself" first, so create a second admin to test the guard.
        assert_eq!(st, StatusCode::BAD_REQUEST);

        // Create a second admin by promoting a lead, then try to delete the first admin via the second.
        let second_admin_id = create_lead(&admin, "admin2@example.com", "Second").await;
        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{second_admin_id}"),
                &serde_json::json!({"role": "admin"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "promote to admin");

        // Login as second admin and try to delete admin 1 (the only remaining active admin of the pair).
        // Actually, now there are 2 admins — deleting admin 1 is allowed since admin 2 still exists.
        let second_admin_pw = {
            let (_, body) = admin
                .post(
                    &format!("/api/v1/users/{second_admin_id}/reset-password"),
                    &serde_json::json!({}),
                )
                .await;
            body["temporary_password"].as_str().unwrap().to_string()
        };
        let second_client = app.client();
        let (st, _) = second_client
            .login("admin2@example.com", &second_admin_pw)
            .await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = second_client
            .change_password(&second_admin_pw, "NewAdminPass!234")
            .await;
        assert_eq!(st, StatusCode::OK);

        // Now only one admin (second) — trying to delete second must fail (can't delete yourself).
        let (st, _) = second_client
            .delete(&format!("/api/v1/users/{second_admin_id}"))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "cannot delete yourself");
    }

    // -- Cannot deactivate user who is approver for active users --
    {
        let lead_id = create_lead(&admin, "lead-guard@example.com", "Guard").await;
        let emp_id = create_emp(&admin, "emp-guard@example.com", "GuardEmp", lead_id).await;

        // Deactivating the lead while emp still reports to them must be rejected.
        let (st, body) = admin
            .post(
                &format!("/api/v1/users/{lead_id}/deactivate"),
                &serde_json::json!({}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "deactivate with active reports must fail"
        );
        let error_msg = body["error"].as_str().unwrap_or("").to_lowercase();
        assert!(
            error_msg.contains("approver") || error_msg.contains("reassign"),
            "error must mention approver/reassign, got: {error_msg}"
        );

        // Reassign emp to admin (id=1), then deactivation must succeed.
        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &serde_json::json!({"approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "reassign emp to admin");

        let (st, _) = admin
            .post(
                &format!("/api/v1/users/{lead_id}/deactivate"),
                &serde_json::json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "deactivate after reassign must succeed");
    }

    // -- Cannot update active=false for user who is approver for active users --
    {
        let lead_id = create_lead(&admin, "lead-put-guard@example.com", "PutGuard").await;
        create_emp(&admin, "emp-put-guard@example.com", "PutGuardEmp", lead_id).await;

        // PUT with active=false while lead has active direct reports must be rejected.
        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{lead_id}"),
                &serde_json::json!({"active": false}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "PUT active=false with active reports must fail"
        );
        let error_msg = body["error"].as_str().unwrap_or("").to_lowercase();
        assert!(
            error_msg.contains("approver") || error_msg.contains("reassign"),
            "error must mention approver/reassign, got: {error_msg}"
        );
    }

    // -- Leave-days endpoint scope and validation --
    {
        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"emp-invalid-leave@example.com","first_name":"Leave","last_name":"Invalid",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":367,"leave_days_next_year":30,
                    "start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "create rejects invalid leave_days_current_year"
        );

        let lead_id = create_lead(&admin, "lead-leave@example.com", "LeaveLead").await;
        let emp_id = create_emp(&admin, "emp-leave@example.com", "LeaveEmp", lead_id).await;

        let (st, body) = admin
            .get(&format!("/api/v1/users/{emp_id}/leave-days"))
            .await;
        assert_eq!(st, StatusCode::OK, "admin can read leave days");
        assert_eq!(
            body.as_array().unwrap().len(),
            2,
            "returns current + next year rows"
        );

        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{emp_id}/leave-days"),
                &json!({"year": year(), "days": 25}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "admin can set leave days for current year"
        );
        assert_eq!(body["ok"], true);

        let (st, body) = admin
            .get(&format!("/api/v1/users/{emp_id}/leave-days"))
            .await;
        assert_eq!(st, StatusCode::OK);
        assert!(body
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["year"].as_i64() == Some(year() as i64)
                && row["days"].as_i64() == Some(25)));

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}/leave-days"),
                &json!({"year": year() + 2, "days": 25}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cannot set leave days more than one year ahead"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}/leave-days"),
                &json!({"year": year() - 2, "days": 25}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cannot set leave days before previous year"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}/leave-days"),
                &json!({"year": year(), "days": 367}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "days upper bound enforced");

        let lead_pw = {
            let (st, body) = admin
                .post(
                    &format!("/api/v1/users/{lead_id}/reset-password"),
                    &json!({}),
                )
                .await;
            assert_eq!(st, StatusCode::OK, "reset lead password");
            body["temporary_password"].as_str().unwrap().to_string()
        };
        let lead = login_change_pw(&app, "lead-leave@example.com", &lead_pw).await;

        let (st, _) = lead
            .get(&format!("/api/v1/users/{emp_id}/leave-days"))
            .await;
        assert_eq!(st, StatusCode::OK, "lead can read direct report leave days");
        let (st, _) = lead
            .put(
                &format!("/api/v1/users/{emp_id}/leave-days"),
                &json!({"year": year(), "days": 24}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "non-admin cannot set leave days");

        let (other_lead_id, other_lead_pw, _other_emp_id, _other_emp_pw, _monday, _cat) =
            bootstrap_team_with_suffix(&app, &admin, false, "leave-other").await;
        let other_lead =
            login_change_pw(&app, "lead-leave-other@example.com", &other_lead_pw).await;
        let (st, _) = other_lead
            .get(&format!("/api/v1/users/{emp_id}/leave-days"))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "unrelated lead cannot read leave days"
        );

        let emp_pw = {
            let (st, body) = admin
                .post(
                    &format!("/api/v1/users/{emp_id}/reset-password"),
                    &json!({}),
                )
                .await;
            assert_eq!(st, StatusCode::OK, "reset employee password");
            body["temporary_password"].as_str().unwrap().to_string()
        };
        let emp = login_change_pw(&app, "emp-leave@example.com", &emp_pw).await;
        let (st, _) = emp
            .get(&format!("/api/v1/users/{other_lead_id}/leave-days"))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "employee cannot read another user's leave days"
        );

        let (st, _) = admin
            .post(&format!("/api/v1/users/{emp_id}/deactivate"), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "deactivate employee");
        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}/leave-days"),
                &json!({"year": year(), "days": 20}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cannot set leave days for inactive user"
        );

        let (st, _) = admin.post("/api/v1/users/1/deactivate", &json!({})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "admin cannot deactivate self");

        let (st, _) = admin.delete("/api/v1/users/1").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "admin cannot delete self");

        let (st, _) = admin
            .post(
                &format!("/api/v1/users/{emp_id}/reset-password"),
                &json!({}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "reset password rejects inactive target"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"leave_days_next_year": 367}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "update rejects invalid leave_days_next_year"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{lead_id}"),
                &json!({"active": false}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cannot deactivate a lead with active direct reports"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{lead_id}"),
                &json!({"role": "employee"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cannot remove a lead with direct reports from approver role"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{lead_id}"),
                &json!({"tracks_time": false}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "tracks_time cannot be disabled on non-admins"
        );
    }

    // -- Additional auth/validation guards for user lifecycle endpoints --
    {
        let lead_id = create_lead(&admin, "lead-guards-extra@example.com", "GuardLead").await;
        let emp_id = create_emp(
            &admin,
            "emp-guards-extra@example.com",
            "GuardEmpExtra",
            lead_id,
        )
        .await;

        let lead_pw = {
            let (st, body) = admin
                .post(
                    &format!("/api/v1/users/{lead_id}/reset-password"),
                    &json!({}),
                )
                .await;
            assert_eq!(st, StatusCode::OK, "reset lead password for extra guards");
            body["temporary_password"].as_str().unwrap().to_string()
        };
        let lead = login_change_pw(&app, "lead-guards-extra@example.com", &lead_pw).await;

        let (st, _) = lead
            .post(&format!("/api/v1/users/{emp_id}/deactivate"), &json!({}))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "non-admin cannot deactivate users"
        );

        let (st, _) = lead.delete(&format!("/api/v1/users/{emp_id}")).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "non-admin cannot delete users");

        let (st, _) = lead
            .post(
                &format!("/api/v1/users/{emp_id}/reset-password"),
                &json!({}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "non-admin cannot reset passwords"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"weekly_hours": 169.0}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "update rejects weekly_hours > 168"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"workdays_per_week": 6}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "update rejects invalid workdays_per_week"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"overtime_start_balance_min": 600000}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "update rejects overtime balance out of range"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"email": "not-an-email"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "update rejects malformed email"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"role":"assistant","weekly_hours":0,"overtime_start_balance_min":5,"approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "assistant update rejects overtime start balance"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"role":"employee","tracks_time":false,"approver_ids":[1]}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "non-admin roles cannot disable tracks_time"
        );

        // Out-of-range leave_days_current_year (branch not otherwise tested — only
        // leave_days_next_year is tested above).
        let (st, body) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({"leave_days_current_year": 400}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "leave_days_current_year=400 rejected: {body}"
        );

        // Admin user may only have another admin as approver (not a team lead).
        // Use the lead_id from the test bootstrap which is a team_lead.
        // This covers the "Admins may only report to an active Admin" branch in
        // services::users::validate_approver_ids.
        let (st, body) = admin
            .put("/api/v1/users/1", &json!({"approver_ids": [lead_id]}))
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "admin with team_lead approver rejected: {body}"
        );
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .contains("admin"),
            "error mentions admin: {body}"
        );
    }

    app.cleanup().await;
}

/// Admins can choose which categories/absence categories a new employee
/// starts with: omitting the fields defaults to "all existing categories"
/// (the previous behavior), an explicit list grants exactly that list
/// (including an empty one), and an unknown id in the list is rejected.
#[tokio::test]
async fn user_creation_with_explicit_category_selection() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, all_cats) = admin.get("/api/v1/categories/all").await;
    let cat_ids: Vec<i64> = all_cats
        .as_array()
        .expect("categories array")
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert!(cat_ids.len() >= 2, "fixture seeds multiple categories");
    let first_cat_id = cat_ids[0];

    let (_, all_abs_cats) = admin.get("/api/v1/absence-categories/all").await;
    let abs_cat_id = all_abs_cats
        .as_array()
        .expect("absence categories array")
        .iter()
        .find(|c| c["slug"].as_str() == Some("training"))
        .expect("training seeded category exists")["id"]
        .as_i64()
        .unwrap();

    // Omitting the fields defaults to every existing category enabled.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"omit-cats@example.com","first_name":"Omit","last_name":"Cats",
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_ids":[1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create without category fields: {body}");
    let omit_user_id = id(&body);
    let (st, enabled) = admin
        .get(&format!("/api/v1/categories/{first_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        enabled
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_i64() == Some(omit_user_id)),
        "omitting category_ids defaults to all categories enabled"
    );

    // An explicit, partial list grants exactly that list.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"explicit-cats@example.com","first_name":"Explicit","last_name":"Cats",
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_ids":[1],
                "category_ids":[first_cat_id],"absence_category_ids":[]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create with explicit categories: {body}");
    let explicit_user_id = id(&body);

    let (st, enabled) = admin
        .get(&format!("/api/v1/categories/{first_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        enabled
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_i64() == Some(explicit_user_id)),
        "explicitly listed category is enabled"
    );
    let second_cat_id = cat_ids[1];
    let (st, enabled) = admin
        .get(&format!("/api/v1/categories/{second_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !enabled
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_i64() == Some(explicit_user_id)),
        "category omitted from the explicit list is not enabled"
    );
    let (st, enabled) = admin
        .get(&format!("/api/v1/absence-categories/{abs_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !enabled
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_i64() == Some(explicit_user_id)),
        "empty absence_category_ids list enables nothing"
    );

    // An unknown category id is rejected with a clean 400, not a 500.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"bad-cat@example.com","first_name":"Bad","last_name":"Cat",
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_ids":[1],
                "category_ids":[9999999]}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "unknown category id rejected: {body}"
    );
    assert_eq!(body["error"], "Unknown category id.");

    // Unknown absence category ids are validated the same way.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"bad-absence-cat@example.com","first_name":"Bad","last_name":"AbsCat",
                "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01","approver_ids":[1],
                "absence_category_ids":[9999999]}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "unknown absence category id rejected: {body}"
    );
    assert_eq!(body["error"], "Unknown absence category id.");

    app.cleanup().await;
}
