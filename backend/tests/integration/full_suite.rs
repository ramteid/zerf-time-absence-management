//! End-to-end integration tests covering the full application surface.
//!
//! Each test function is fully independent: it spawns its own database and
//! server instance, sets up the required users/state via helpers, and tears
//! down automatically when the function returns.
//!
//! Splitting into focused functions keeps failure messages local and allows
//! the test runner to execute them in parallel.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

// ============================================================================
// 1. Auth, user-management and RBAC
// ============================================================================

#[tokio::test]
async fn auth_and_rbac_workflow() {
    let app = TestApp::spawn().await;

    // -- Anonymous endpoints --------------------------------------------------
    {
        let c = app.client();
        let (st, _) = c.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "/auth/me unauth");
        let (st, _) = c.get("/api/v1/users").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "/users unauth");
        let (st, _) = c.login("admin@example.com", "WRONG").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "bad login rejected");
    }

    // -- Admin login + forced password change ---------------------------------
    let admin = app.client();
    {
        let (st, _) = admin.login("admin@example.com", &app.admin_password).await;
        assert_eq!(st, StatusCode::OK, "login admin");
        let (st, body) = admin.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["must_change_password"], true, "must_change_password set");
        let (st, _) = admin
            .change_password(&app.admin_password, "AdminPass!234")
            .await;
        assert_eq!(st, StatusCode::OK, "change pw");
        let (st, body) = admin.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["must_change_password"], false, "flag cleared");
    }

    // -- Default seed data ----------------------------------------------------
    let cat_id: i64;
    {
        let (st, body) = admin.get("/api/v1/categories").await;
        assert_eq!(st, StatusCode::OK);
        let count = count_ids(&body);
        assert!(count >= 6, "≥6 categories (got {})", count);
        cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();

        let yr = year();
        let (st, body) = admin.get(&format!("/api/v1/holidays?year={}", yr)).await;
        assert_eq!(st, StatusCode::OK);
        let hc = count_ids(&body);
        assert!(hc >= 9, "≥9 BW holidays (got {})", hc);
    }

    // -- User management ------------------------------------------------------
    let emp_id: i64;
    let emp_pw: String;
    let lead_id: i64;
    let lead_pw: String;
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"lead@example.com","first_name":"Lea","last_name":"Lead","role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,"start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create team_lead");
        lead_id = id(&body);
        lead_pw = temp_pw(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"erin@example.com","first_name":"Erin","last_name":"Worker","role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,"start_date":"2024-01-01","approver_ids": [lead_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create employee");
        emp_id = id(&body);
        emp_pw = temp_pw(&body);

        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"erin@example.com","first_name":"Dup","last_name":"Dup","role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,"start_date":"2024-01-01"}),
            )
            .await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::CONFLICT,
            "duplicate email rejected (got {})",
            st
        );
    }

    let emp = app.client();
    {
        let (st, _) = emp.login("erin@example.com", &emp_pw).await;
        assert_eq!(st, StatusCode::OK, "login emp");
        let (st, _) = emp.change_password(&emp_pw, "EmployeePass!234").await;
        assert_eq!(st, StatusCode::OK, "emp change pw");
    }

    let lead = app.client();
    {
        let (st, _) = lead.login("lead@example.com", &lead_pw).await;
        assert_eq!(st, StatusCode::OK, "login lead");
        let (st, _) = lead.change_password(&lead_pw, "TeamLeadPass!234").await;
        assert_eq!(st, StatusCode::OK, "lead change pw");
    }

    // -- Role-elevation hardening ---------------------------------------------
    {
        let (st, _) = emp
            .put(&format!("/api/v1/users/{}", emp_id), &json!({"role":"admin"}))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp self-promote 403");

        let (st, _) = admin.put("/api/v1/users/1", &json!({"role":"employee"})).await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::CONFLICT,
            "admin self-demote rejected (got {})",
            st
        );

        let (st, _) = admin.put("/api/v1/users/1", &json!({"active":false})).await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::CONFLICT,
            "admin self-deactivate rejected (got {})",
            st
        );

        let (st, _) = admin
            .put(&format!("/api/v1/users/{}", emp_id), &json!({"role":"superuser"}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "bogus role rejected");
    }

    // -- RBAC -----------------------------------------------------------------
    {
        let (st, _) = emp.get("/api/v1/users").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp /users 403");

        let (st, _) = emp.get("/api/v1/audit-log").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp /audit 403");

        let (st, _) = lead
            .post(
                "/api/v1/users",
                &json!({"email":"x@example.com","first_name":"X","last_name":"X","role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,"start_date":"2024-01-01"}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "lead create user 403");

        let (st, _) = lead
            .post("/api/v1/categories", &json!({"name":"X","color":"#000"}))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "lead create category 403");
    }

    // -- Password reset + deactivation ----------------------------------------
    {
        let (st, body) = admin
            .post(&format!("/api/v1/users/{}/reset-password", emp_id), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "reset password");
        let new_pw = temp_pw(&body);
        assert!(!new_pw.is_empty(), "new temp pw issued");

        let emp2 = app.client();
        let (st, _) = emp2.login("erin@example.com", &new_pw).await;
        assert_eq!(st, StatusCode::OK, "login with reset pw");

        let (st, _) = admin
            .post(&format!("/api/v1/users/{}/deactivate", emp_id), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "deactivate user");

        let emp3 = app.client();
        let (st, _) = emp3.login("erin@example.com", "EmployeePass!234").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "deactivated login rejected");
    }

    // -- Logout invalidates session --------------------------------------------
    {
        let (st, _) = admin.post("/api/v1/auth/logout", &json!({})).await;
        assert_eq!(st, StatusCode::OK, "logout");
        let (st, _) = admin.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "me after logout");
    }

    // suppress unused-variable warning for cat_id (needed to confirm seed data)
    let _ = cat_id;

    app.cleanup().await;
}

