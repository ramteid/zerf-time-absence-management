//! End-to-end absence workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use std::collections::HashSet;

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{
    admin_login, bootstrap_team, id, login_change_pw, next_monday, reference_date, temp_pw,
};

#[tokio::test]
async fn absences_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _, cat_id) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    // -- Non-sick absence can be requested even with logged time; approval is blocked --
    // Time-entry conflicts are checked at approval, not at request creation.
    // This allows employees to request an absence they forgot to log time for;
    // the approver then handles the conflict.
    {
        let work_day = next_monday(-7).format("%Y-%m-%d").to_string();
        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": work_day,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create time entry");

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": work_day,"end_date": work_day}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "absence can be requested even with logged time"
        );
        let pre_existing_conflict_id = id(&body);

        // Approval must fail because the time entry still exists.
        let (st, body) = lead
            .post(
                &format!("/api/v1/absences/{pre_existing_conflict_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "approval blocked by pre-existing time entry"
        );
        assert!(
            body.to_string().contains("logged time"),
            "error mentions logged time: {body}"
        );

        // Clean up: cancel the requested absence so it does not interfere with later sections.
        let (st, _) = emp
            .delete(&format!("/api/v1/absences/{pre_existing_conflict_id}"))
            .await;
        assert_eq!(st, StatusCode::OK, "cancel conflicting absence");
    }
    // -- Absence requires at least one effective workday --
    {
        let next_week_monday = next_monday(7);
        let saturday = (next_week_monday + chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        let sunday = (next_week_monday + chrono::Duration::days(6))
            .format("%Y-%m-%d")
            .to_string();

        for kind in [
            "vacation",
            "sick",
            "training",
            "special_leave",
            "unpaid",
            "general_absence",
        ] {
            let (st, body) = emp
                .post(
                    "/api/v1/absences",
                    &json!({"kind": kind, "start_date": saturday, "end_date": sunday}),
                )
                .await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "weekend-only {kind} absence should be rejected"
            );
            assert!(
                body.to_string()
                    .contains("Absence must include at least one workday"),
                "error should mention missing workday for {kind}: {body}"
            );
        }
    }

    // -- Absence update requires at least one effective workday --
    {
        // Use a Monday far enough in the future to avoid public holidays
        // (e.g. Whit Monday) that would make the single-day absence invalid,
        // and distinct from dates used in other test sections.
        let next_week_monday = next_monday(21);
        let monday = next_week_monday.format("%Y-%m-%d").to_string();
        let saturday = (next_week_monday + chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        let sunday = (next_week_monday + chrono::Duration::days(6))
            .format("%Y-%m-%d")
            .to_string();

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": monday,"end_date": monday}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create weekday absence");
        let absence_id = id(&body);

        let (st, body) = emp
            .put(
                &format!("/api/v1/absences/{absence_id}"),
                &json!({"kind":"vacation","start_date": saturday,"end_date": sunday}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "update to weekend-only rejected"
        );
        assert!(
            body.to_string()
                .contains("Absence must include at least one workday"),
            "error should mention missing workday: {body}"
        );
    }

    // -- Approval rejects logged time conflicts --
    {
        // Use a different workday than the previous block to avoid state bleed
        // from the earlier "logged time" test case.
        // Use next_monday(-14) + 1 day to ensure it's in the past and not a holiday.
        let conflict_day = (next_monday(-14) + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        // Create the time entry FIRST — once a requested absence covers this day,
        // new time entries are blocked to prevent the approval deadlock.
        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": conflict_day,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "time entry allowed before absence is requested"
        );

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": conflict_day,"end_date": conflict_day}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "create requested absence after time entry"
        );
        let absence_id = id(&body);

        // New time entries on the same day must now be blocked (prevents deadlock
        // where entries added after requesting make the absence impossible to approve).
        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": conflict_day,
                    "start_time": "13:00",
                    "end_time": "14:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "time entry blocked while absence is pending"
        );

        let (st, body) = lead
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "approval rejects logged time conflict"
        );
        assert!(
            body.to_string().contains("logged time"),
            "error mentions logged time: {body}"
        );
    }

    // -- Sick updates cannot backdate and auto-approved sick can be cancelled --
    {
        let future_start = next_monday(14).format("%Y-%m-%d").to_string();
        let future_end = (next_monday(14) + chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"sick","start_date": future_start,"end_date": future_end}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create future sick absence");
        let future_sick = id(&body);
        assert_eq!(body["status"], "requested", "future sick stays requested");

        let too_old = (reference_date() - chrono::Duration::days(31))
            .format("%Y-%m-%d")
            .to_string();
        let (st, body) = emp
            .put(
                &format!("/api/v1/absences/{future_sick}"),
                &json!({"kind":"sick","start_date": too_old,"end_date": too_old}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "backdated sick update rejected"
        );
        assert!(
            body.to_string().contains("backdated more than 30 days"),
            "error mentions backdate limit: {body}"
        );

        let current_start = next_monday(-21).format("%Y-%m-%d").to_string();
        let current_end = (next_monday(-21) + chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"sick","start_date": current_start,"end_date": current_end}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create current sick absence");
        let auto_sick = id(&body);
        assert_eq!(body["status"], "approved", "current sick auto-approved");

        let (st, body) = emp
            .put(
                &format!("/api/v1/absences/{auto_sick}"),
                &json!({"kind":"sick","start_date": current_start,"end_date": current_end,"comment":"updated"}),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "approved sick edit rejected");
        assert!(
            body.to_string()
                .contains("Only requested absences can be edited"),
            "edit failure body: {body}"
        );

        let (st, body) = emp.delete(&format!("/api/v1/absences/{auto_sick}")).await;
        assert_eq!(st, StatusCode::OK, "approved sick cancellation accepted");
        assert_eq!(
            body["pending"], true,
            "approved sick cancellation requires approver review"
        );
    }

    // -- Approved absence cannot be edited but cancellation requires approval --
    {
        let day_start = next_monday(28).format("%Y-%m-%d").to_string();
        let day_end = (next_monday(28) + chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": day_start,"end_date": day_end}),
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

        let (st, body) = emp
            .put(
                &format!("/api/v1/absences/{absence_id}"),
                &json!({"kind":"vacation","start_date": day_start,"end_date": day_end,"comment":"edited"}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "approved absence is not editable"
        );
        assert!(
            body.to_string()
                .contains("Only requested absences can be edited"),
            "edit failure body: {body}"
        );

        // Cancelling an approved absence triggers a cancellation approval workflow.
        let (st, body) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
        assert_eq!(st, StatusCode::OK, "cancellation request accepted");
        assert_eq!(
            body["pending"], true,
            "cancellation requires approver review"
        );
    }

    // -- Employee calendar is scoped strictly to themselves --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"peer@example.com",
                    "first_name":"Pia",
                    "last_name":"Peer",
                    "role":"employee",
                    "weekly_hours":39,
                    "leave_days_current_year":30,"leave_days_next_year":30, "annual_leave_days": 30,
                    "start_date":"2024-01-01",
                    "approver_ids": [lead_id],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create peer");
        let peer_id = id(&body);
        let peer_pw = temp_pw(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"lead-two@example.com",
                    "first_name":"Ola",
                    "last_name":"OtherLead",
                    "role":"team_lead",
                    "weekly_hours":39,
                    "leave_days_current_year":30,"leave_days_next_year":30, "annual_leave_days": 30,
                    "start_date":"2024-01-01",
                    "approver_ids":[1],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create second lead");
        let other_lead_id = id(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"outsider@example.com",
                    "first_name":"Otto",
                    "last_name":"Outsider",
                    "role":"employee",
                    "weekly_hours":39,
                    "leave_days_current_year":30,"leave_days_next_year":30, "annual_leave_days": 30,
                    "start_date":"2024-01-01",
                    "approver_ids": [other_lead_id],
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create outsider");
        let outsider_id = id(&body);
        let outsider_pw = temp_pw(&body);

        let peer = login_change_pw(&app, "peer@example.com", &peer_pw).await;
        let outsider = login_change_pw(&app, "outsider@example.com", &outsider_pw).await;

        let calendar_day = next_monday(35).format("%Y-%m-%d").to_string();
        let month = calendar_day[..7].to_string();

        let (st, _) = lead
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": calendar_day,"end_date": calendar_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create approver absence");

        let (st, _) = peer
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": calendar_day,"end_date": calendar_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create peer absence");

        // Outsider's absence has a comment so we can also verify that even
        // the comment privacy doesn't have a backdoor: an employee must not
        // see ANY data from a non-scope user, full stop.
        let (st, _) = outsider
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": calendar_day,"end_date": calendar_day,"comment":"family trip"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create outsider absence");

        // emp also takes a vacation on the same day so the positive assertion
        // below actually executes (visible_ids would be empty without this).
        let (st, _) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": calendar_day,"end_date": calendar_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create emp absence");

        let (st, body) = emp
            .get(&format!("/api/v1/absences/calendar?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "calendar request");
        let rows = body.as_array().expect("calendar rows should be an array");
        let visible_ids: HashSet<i64> = rows
            .iter()
            .filter_map(|row| row["user_id"].as_i64())
            .collect();

        // Strict scope rule: a regular employee sees ONLY their own absences
        // in the calendar. No per-category carve-out exists — `team_visible`
        // was removed in migration 019 because the scope rule is the single
        // source of truth and the flag was redundant under it.
        assert!(
            visible_ids.contains(&emp_id),
            "employee must see their own absence"
        );
        assert!(
            !visible_ids.contains(&lead_id),
            "approver must NOT be visible in employee calendar"
        );
        assert!(
            !visible_ids.contains(&peer_id),
            "peer must NOT be visible in employee calendar"
        );
        assert!(
            !visible_ids.contains(&outsider_id),
            "outsider must NOT be visible in employee calendar"
        );
        for id in &visible_ids {
            assert_eq!(*id, emp_id, "only the requester's own entries may appear");
        }

        // Leads see their direct reports and themselves; they do NOT see
        // users outside their report scope, regardless of category.
        let (st, body) = lead
            .get(&format!("/api/v1/absences/calendar?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "lead calendar request");
        let lead_visible: HashSet<i64> = body
            .as_array()
            .expect("calendar rows should be an array")
            .iter()
            .filter_map(|row| row["user_id"].as_i64())
            .collect();
        assert!(lead_visible.contains(&lead_id), "lead sees own entries");
        assert!(
            lead_visible.contains(&peer_id),
            "lead sees peer (direct report)"
        );
        assert!(
            lead_visible.contains(&emp_id),
            "lead sees emp (direct report)"
        );
        assert!(
            !lead_visible.contains(&outsider_id),
            "lead must NOT see users outside their report scope"
        );
    }

    // -- Privacy regression: a sick absence from a non-scope user (lead) must
    // -- not appear at all in a regular employee's calendar. The same strict
    // -- scope rule applies regardless of category — sick is just a clear
    // -- example (GDPR Art. 9 health data).
    {
        let sick_day = next_monday(36).format("%Y-%m-%d").to_string();
        let sick_month = sick_day[..7].to_string();
        let (st, _) = lead
            .post(
                "/api/v1/absences",
                &json!({"kind":"sick","start_date": sick_day,"end_date": sick_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create lead sick absence");

        let (st, body) = emp
            .get(&format!("/api/v1/absences/calendar?month={sick_month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "emp sick-month calendar request");
        let rows = body.as_array().expect("calendar rows should be an array");
        for row in rows {
            if row["user_id"].as_i64() == Some(lead_id)
                && row["start_date"].as_str() == Some(sick_day.as_str())
            {
                panic!("non-lead must not see lead's sick absence — scope rule: {row}");
            }
        }
    }

    // -- Absences list rejects invalid year query --
    {
        let (st, body) = emp.get("/api/v1/absences?year=2147483647").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid year must be rejected");
        assert!(
            body.to_string().contains("Invalid year"),
            "error should mention invalid year: {body}"
        );
    }

    // -- Leave balance rejects invalid year query --
    {
        let (st, body) = emp
            .get(&format!("/api/v1/leave-balance/{emp_id}?year=2147483647"))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid year must be rejected");
        assert!(
            body.to_string().contains("Invalid year"),
            "error should mention invalid year: {body}"
        );
    }

    // -- cancellation_pending vacation remains reserved and moves to pending bucket --
    {
        let target_day = next_monday(42).format("%Y-%m-%d").to_string();
        let year = &target_day[..4];

        let (st, balance_before) = emp
            .get(&format!("/api/v1/leave-balance/{emp_id}?year={year}"))
            .await;
        assert_eq!(st, StatusCode::OK, "load baseline leave balance");

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": target_day,"end_date": target_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create vacation for cancellation test");
        let absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "approve vacation before cancellation request"
        );

        let (st, balance_after_approval) = emp
            .get(&format!("/api/v1/leave-balance/{emp_id}?year={year}"))
            .await;
        assert_eq!(st, StatusCode::OK, "load leave balance after approval");

        let approved_before = balance_before["approved_upcoming"].as_f64().unwrap_or(0.0);
        let requested_before = balance_before["requested"].as_f64().unwrap_or(0.0);
        let approved_after = balance_after_approval["approved_upcoming"]
            .as_f64()
            .unwrap_or(0.0);
        let requested_after = balance_after_approval["requested"].as_f64().unwrap_or(0.0);
        let booked_days = approved_after - approved_before;
        assert!(
            booked_days > 0.0,
            "approved upcoming should increase after approval (before={approved_before}, after={approved_after})"
        );
        assert_eq!(
            requested_after, requested_before,
            "requested bucket should not change after approval"
        );

        let (st, body) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
        assert_eq!(
            st,
            StatusCode::OK,
            "request cancellation for approved vacation"
        );
        assert_eq!(
            body["pending"], true,
            "approved vacation cancellation should enter pending workflow"
        );

        let (st, balance_after_cancellation_request) = emp
            .get(&format!("/api/v1/leave-balance/{emp_id}?year={year}"))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "load leave balance after cancellation request"
        );

        let approved_pending = balance_after_cancellation_request["approved_upcoming"]
            .as_f64()
            .unwrap_or(0.0);
        let requested_pending = balance_after_cancellation_request["requested"]
            .as_f64()
            .unwrap_or(0.0);
        let available_after_approval = balance_after_approval["available"].as_f64().unwrap_or(0.0);
        let available_pending = balance_after_cancellation_request["available"]
            .as_f64()
            .unwrap_or(0.0);

        assert_eq!(
            approved_pending, approved_before,
            "approved upcoming should drop back when cancellation is pending"
        );
        assert_eq!(
            requested_pending,
            requested_before + booked_days,
            "pending cancellation days should move into requested bucket"
        );
        assert_eq!(
            available_pending, available_after_approval,
            "available balance should remain reserved while cancellation is pending"
        );
    }

    // -- Cancellation approval and rejection follow distinct status and balance paths --
    {
        let approval_day = next_monday(49).format("%Y-%m-%d").to_string();
        let rejection_day = next_monday(56).format("%Y-%m-%d").to_string();
        let year = &approval_day[..4];

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": approval_day,"end_date": approval_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create approval-path vacation");
        let approval_absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{approval_absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve vacation before cancellation");

        let (st, _) = emp
            .delete(&format!("/api/v1/absences/{approval_absence_id}"))
            .await;
        assert_eq!(st, StatusCode::OK, "request cancellation");

        let (st, body) = lead
            .post(
                &format!("/api/v1/absences/{approval_absence_id}/approve-cancellation"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve cancellation");
        assert_eq!(body["ok"], true);

        let (st, list) = emp.get(&format!("/api/v1/absences?year={year}")).await;
        assert_eq!(st, StatusCode::OK);
        let approved_cancelled = list
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"].as_i64() == Some(approval_absence_id))
            .expect("approved-cancellation absence present");
        assert_eq!(approved_cancelled["status"], "cancelled");

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": rejection_day,"end_date": rejection_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create rejection-path vacation");
        let rejection_absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{rejection_absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve second vacation");

        let (st, _) = emp
            .delete(&format!("/api/v1/absences/{rejection_absence_id}"))
            .await;
        assert_eq!(st, StatusCode::OK, "request second cancellation");

        let (st, body) = lead
            .post(
                &format!("/api/v1/absences/{rejection_absence_id}/reject-cancellation"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "reject cancellation");
        assert_eq!(body["ok"], true);

        let (st, list) = emp.get(&format!("/api/v1/absences?year={year}")).await;
        assert_eq!(st, StatusCode::OK);
        let rejection_restored = list
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"].as_i64() == Some(rejection_absence_id))
            .expect("rejected-cancellation absence present");
        assert_eq!(rejection_restored["status"], "approved");

        let (st, body) = emp
            .get(&format!("/api/v1/leave-balance/{emp_id}?year={year}"))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "load balance after cancellation decisions"
        );
        assert!(
            body["approved_upcoming"].as_f64().unwrap_or(0.0) >= 1.0,
            "rejected cancellation keeps approved future vacation reserved"
        );
    }

    // -- Admin revoke cancels approved absence and non-admins cannot revoke --
    {
        let revoke_day = next_monday(63).format("%Y-%m-%d").to_string();

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({"kind":"vacation","start_date": revoke_day,"end_date": revoke_day}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create revoke candidate");
        let revoke_absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{revoke_absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve revoke candidate");

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{revoke_absence_id}/revoke"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "only admins can revoke");

        let (st, body) = admin
            .post(
                &format!("/api/v1/absences/{revoke_absence_id}/revoke"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin revoke succeeds");
        assert_eq!(body["ok"], true);

        let year = &revoke_day[..4];
        let (st, list) = emp.get(&format!("/api/v1/absences?year={year}")).await;
        assert_eq!(st, StatusCode::OK);
        let revoked = list
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"].as_i64() == Some(revoke_absence_id))
            .expect("revoked absence present");
        assert_eq!(revoked["status"], "cancelled");
    }

    app.cleanup().await;
}

/// Covers the "no valid approver" conflict error path in
/// `services::auth::required_approval_recipient_ids`.
///
/// This path triggers when a non-admin employee submits an absence but their
/// only approver has been deactivated, leaving the approver list empty.
#[tokio::test]
async fn absence_request_fails_when_approver_is_deactivated() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create a lead and an employee who reports to that lead.
    let (lead_id, lead_pw, emp_id, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    // The lead must first change their password too.
    let _lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    // Deactivate the lead (the employee's only approver).
    // First, reassign the employee's time entries lead by removing direct reports.
    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{emp_id}"),
            &json!({"approver_ids": [1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reassign emp to admin approver: {body}");

    let (st, _) = admin
        .post(&format!("/api/v1/users/{lead_id}/deactivate"), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "deactivate lead");

    // Reassign back to the now-deactivated lead at DB level to simulate the
    // scenario where the lead was deactivated after the approver relationship
    // was set up.  The application service filters inactive approvers, so the
    // list will come back empty.
    sqlx::query("DELETE FROM user_approvers WHERE user_id=$1")
        .bind(emp_id)
        .execute(&app.state.pool)
        .await
        .expect("remove current approvers");
    sqlx::query("INSERT INTO user_approvers(user_id, approver_id) VALUES ($1, $2)")
        .bind(emp_id)
        .bind(lead_id)
        .execute(&app.state.pool)
        .await
        .expect("force stale approver row");

    // Employee tries to create a vacation absence — must fail because the
    // deactivated lead is filtered out and no valid approver remains.
    let vac_start = next_monday(7).format("%Y-%m-%d").to_string();
    let vac_end = vac_start.clone();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": vac_start,"end_date": vac_end}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::CONFLICT,
        "absence request must fail when all approvers are inactive: {body}"
    );

    app.cleanup().await;
}

/// Verifies the in-app-only notification path taken when an admin approves,
/// rejects, or processes a cancellation of their **own** absence.
///
/// When `absence.user_id == requester.id`, the service calls
/// `notify_absence_inapp_only` instead of the email-enabled `notify_absence`.
/// This exercises `services::notifications::create_inapp_only` and
/// `create_translated_inapp_only` which are otherwise never reached in the
/// regular integration-test flows (where a lead approves an employee's absence).
#[tokio::test]
async fn absences_admin_self_approval_uses_inapp_only_notifications() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Create a future vacation for the admin (id=1) so the admin can approve it.
    // Use a Monday ≥ 14 days out to avoid upcoming public holidays.
    let vac_start = next_monday(14).format("%Y-%m-%d").to_string();
    let vac_end = (next_monday(14) + chrono::Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/absences",
            &json!({
                "kind": "vacation",
                "start_date": vac_start,
                "end_date": vac_end,
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin creates own vacation: {body}");
    let abs_id = id(&body);
    assert_eq!(body["status"], "requested", "vacation starts as requested");

    // Admin approves their own vacation → triggers notify_absence_inapp_only
    // (absence_approved path), covering create_translated_inapp_only and
    // create_inapp_only in services::notifications.
    let (st, body) = admin
        .post(&format!("/api/v1/absences/{abs_id}/approve"), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "admin approves own vacation: {body}");

    // Verify the absence is now approved.
    let (st, body) = admin.get(&format!("/api/v1/absences/{abs_id}")).await;
    assert_eq!(st, StatusCode::OK, "get approved absence");
    assert_eq!(body["status"], "approved", "absence should be approved");

    // Admin requests cancellation of the just-approved absence.
    let (st, body) = admin.delete(&format!("/api/v1/absences/{abs_id}")).await;
    assert_eq!(st, StatusCode::OK, "admin requests cancellation: {body}");
    assert_eq!(
        body["pending"], true,
        "cancellation enters pending workflow"
    );

    // Admin approves the cancellation of their own absence →
    // notify_absence_inapp_only (absence_cancellation_approved path).
    let (st, body) = admin
        .post(
            &format!("/api/v1/absences/{abs_id}/approve-cancellation"),
            &json!({}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "admin approves own cancellation: {body}"
    );

    // ── reject path ──────────────────────────────────────────────────────────
    // Create a second vacation and have the admin reject it themselves.
    let vac2_start = next_monday(21).format("%Y-%m-%d").to_string();
    let vac2_end = (next_monday(21) + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/absences",
            &json!({"kind": "vacation", "start_date": vac2_start, "end_date": vac2_end}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "admin creates second own vacation: {body}"
    );
    let abs2_id = id(&body);

    // Admin rejects their own vacation →
    // notify_absence_inapp_only (absence_rejected path).
    let (st, body) = admin
        .post(
            &format!("/api/v1/absences/{abs2_id}/reject"),
            &json!({"reason": "test self-rejection"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin rejects own vacation: {body}");

    // ── revoke path: create + approve + revoke ────────────────────────────────
    let vac3_start = next_monday(28).format("%Y-%m-%d").to_string();
    let vac3_end = (next_monday(28) + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/absences",
            &json!({"kind": "vacation", "start_date": vac3_start, "end_date": vac3_end}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create vac3: {body}");
    let abs3_id = id(&body);

    let (st, _) = admin
        .post(&format!("/api/v1/absences/{abs3_id}/approve"), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "approve vac3");

    // Admin revokes their own approved absence →
    // notify_absence_inapp_only (absence_revoked path).
    let (st, body) = admin
        .post(&format!("/api/v1/absences/{abs3_id}/revoke"), &json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "admin revokes own absence: {body}");

    app.cleanup().await;
}

/// Assistants (Aushilfen) have no fixed weekdays, so they must be able to
/// submit absences on any day of the week — including weekends.
#[tokio::test]
async fn assistant_absence_any_weekday() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "assistant-absences@example.com",
                "first_name": "Assist",
                "last_name": "AnyDay",
                "role": "assistant",
                "weekly_hours": 0,
                "leave_days_current_year": 0,
                "leave_days_next_year": 0,
                "annual_leave_days": 0,
                "start_date": "2024-01-01",
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create assistant user");
    let assistant_pw = temp_pw(&body);
    let assistant = login_change_pw(&app, "assistant-absences@example.com", &assistant_pw).await;

    let next_week_monday = next_monday(7);
    let saturday = (next_week_monday + chrono::Duration::days(5))
        .format("%Y-%m-%d")
        .to_string();
    let sunday = (next_week_monday + chrono::Duration::days(6))
        .format("%Y-%m-%d")
        .to_string();
    let next_saturday = (next_week_monday + chrono::Duration::days(12))
        .format("%Y-%m-%d")
        .to_string();

    // Weekend-only absence must be accepted for assistants.
    let (st, _) = assistant
        .post(
            "/api/v1/absences",
            &json!({"kind": "general_absence", "start_date": saturday, "end_date": sunday}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "assistant can submit absence on Saturday+Sunday"
    );

    // Single-day Saturday absence must also be accepted.
    let (st, _) = assistant
        .post(
            "/api/v1/absences",
            &json!({"kind": "general_absence", "start_date": next_saturday, "end_date": next_saturday}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "assistant can submit single-day Saturday absence"
    );

    app.cleanup().await;
}

/// Covers Bug 5: `validate_flextime_balance` must run at approval time, not
/// only at request time. A request that passed validation when the balance
/// was sufficient must be rejected at approval if the balance has fallen
/// below the floor in the meantime.
#[tokio::test]
async fn flextime_balance_revalidated_at_approval() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, emp_pw, _, _cat_id) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    // bootstrap_team gives the employee start_date=2024-01-01. With the test
    // reference date in 2030 and no logged time, the cumulative flextime deficit
    // is multi-million minutes — overwhelming any small seed. We use a giant
    // positive seed for the request, then flip it negative to drain the balance
    // below the floor for the approval re-check. The exact numbers don't
    // matter; the assertion is "pass at request, fail at approval after drain."
    sqlx::query("UPDATE users SET overtime_start_balance_min = 99000000 WHERE id = $1")
        .bind(emp_id)
        .execute(&app.state.pool)
        .await
        .expect("seed flextime balance positive");

    let monday = next_monday(7).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"flextime_reduction","start_date":monday,"end_date":monday}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "flextime request must pass while balance is sufficient: {body}"
    );
    let absence_id = id(&body);

    // Drain the balance: zeroing the seed leaves only the multi-million-minute
    // historical deficit (years of workdays with no logged time), which is far
    // below the default floor (0). The re-validation at approval must reject.
    sqlx::query("UPDATE users SET overtime_start_balance_min = 0 WHERE id = $1")
        .bind(emp_id)
        .execute(&app.state.pool)
        .await
        .expect("drain flextime balance");

    let (st, body) = lead
        .post(
            &format!("/api/v1/absences/{absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "approval must be rejected once balance falls below floor: {body}"
    );
    assert!(
        body.to_string().contains("flextime balance"),
        "rejection should mention flextime balance: {body}"
    );

    app.cleanup().await;
}

/// Covers Bug 6: editing a requested absence whose category was deactivated
/// by an admin in the meantime must succeed when the user is NOT changing the
/// category — otherwise users can never adjust dates or comment on an
/// in-flight request once their category is retired. Switching INTO a
/// different inactive category must still be rejected.
#[tokio::test]
async fn edit_requested_absence_allowed_when_inactive_category_unchanged() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _, _cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let day_a = next_monday(30).format("%Y-%m-%d").to_string();
    let day_b = next_monday(31).format("%Y-%m-%d").to_string();

    // Request a training absence (review-required, no balance impact).
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"training","start_date":day_a,"end_date":day_a}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create training request: {body}");
    let absence_id = id(&body);
    let training_cat_id = body["category_id"]
        .as_i64()
        .expect("category_id present on response");

    // Admin deactivates the training category. (No DELETE endpoint — the
    // service-level update accepts {"active": false}.)
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"active": false}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "deactivate training category");

    // The user edits the absence (same category, different end date).
    // Without the Bug 6 fix this fails with 400 "Absence category is no
    // longer active." even though the user is not changing category.
    let (st, body) = emp
        .put(
            &format!("/api/v1/absences/{absence_id}"),
            &json!({
                "category_id": training_cat_id,
                "start_date": day_a,
                "end_date": day_b,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "edit must succeed when inactive category is unchanged: {body}"
    );
    assert_eq!(body["end_date"].as_str(), Some(day_b.as_str()));

    // Switching INTO another inactive category must still be rejected.
    // First deactivate a second category to use as the switch target.
    let (_, cats_body) = admin.get("/api/v1/absence-categories/all").await;
    let general_cat_id = cats_body
        .as_array()
        .expect("categories array")
        .iter()
        .find(|c| c["slug"].as_str() == Some("general_absence"))
        .expect("general_absence seeded category exists")["id"]
        .as_i64()
        .expect("id is number");
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{general_cat_id}"),
            &json!({"active": false}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "deactivate general_absence category");

    let (st, body) = emp
        .put(
            &format!("/api/v1/absences/{absence_id}"),
            &json!({
                "category_id": general_cat_id,
                "start_date": day_a,
                "end_date": day_a,
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "switching INTO a different inactive category must be rejected: {body}"
    );

    app.cleanup().await;
}

/// Covers Bug 9: an admin must not be able to change a category's `cost_type`
/// or `auto_approve_past` while it still has at least one referencing absence.
/// Doing so would silently mutate the financial / approval meaning of those
/// existing rows — past balance recomputations would suddenly debit or
/// credit different ledgers, and the approval workflow would relax or
/// tighten without affected employees seeing it. The right escape hatch is
/// to deactivate the category and create a new one with the desired flags.
#[tokio::test]
async fn absence_category_cost_flag_change_blocked_when_in_use() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _, _cat_id) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // Look up the seeded `training` category id (cost_type='none',
    // auto_approve_past=false).
    let (_, cats_body) = admin.get("/api/v1/absence-categories/all").await;
    let training_cat_id = cats_body
        .as_array()
        .expect("categories array")
        .iter()
        .find(|c| c["slug"].as_str() == Some("training"))
        .expect("training seeded category exists")["id"]
        .as_i64()
        .expect("id is int");

    // Without any referencing absences, the admin can freely toggle the flag.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"auto_approve_past": true}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "toggling flag must work while category is unused"
    );
    // Revert so the rest of the test starts from a clean state.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"auto_approve_past": false}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "revert flag");

    // Once an employee submits a request in this category, the flag is locked.
    let training_day = next_monday(20).format("%Y-%m-%d").to_string();
    let (st, _) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"training","start_date": training_day,"end_date": training_day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create training absence");

    // Attempting to switch cost_type to 'vacation' must be rejected —
    // flipping it would retroactively debit the employee's annual leave
    // for an already-approved-or-pending range they never agreed to.
    let (st, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"cost_type": "vacation"}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "cost_type change to 'vacation' must be blocked while category is in use: {body}"
    );

    // Same protection for switching to 'flextime'.
    let (st, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"cost_type": "flextime"}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "cost_type change to 'flextime' must be blocked while category is in use: {body}"
    );

    // Same protection for auto_approve_past.
    let (st, body) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"auto_approve_past": true}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "auto_approve_past flip must be blocked while category is in use: {body}"
    );

    // Cosmetic + active changes MUST still be allowed — they don't change
    // the financial meaning of existing rows. The Bug 6 fix relies on
    // `active=false` being permitted even when the category has referencing
    // absences.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}"),
            &json!({"name": "Training (renamed)", "color": "#aabbcc", "active": false}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "cosmetic + active changes must still be allowed"
    );

    app.cleanup().await;
}
