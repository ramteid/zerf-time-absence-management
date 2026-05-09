use std::collections::HashSet;

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team, id, login_change_pw, next_monday, temp_pw};

#[tokio::test]
async fn non_sick_absence_rejects_logged_time() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, _, _, emp_pw, _, cat_id) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

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
        StatusCode::BAD_REQUEST,
        "absence over logged time rejected"
    );
    assert!(
        body.to_string().contains("logged time"),
        "error mentions logged time: {body}"
    );
}

#[tokio::test]
async fn absence_requires_at_least_one_effective_workday() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, _, _, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let next_week_monday = next_monday(7);
    let saturday = (next_week_monday + chrono::Duration::days(5))
        .format("%Y-%m-%d")
        .to_string();
    let sunday = (next_week_monday + chrono::Duration::days(6))
        .format("%Y-%m-%d")
        .to_string();

    for kind in ["vacation", "sick", "training", "special_leave", "unpaid", "general_absence"] {
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

#[tokio::test]
async fn absence_update_requires_at_least_one_effective_workday() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, _, _, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    let next_week_monday = next_monday(7);
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
    assert_eq!(st, StatusCode::BAD_REQUEST, "update to weekend-only rejected");
    assert!(
        body.to_string()
            .contains("Absence must include at least one workday"),
        "error should mention missing workday: {body}"
    );
}

#[tokio::test]
async fn approval_rejects_logged_time_conflicts() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, lead_pw, _, emp_pw, _, cat_id) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let work_day = next_monday(-7).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": work_day,"end_date": work_day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create requested absence");
    let absence_id = id(&body);

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
    assert_eq!(
        st,
        StatusCode::OK,
        "time entry still allowed while absence is pending"
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

#[tokio::test]
async fn sick_updates_cannot_backdate_and_auto_approved_sick_can_be_cancelled() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, _, _, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

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

    let too_old = (chrono::Local::now().date_naive() - chrono::Duration::days(31))
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

    let current_start = next_monday(-7).format("%Y-%m-%d").to_string();
    let current_end = (next_monday(-7) + chrono::Duration::days(2))
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
        body.to_string().contains("Cannot edit"),
        "edit failure body: {body}"
    );

    let (st, body) = emp.delete(&format!("/api/v1/absences/{auto_sick}")).await;
    assert_eq!(st, StatusCode::OK, "approved sick cancellation accepted");
    assert_eq!(
        body["pending"], true,
        "approved sick cancellation requires approver review"
    );
}

#[tokio::test]
async fn approved_absence_cannot_be_edited_but_cancellation_requires_approval() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_, lead_pw, _, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;

    let day_start = next_monday(14).format("%Y-%m-%d").to_string();
    let day_end = (next_monday(14) + chrono::Duration::days(2))
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
        body.to_string().contains("Cannot edit"),
        "edit failure body: {body}"
    );

    // Cancelling an approved absence triggers a cancellation approval workflow.
    let (st, body) = emp.delete(&format!("/api/v1/absences/{absence_id}")).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "cancellation request accepted"
    );
    assert_eq!(
        body["pending"], true,
        "cancellation requires approver review"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn employee_calendar_is_scoped_to_their_team() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, _, emp_pw, _, _) = bootstrap_team(&app, &admin, false).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email":"peer@example.com",
                "first_name":"Pia",
                "last_name":"Peer",
                "role":"employee",
                "weekly_hours":39,
                "leave_days_current_year":30,"leave_days_next_year":30,
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
                "leave_days_current_year":30,"leave_days_next_year":30,
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
                "leave_days_current_year":30,"leave_days_next_year":30,
                "start_date":"2024-01-01",
                "approver_ids": [other_lead_id],
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create outsider");
    let outsider_id = id(&body);
    let outsider_pw = temp_pw(&body);

    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let peer = login_change_pw(&app, "peer@example.com", &peer_pw).await;
    let outsider = login_change_pw(&app, "outsider@example.com", &outsider_pw).await;

    let calendar_day = next_monday(21).format("%Y-%m-%d").to_string();
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

    let (st, _) = outsider
        .post(
            "/api/v1/absences",
            &json!({"kind":"vacation","start_date": calendar_day,"end_date": calendar_day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create outsider absence");

    let (st, body) = emp
        .get(&format!("/api/v1/absences/calendar?month={month}"))
        .await;
    assert_eq!(st, StatusCode::OK, "calendar request");
    let visible_ids: HashSet<i64> = body
        .as_array()
        .expect("calendar rows should be an array")
        .iter()
        .filter_map(|row| row["user_id"].as_i64())
        .collect();

    assert!(visible_ids.contains(&lead_id), "approver is visible");
    assert!(visible_ids.contains(&peer_id), "peer is visible");
    assert!(
        !visible_ids.contains(&outsider_id),
        "outsider should not be visible in team calendar"
    );
}