// ============================================================================
// 2. Time-entry validation, submit/approve workflow and change requests
// ============================================================================

#[tokio::test]
async fn time_entry_and_cr_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _, cat_id) = bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let entry_day = date_offset(-1);
    let future_day = date_offset(5);

    // -- Time entry validations -----------------------------------------------
    let te1: i64;
    let te2: i64;
    {
        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({"entry_date": &entry_day, "start_time":"08:00","end_time":"12:00","category_id": cat_id, "comment":"morning"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create entry 1");
        te1 = id(&body);

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({"entry_date": &entry_day, "start_time":"10:00","end_time":"11:00","category_id": cat_id}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "overlap rejected");

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({"entry_date": &entry_day, "start_time":"14:00","end_time":"13:00","category_id": cat_id}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "end<start rejected");

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({"entry_date": &future_day, "start_time":"08:00","end_time":"09:00","category_id": cat_id}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "future date rejected");

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({"entry_date": &entry_day, "start_time":"13:00","end_time":"15:00","category_id": cat_id}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create entry 2");
        te2 = id(&body);

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({"entry_date": &entry_day, "start_time":"15:00","end_time":"23:30","category_id": cat_id}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, ">14h day rejected");

        let (st, body) = emp
            .get(&format!("/api/v1/time-entries?from={}&to={}", entry_day, entry_day))
            .await;
        assert_eq!(st, StatusCode::OK, "list own entries");
        assert!(has_id(&body, te1), "TE1 in list");
    }

    // -- Submit + approve workflow --------------------------------------------
    {
        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [te1, te2]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit");

        let (st, _) = emp
            .put(
                &format!("/api/v1/time-entries/{}", te1),
                &json!({"entry_date": &entry_day, "start_time":"08:00","end_time":"11:00","category_id": cat_id, "comment":"x"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "edit submitted entry rejected");

        let (st, body) = lead
            .post("/api/v1/time-entries/batch-approve", &json!({"ids": [te1]}))
            .await;
        assert_eq!(st, StatusCode::OK, "lead approve TE1");
        assert_eq!(body["count"], 1);

        let (st, body) = lead
            .post(
                "/api/v1/time-entries/batch-reject",
                &json!({"ids": [te2], "reason":"clarify"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead reject TE2");
        assert_eq!(body["count"], 1);

        let (st, _) = emp
            .post("/api/v1/time-entries/batch-approve", &json!({"ids": [te1]}))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp approve forbidden");
    }

    // -- Audit log contains entries for employee and reviewer actions ---------
    {
        let (st, body) = admin
            .get(&format!("/api/v1/audit-log?user_id={}", emp_id))
            .await;
        assert_eq!(st, StatusCode::OK, "audit log");
        let lc = count_ids(&body);
        assert!(lc >= 4, "employee audit entries={} (need >=4)", lc);

        let (st, body) = admin
            .get(&format!("/api/v1/audit-log?user_id={}", lead_id))
            .await;
        assert_eq!(st, StatusCode::OK, "lead audit log");
        let reviewer_entries = serde_json::to_string(&body)
            .unwrap()
            .matches("\"table_name\":\"time_entries\"")
            .count();
        assert!(
            reviewer_entries >= 2,
            "reviewer time-entry audit entries={} (need >=2)",
            reviewer_entries
        );
    }

    app.cleanup().await;
}

// ============================================================================
// 3. Absence workflows and reports (vacation, sick leave, general absence)
// ============================================================================

#[tokio::test]
async fn absence_and_report_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // -- Vacation: request, approve, balance ----------------------------------
    let v_from = next_monday(10);
    let v_to = v_from + chrono::Duration::days(2);
    let abs_id: i64;
    let balance_after_vacation: serde_json::Value;
    {
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": v_from.to_string(),"end_date": v_to.to_string()}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "request vacation");
        abs_id = id(&body);
        assert_eq!(body["status"], "requested");

        // Sick: auto-approved. Extend to next_monday(0) so range always has a workday.
        let sick_end = next_monday(0).to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"sick","start_date": &today(),"end_date": &sick_end}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "report sick");
        assert_eq!(body["status"], "approved", "sick auto-approved");

        // Overlap rejected.
        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": v_from.to_string(),"end_date": v_from.to_string()}),
            )
            .await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::CONFLICT,
            "overlap rejected (got {})",
            st
        );

        // Invalid kind rejected.
        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"holiday","start_date": v_from.to_string(),"end_date": v_from.to_string()}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid kind rejected");

        let (st, _) = lead
            .post(&format!("/api/v1/absences/{}/approve", abs_id), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "approve vacation");

        let (_, bal) = emp
            .get(&format!("/api/v1/leave-balance/{}?year={}", emp_id, year()))
            .await;
        balance_after_vacation = bal;
    }

    // -- General absence: happy path ------------------------------------------
    let ga_from = next_monday(30);
    let ga_to = ga_from + chrono::Duration::days(4);
    let ga_to2 = ga_from + chrono::Duration::days(10);
    let ga_month = ga_from.format("%Y-%m").to_string();
    let gabs: i64;
    {
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": ga_from.to_string(),"end_date": ga_to.to_string(),"comment":"parental leave"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "POST general_absence");
        gabs = id(&body);
        assert_eq!(body["kind"], "general_absence");
        assert_eq!(body["status"], "requested");
        assert_eq!(body["comment"], "parental leave");

        let yr = ga_from.format("%Y").to_string();
        let (_, body) = emp.get(&format!("/api/v1/absences?year={}", yr)).await;
        assert!(has_id(&body, gabs), "shows in own list");

        let (_, body) = lead.get("/api/v1/absences/all?status=requested").await;
        assert!(has_id(&body, gabs), "appears in lead queue");

        let (st, _) = emp.get("/api/v1/absences/all").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp /absences/all 403");

        // Edit while pending.
        let (st, body) = emp
            .put(
                &format!("/api/v1/absences/{}", gabs),
                &json!({"kind":"general_absence","start_date": ga_from.to_string(),"end_date": ga_to2.to_string(),"comment":"updated"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "edit pending general_absence");
        assert_eq!(body["end_date"], ga_to2.to_string());

        // Lead approves.
        let (st, _) = lead
            .post(&format!("/api/v1/absences/{}/approve", gabs), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "lead approve");

        let (_, body) = emp.get(&format!("/api/v1/absences?year={}", yr)).await;
        let ga_obj = find_by_id(&body, gabs).expect("GA not found");
        assert_eq!(ga_obj["status"], "approved");

        // No edit after approval.
        let (st, _) = emp
            .put(
                &format!("/api/v1/absences/{}", gabs),
                &json!({"kind":"general_absence","start_date": ga_from.to_string(),"end_date": ga_to.to_string(),"comment":"x"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "edit approved rejected");

        // Cancel triggers cancellation-approval workflow.
        let (st, body) = emp.delete(&format!("/api/v1/absences/{}", gabs)).await;
        assert_eq!(st, StatusCode::OK, "cancel approved triggers workflow");
        assert_eq!(body["pending"], true);

        // Reject the cancellation to restore.
        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{}/reject-cancellation", gabs),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "reject cancellation to restore");

        // Calendar shows it.
        let (_, body) = lead
            .get(&format!("/api/v1/absences/calendar?month={}", ga_month))
            .await;
        assert!(
            serde_json::to_string(&body)
                .unwrap()
                .contains("\"general_absence\""),
            "calendar shows general_absence"
        );

        // Vacation balance unchanged by general absence.
        let (_, body) = emp
            .get(&format!("/api/v1/leave-balance/{}?year={}", emp_id, year()))
            .await;
        assert_eq!(body["annual_entitlement"], 30);
        assert_eq!(
            body["available"], balance_after_vacation["available"],
            "balance unchanged after general_absence"
        );

        // Monthly report includes it.
        let (_, body) = emp
            .get(&format!("/api/v1/reports/month?month={}", ga_month))
            .await;
        assert!(
            serde_json::to_string(&body)
                .unwrap()
                .contains("\"general_absence\""),
            "monthly report flags day"
        );

        // Audit log has ≥3 entries for this absence.
        let (_, body) = admin
            .get(&format!("/api/v1/audit-log?user_id={}", emp_id))
            .await;
        let ga_audit = serde_json::to_string(&body)
            .unwrap()
            .matches(&format!("\"record_id\":{}", gabs))
            .count();
        assert!(ga_audit >= 3, "audit has {} entries for absence (need ≥3)", ga_audit);
    }

    // -- General absence: overlap and validation edge cases -------------------
    {
        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": ga_from.to_string(),"end_date": ga_from.to_string()}),
            )
            .await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::CONFLICT,
            "overlap with approved GA rejected (got {})",
            st
        );

        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": ga_from.to_string(),"end_date": ga_from.to_string()}),
            )
            .await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::CONFLICT,
            "vacation overlapping GA rejected (got {})",
            st
        );

        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date":"2099-01-10","end_date":"2099-01-05"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "inverted range rejected");

        // Pending absences can change type.
        // Use next_monday(50) to avoid holidays like Easter. 50 weeks from Jan 7 = late March.
        let editable_day = next_monday(50).format("%Y-%m-%d").to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": &editable_day,"end_date": &editable_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create editable pending absence");
        let editable_id = id(&body);

        let (st, body) = emp
            .put(
                &format!("/api/v1/absences/{}", editable_id),
                &json!({"kind":"vacation","start_date": &editable_day,"end_date": &editable_day,"comment":"converted"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "edit pending kind");
        assert_eq!(body["kind"], "vacation");

        let (st, _) = emp.delete(&format!("/api/v1/absences/{}", editable_id)).await;
        assert_eq!(st, StatusCode::OK, "cancel edited pending");

        // Approved sick cannot change type. Use offset 55 to avoid holidays.
        let sick_edit_day = next_monday(55).format("%Y-%m-%d").to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"sick","start_date": &sick_edit_day,"end_date": &sick_edit_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create sick for edit test");
        let editable_sick = id(&body);

        let (st, _) = emp
            .put(
                &format!("/api/v1/absences/{}", editable_sick),
                &json!({"kind":"vacation","start_date": &sick_edit_day,"end_date": &sick_edit_day}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "approved sick kind change rejected");

        // Unauthenticated caller rejected.
        let anon = app.client();
        let anon_day = next_monday(90).format("%Y-%m-%d").to_string();
        let (st, _) = anon
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": &anon_day,"end_date": &anon_day}),
            )
            .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "anon create rejected");

        // Non-allowlisted kind rejected.
        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"parental","start_date": &anon_day,"end_date": &anon_day}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "non-allowlisted kind rejected");

        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"","start_date": &anon_day,"end_date": &anon_day}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "empty kind rejected");
    }

    // -- General absence: cancel, reject and RBAC journeys -------------------
    {
        // Use offset 60 to avoid holidays.
        let ga4_monday = next_monday(60);
        let ga4_from = ga4_monday.format("%Y-%m-%d").to_string();
        let ga4_to = (ga4_monday + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": &ga4_from,"end_date": &ga4_to}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        let gabs4 = id(&body);

        let (st, _) = emp.delete(&format!("/api/v1/absences/{}", gabs4)).await;
        assert_eq!(st, StatusCode::OK, "employee cancels own pending request");

        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": &ga4_from,"end_date": &ga4_to}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "re-request after cancel allowed");

        // Reject journey.
        let ga5_monday = next_monday(200);
        let ga5_from = ga5_monday.format("%Y-%m-%d").to_string();
        let ga5_to = (ga5_monday + chrono::Duration::days(2)).format("%Y-%m-%d").to_string();

        let (_, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": &ga5_from,"end_date": &ga5_to}),
            )
            .await;
        let gabs5 = id(&body);

        let (st, _) = emp
            .post(&format!("/api/v1/absences/{}/approve", gabs5), &json!({}))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp self-approve 403");

        let (st, _) = emp
            .post(
                &format!("/api/v1/absences/{}/reject", gabs5),
                &json!({"reason":"nope"}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "emp reject 403");

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{}/reject", gabs5),
                &json!({"reason":""}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "empty reject reason rejected");

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{}/reject", gabs5),
                &json!({"reason":"Need more documentation."}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead reject general_absence");

        let (st, _) = emp.delete(&format!("/api/v1/absences/{}", gabs5)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "cancel-after-reject rejected");

        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"general_absence","start_date": &ga5_from,"end_date": &ga5_to}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "re-request after reject allowed");

        let (st, _) = lead
            .post("/api/v1/absences/9999999/approve", &json!({}))
            .await;
        assert_ne!(st, StatusCode::OK, "approve unknown id not 200 (got {})", st);
    }

    // -- Vacation balance matches captured value ------------------------------
    {
        let (st, body) = emp
            .get(&format!("/api/v1/leave-balance/{}?year={}", emp_id, year()))
            .await;
        assert_eq!(st, StatusCode::OK, "leave balance");
        assert_eq!(body["annual_entitlement"], 30);
        assert_eq!(body["approved_upcoming"], balance_after_vacation["approved_upcoming"]);
        assert_eq!(body["available"], balance_after_vacation["available"]);
    }

    // -- Reports --------------------------------------------------------------
    {
        let month = reference_date().format("%Y-%m").to_string();
        let yr = year();

        let (st, _) = lead
            .get(&format!("/api/v1/absences/calendar?month={}", month))
            .await;
        assert_eq!(st, StatusCode::OK, "calendar");

        let (st, _) = lead
            .get(&format!("/api/v1/reports/month?user_id={}&month={}", emp_id, month))
            .await;
        assert_eq!(st, StatusCode::OK, "monthly report");

        let (st, _) = lead.get(&format!("/api/v1/reports/team?month={}", month)).await;
        assert_eq!(st, StatusCode::OK, "team report");

        let (st, _) = lead
            .get(&format!("/api/v1/reports/categories?from={}-01-01&to={}-12-31", yr, yr))
            .await;
        assert_eq!(st, StatusCode::OK, "category report");

        let (st, _) = lead
            .get(&format!("/api/v1/reports/overtime?user_id={}&year={}", emp_id, yr))
            .await;
        assert_eq!(st, StatusCode::OK, "overtime report");

        let (st, csv_body) = lead
            .get_raw(&format!("/api/v1/reports/month/csv?user_id={}&month={}", emp_id, month))
            .await;
        assert_eq!(st, StatusCode::OK, "CSV export");
        assert!(csv_body.len() > 100, "CSV has content (len={})", csv_body.len());
    }

    app.cleanup().await;
}

