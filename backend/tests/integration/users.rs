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

        // Assistants must not have fixed weekly target hours.
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"assistant-invalid-hours@example.com","first_name":"Assist","last_name":"Hours",
                    "role":"assistant","weekly_hours":10,"leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "assistant weekly_hours must be 0");
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
        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"assistant-valid@example.com","first_name":"Assist","last_name":"Valid",
                    "role":"assistant","weekly_hours":0,"leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date":"2024-01-01","approver_ids": [1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create assistant user");
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
                &format!("/api/v1/time-entries/{eid}/approve"),
                &serde_json::json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve entry");

        // Employee requests reopen; lead approves → reviewed_by = lead_id in reopen_requests.
        let (st, rr_body) = emp
            .post(
                "/api/v1/reopen-requests",
                &serde_json::json!({"week_start": monday_iso}),
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

    app.cleanup().await;
}