// ============================================================================
// 4. Comprehensive multi-user time-entry journey (Tina)
// ============================================================================

#[tokio::test]
async fn tina_time_tracking_journey() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    // emp is used only for admin-ops assertions at the end
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // Create Tina.
    let tina_id: i64;
    let tina_pw: String;
    {
        let (_, lead_id_body) = admin.get("/api/v1/users").await;
        let lead_id: i64 = lead_id_body
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["email"].as_str() == Some("lead-r@example.com"))
            .unwrap()["id"]
            .as_i64()
            .unwrap();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"tina@example.com","first_name":"Tina","last_name":"Timekeeper","role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,"start_date":"2024-01-01","approver_ids": [lead_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create Tina");
        tina_id = id(&body);
        tina_pw = temp_pw(&body);
    }

    let tina = app.client();
    {
        let (st, _) = tina.login("tina@example.com", &tina_pw).await;
        assert_eq!(st, StatusCode::OK, "tina login");

        let (st, body) = tina.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["must_change_password"], true, "tina forced pw flag");

        let tina2 = app.client();
        let (st, _) = tina2.login("tina@example.com", &tina_pw).await;
        assert_eq!(st, StatusCode::OK, "second login OK while pw-flagged");

        let (st, _) = tina.change_password(&tina_pw, "short").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "weak pw rejected");

        let (st, _) = tina.change_password(&tina_pw, "TinaPass!234").await;
        assert_eq!(st, StatusCode::OK, "tina change pw");

        let (st, body) = tina.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["must_change_password"], false, "flag cleared");

        let (st, _) = tina
            .change_password("WRONG-WRONG-WRONG", "AnotherPass!234")
            .await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::UNAUTHORIZED,
            "wrong current pw rejected (got {})",
            st
        );
    }

    // Resolve category IDs.
    let cat_core: i64;
    let cat_prep: i64;
    let cat_lead_cat: i64;
    let cat_meet: i64;
    let cat_other: i64;
    {
        let (_, body) = tina.get("/api/v1/categories").await;
        let cats = body.as_array().expect("categories array");
        let find_cat = |name: &str| -> i64 {
            cats.iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("category '{}' not found", name))["id"]
                .as_i64()
                .unwrap()
        };
        cat_core = find_cat("Core Duties");
        cat_prep = find_cat("Preparation Time");
        cat_lead_cat = find_cat("Leadership Tasks");
        cat_meet = find_cat("Team Meeting");
        cat_other = find_cat("Other");
    }

    let yday = date_offset(-1);
    let day2 = date_offset(-2);
    let day3 = date_offset(-3);
    let day7 = date_offset(-7);
    let today_s = today();
    let tina_month = (reference_date() + chrono::Duration::days(-1))
        .format("%Y-%m")
        .to_string();

    // -- 1. Multi-category workday (yesterday) --------------------------------
    let id_y1: i64;
    let id_y2: i64;
    let id_y3: i64;
    let id_y4: i64;
    {
        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"08:00","end_time":"10:00","category_id": cat_core, "comment":"focused work"}))
            .await;
        assert_eq!(st, StatusCode::OK, "Y core duties 08-10");
        id_y1 = id(&body);

        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"10:00","end_time":"10:30","category_id": cat_meet, "comment":"standup"}))
            .await;
        assert_eq!(st, StatusCode::OK, "Y meeting 10-10:30");
        id_y2 = id(&body);

        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"10:30","end_time":"12:00","category_id": cat_core, "comment":"follow-up"}))
            .await;
        assert_eq!(st, StatusCode::OK, "Y core duties 10:30-12");
        id_y3 = id(&body);

        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"13:00","end_time":"16:30","category_id": cat_prep, "comment":"prep — Übung mit Ümlaut 🎨"}))
            .await;
        assert_eq!(st, StatusCode::OK, "Y prep 13-16:30 (unicode+emoji)");
        id_y4 = id(&body);
    }

    // -- 2. Overlap and boundary edge cases on yesterday ----------------------
    let id_y5: i64;
    let id_y6: i64;
    {
        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"08:00","end_time":"10:00","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "exact-duplicate overlap");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"09:00","end_time":"11:00","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "partial overlap");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"09:59","end_time":"10:01","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "one-minute overlap");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"14:00","end_time":"15:00","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "contained overlap");

        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"12:00","end_time":"13:00","category_id": cat_core, "comment":"coverage"}))
            .await;
        assert_eq!(st, StatusCode::OK, "adjacent 12-13 fills gap");
        id_y5 = id(&body);

        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"16:30:00","end_time":"17:00:00","category_id": cat_other, "comment":"clean-up"}))
            .await;
        assert_eq!(st, StatusCode::OK, "HH:MM:SS accepted");
        id_y6 = id(&body);

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"17:00","end_time":"17:00","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "zero-length rejected");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"18:00","end_time":"17:30","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "inverted times rejected");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"25:00","end_time":"26:00","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "out-of-range hour rejected");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"ab:cd","end_time":"ef:gh","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "garbage time rejected");

        let fut = date_offset(2);
        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &fut, "start_time":"08:00","end_time":"09:00","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "future date rejected");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"19:00","end_time":"19:30","category_id": 999999}))
            .await;
        assert_ne!(st, StatusCode::OK, "bogus category rejected (got {})", st);

        let (st, _) = tina.post_raw("/api/v1/time-entries", "{not-json").await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::UNPROCESSABLE_ENTITY,
            "malformed JSON rejected (got {})",
            st
        );
    }

    // -- 3. 14h day-cap edge cases --------------------------------------------
    let id_c1: i64;
    {
        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &day2, "start_time":"06:00","end_time":"20:00","category_id": cat_core, "comment":"long shift"}))
            .await;
        assert_eq!(st, StatusCode::OK, "exactly 14h allowed");
        id_c1 = id(&body);

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &day2, "start_time":"20:00","end_time":"20:01","category_id": cat_other}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, ">14h day total rejected");

        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &day3, "start_time":"05:00","end_time":"19:30","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "single 14:30 entry rejected");
    }

    // -- 4. Long comment ------------------------------------------------------
    {
        let long: String = "x".repeat(2000);
        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &day3, "start_time":"08:00","end_time":"08:30","category_id": cat_other, "comment": &long}))
            .await;
        assert!(
            st == StatusCode::OK || st == StatusCode::BAD_REQUEST,
            "long comment handled gracefully (got {})",
            st
        );
    }

    // -- 5. Listing and filtering ---------------------------------------------
    {
        let (st, body) = tina
            .get(&format!("/api/v1/time-entries?from={}&to={}", yday, yday))
            .await;
        assert_eq!(st, StatusCode::OK);
        assert!(count_ids(&body) >= 6, "yesterday list has ≥6");

        let (_, body) = tina
            .get(&format!("/api/v1/time-entries?from={}&to={}", day7, today_s))
            .await;
        assert!(has_id(&body, id_y1), "wide range includes Y1");
        assert!(has_id(&body, id_c1), "wide range includes 14h block");

        let body_str = serde_json::to_string(&body).unwrap();
        assert!(
            !body_str.contains(&format!("\"user_id\":{}", emp_id)),
            "no cross-user leakage"
        );

        let (st, _) = tina.get("/api/v1/time-entries/all").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "tina /all 403");
    }

    // -- 6. Edit drafts then submit -------------------------------------------
    let id_y2b: i64;
    {
        let (st, _) = tina
            .put(&format!("/api/v1/time-entries/{}", id_y4), &json!({"entry_date": &yday, "start_time":"13:00","end_time":"17:00","category_id": cat_prep, "comment":"extended"}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "edit causing overlap rejected");

        let (st, _) = tina
            .put(&format!("/api/v1/time-entries/{}", id_y4), &json!({"entry_date": &yday, "start_time":"13:00","end_time":"16:00","category_id": cat_prep, "comment":"shorter"}))
            .await;
        assert_eq!(st, StatusCode::OK, "valid draft edit");

        let (st, _) = tina
            .delete(&format!("/api/v1/time-entries/{}", id_y2))
            .await;
        assert_eq!(st, StatusCode::OK, "delete draft OK");

        let (st, body) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"10:00","end_time":"10:30","category_id": cat_meet, "comment":"standup redo"}))
            .await;
        assert_eq!(st, StatusCode::OK, "re-create deleted slot");
        id_y2b = id(&body);

        let (st, _) = tina
            .post("/api/v1/time-entries/submit", &json!({"ids": [id_y1, id_y3, id_y4, id_y5, id_y6, id_y2b]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit batch");

        let (st, _) = tina
            .put(&format!("/api/v1/time-entries/{}", id_y1), &json!({"entry_date": &yday, "start_time":"08:00","end_time":"09:30","category_id": cat_core}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "edit submitted rejected");

        let (st, _) = tina.delete(&format!("/api/v1/time-entries/{}", id_y1)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "delete submitted rejected");

        let (st, _) = tina
            .post("/api/v1/time-entries/submit", &json!({"ids": [id_y1]}))
            .await;
        assert_eq!(st, StatusCode::OK, "re-submit no-op");
    }

    // -- 7. Lead reviews ------------------------------------------------------
    {
        let (st, _) = lead
            .post("/api/v1/time-entries/batch-reject", &json!({"ids": [id_y1], "reason":"   "}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "empty reject reason rejected");

        let (st, body) = lead
            .post("/api/v1/time-entries/batch-reject", &json!({"ids": [id_y1], "reason":"please add a comment"}))
            .await;
        assert_eq!(st, StatusCode::OK, "lead rejects Y1");
        assert_eq!(body["count"], 1);

        let (st, body) = lead
            .post("/api/v1/time-entries/batch-approve", &json!({"ids": [id_y3, id_y4, id_y5, id_y6, id_y2b]}))
            .await;
        assert_eq!(st, StatusCode::OK, "batch approve");
        assert_eq!(body["count"], 5, "exactly 5 approved");

        let (_, body) = lead
            .post("/api/v1/time-entries/batch-approve", &json!({"ids": [id_y1]}))
            .await;
        assert_eq!(body["count"], 0, "rejected entry not re-approved");

        let (_, body) = tina
            .get(&format!("/api/v1/time-entries?from={}&to={}", yday, yday))
            .await;
        let y3_obj = find_by_id(&body, id_y3).expect("Y3 not in list");
        assert_eq!(y3_obj["status"], "approved");

        let approved = body.as_array().unwrap().iter().filter(|e| e["status"] == "approved").count();
        assert!(approved >= 5, "≥5 approved on {} (got {})", yday, approved);
    }

    // -- 8. Self-review hardening (lead cannot approve their own entries) ------
    {
        let (st, body) = lead
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"06:00","end_time":"07:00","category_id": cat_lead_cat}))
            .await;
        assert_eq!(st, StatusCode::OK);
        let lea_te_id = id(&body);

        let (st, _) = lead
            .post("/api/v1/time-entries/submit", &json!({"ids": [lea_te_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "lea submit own");

        let (st, body) = lead
            .post("/api/v1/time-entries/batch-approve", &json!({"ids": [lea_te_id]}))
            .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["count"], 0, "lea self-approve skipped");

        let (st, body) = admin
            .post("/api/v1/time-entries/batch-approve", &json!({"ids": [lea_te_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "admin approves lead entry");
        assert_eq!(body["count"], 1);
    }

    // -- 9. Submitted entries are immutable until a reopen request lands ------
    // A submitted week is treated atomically: individual entries inside it
    // cannot be edited, and the per-entry change-request endpoint no longer
    // exists.  Direct PUTs on a non-draft entry must therefore fail.
    {
        let (st, _) = tina
            .put(
                &format!("/api/v1/time-entries/{}", id_y3),
                &json!({"entry_date": &yday, "start_time":"10:30","end_time":"11:45","category_id": cat_prep, "comment":"reclassified"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "direct PUT on submitted entry rejected"
        );

        // The /api/v1/change-requests endpoint is no longer routed.
        let (st, _) = tina
            .post(
                "/api/v1/change-requests",
                &json!({"time_entry_id": id_y3, "new_end_time":"12:30", "reason":"forgot 30 min"}),
            )
            .await;
        assert!(
            st == StatusCode::NOT_FOUND || st == StatusCode::METHOD_NOT_ALLOWED,
            "change-requests endpoint no longer accepts requests (got {})",
            st
        );
    }

    // -- 10. Reports reflect Tina's data --------------------------------------
    {
        let (st, body) = lead
            .get(&format!("/api/v1/reports/month?user_id={}&month={}", tina_id, tina_month))
            .await;
        assert_eq!(st, StatusCode::OK, "tina monthly report");
        assert!(
            serde_json::to_string(&body).unwrap().contains(&yday),
            "report mentions {}",
            yday
        );

        let (st, body) = lead
            .get(&format!("/api/v1/reports/categories?from={}&to={}", day7, today_s))
            .await;
        assert_eq!(st, StatusCode::OK, "category report");
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(body_str.contains("Core Duties"));
        assert!(body_str.contains("Preparation Time"));
    }

    // -- 11. Logout invalidates session ---------------------------------------
    {
        let (st, _) = tina.post("/api/v1/auth/logout", &json!({})).await;
        assert_eq!(st, StatusCode::OK, "tina logout");
        let (st, _) = tina.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "tina /me 401 after logout");
        let (st, _) = tina
            .post("/api/v1/time-entries", &json!({"entry_date": &yday, "start_time":"21:00","end_time":"21:30","category_id": cat_other}))
            .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "post-logout create rejected");
    }

    // -- 12. Audit log --------------------------------------------------------
    {
        let (st, _) = admin
            .get(&format!("/api/v1/audit-log?user_id={}", emp_id))
            .await;
        assert_eq!(st, StatusCode::OK, "audit log query for emp");

        let (_, body) = admin
            .get(&format!("/api/v1/audit-log?user_id={}", tina_id))
            .await;
        assert!(count_ids(&body) > 15, "tina has >15 audit entries (got {})", count_ids(&body));
    }

    // -- 13. Admin ops: password reset and deactivation -----------------------
    {
        let (st, body) = admin
            .post(&format!("/api/v1/users/{}/reset-password", emp_id), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "reset password");
        let new_pw = temp_pw(&body);
        assert!(!new_pw.is_empty());

        let emp2 = app.client();
        let (st, _) = emp2.login("emp-r@example.com", &new_pw).await;
        assert_eq!(st, StatusCode::OK, "login with reset pw");

        let (st, _) = admin
            .post(&format!("/api/v1/users/{}/deactivate", emp_id), &json!({}))
            .await;
        assert_eq!(st, StatusCode::OK, "deactivate user");

        let emp3 = app.client();
        let (st, _) = emp3.login("emp-r@example.com", "GoodPass!234").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "deactivated login rejected");
    }

    // suppress unused-variable warnings for variables not re-used after their section
    let _ = emp;

    app.cleanup().await;
}
