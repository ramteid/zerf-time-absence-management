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

    // -- Non-crediting entries still block overlaps (there is no per-day cap) --
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
        assert_eq!(
            st,
            StatusCode::OK,
            "long crediting entry accepted (no per-day cap)"
        );
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

    // -- Non-admin lead cannot batch reject own submitted entry --
    {
        let (_lead_id, lead_pw, _emp_id, _emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3").await;
        let lead = login_change_pw(&app, "lead-3@example.com", &lead_pw).await;

        let (st, body) = lead
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday_iso,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "lead self-review check"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead creates own entry");
        let entry_id = id(&body);

        let (st, _) = lead
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "lead submits own entry");

        let (st, body) = lead
            .post(
                "/api/v1/time-entries/batch-reject",
                &json!({"ids": [entry_id], "reason": "self-review should be skipped"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "self-reject request is processed");
        assert_eq!(body["count"], 0, "non-admin self-reject is skipped");

        let (_, entries) = lead.get("/api/v1/time-entries").await;
        let entry = find_by_id(&entries, entry_id).expect("entry exists");
        assert_eq!(entry["status"], "submitted");
        assert_eq!(entry["rejection_reason"], serde_json::Value::Null);
    }

    // -- Submission auto-approval: silent draft -> approved, no notifications --
    {
        let (_lead_id, lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "subauto").await;
        let lead = login_change_pw(&app, "lead-subauto@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-subauto@example.com", &emp_pw).await;

        let (st, _) = admin
            .put(
                &format!("/api/v1/team-settings/{}", emp_id),
                &json!({"allow_reopen_without_approval": false, "allow_submission_without_approval": true}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "enable submission auto-approval");

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday_iso,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "auto-approved work"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create draft entry");
        let entry_id = id(&body);

        let (st, body) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit auto-approved entry");
        assert_eq!(body["count"], 1);
        assert_eq!(body["auto_approved"], true);

        let (_, entries) = emp.get("/api/v1/time-entries").await;
        let entry = find_by_id(&entries, entry_id).expect("entry exists");
        assert_eq!(
            entry["status"], "approved",
            "entry skips 'submitted' and goes straight to 'approved'"
        );
        assert_eq!(
            entry["reviewed_by"], emp_id,
            "the system records the user themselves as reviewer"
        );

        // Silent by design: neither the requester nor the approver is
        // notified, and (by extension, since no notification is created) no
        // email is sent either.
        let (_, emp_notifications) = emp.get("/api/v1/notifications").await;
        assert!(
            emp_notifications.as_array().unwrap().is_empty(),
            "requester must not be notified about their own auto-approved submission"
        );
        let (_, lead_notifications) = lead.get("/api/v1/notifications").await;
        assert!(
            lead_notifications.as_array().unwrap().is_empty(),
            "approver must not be notified about an auto-approved submission"
        );
    }

    // -- Team entries carry the owner name for calendar rendering --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, monday_iso, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "calendarname").await;
        let lead = login_change_pw(&app, "lead-calendarname@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-calendarname@example.com", &emp_pw).await;
        let entry_id = create_and_submit_entry(&emp, &monday_iso, cat_id).await;

        let (st, entries) = lead
            .get(&format!(
                "/api/v1/time-entries/all?from={monday_iso}&to={monday_iso}"
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead loads team entries");
        let entry = find_by_id(&entries, entry_id).expect("team entry exists");
        assert_eq!(
            entry["user_name"], "Emilcalendarname Empcalendarname",
            "team entry includes its owner's full display name"
        );
    }

    app.cleanup().await;
}

/// A day can hold approved hours and a fresh submission at the same time:
/// nothing stops booking more time into a week that was already signed off, and
/// the automatic break is worked out over the whole day. The approval queue
/// therefore asks for the approved hours of the days it is showing, so it can
/// say what a week will really credit — this pins the query it relies on.
#[tokio::test]
async fn approved_hours_of_a_pending_day_are_readable_by_the_approver() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "pending-day").await;
    let lead = login_change_pw(&app, "lead-pending-day@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-pending-day@example.com", &emp_pw).await;

    // A morning shift, handed in and signed off.
    let (st, body) = employee
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso, "start_time": "08:00", "end_time": "13:00",
                "category_id": cat_id, "comment": "morning"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book the morning shift: {body}");
    let morning_id = id(&body);
    let (st, body) = employee
        .post("/api/v1/time-entries/submit", &json!({"ids": [morning_id]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit the morning shift: {body}");
    let (st, body) = lead
        .post(
            "/api/v1/time-entries/batch-approve",
            &json!({"ids": [morning_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve the morning shift: {body}");

    // An afternoon shift added to that same, already approved day.
    let (st, body) = employee
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday_iso, "start_time": "13:00", "end_time": "17:00",
                "category_id": cat_id, "comment": "afternoon"
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "an approved day still accepts more time: {body}"
    );
    let afternoon_id = id(&body);
    let (st, body) = employee
        .post("/api/v1/time-entries/submit", &json!({"ids": [afternoon_id]}))
        .await;
    assert_eq!(st, StatusCode::OK, "submit the afternoon shift: {body}");

    // The queue sees only the afternoon shift as pending ...
    let (st, body) = lead.get("/api/v1/time-entries/all?status=submitted").await;
    assert_eq!(st, StatusCode::OK, "pending entries: {body}");
    let pending: Vec<i64> = body
        .as_array()
        .expect("array")
        .iter()
        .filter(|entry| entry["user_id"].as_i64() == Some(emp_id))
        .map(id)
        .collect();
    assert_eq!(pending, vec![afternoon_id]);

    // ... and can read the morning's approved hours for the same day, which is
    // what tells it how much break that day already carries.
    let (st, body) = lead
        .get(&format!(
            "/api/v1/time-entries/all?status=approved&from={monday_iso}&to={monday_iso}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "approved entries on that day: {body}");
    let approved: Vec<&serde_json::Value> = body
        .as_array()
        .expect("array")
        .iter()
        .filter(|entry| entry["user_id"].as_i64() == Some(emp_id))
        .collect();
    assert_eq!(approved.len(), 1, "just the morning shift: {approved:?}");
    assert_eq!(approved[0]["id"].as_i64(), Some(morning_id));
    assert_eq!(approved[0]["start_time"].as_str(), Some("08:00"));
    assert_eq!(approved[0]["end_time"].as_str(), Some("13:00"));
    assert_eq!(
        approved[0]["counts_as_work"].as_bool(),
        Some(true),
        "the crediting flag travels with it, or the break cannot be computed"
    );

    app.cleanup().await;
}

/// What approving such a day actually credits, measured on the server.
///
/// The approval queue states the same figure, and it can only get there by
/// pricing the submission against the whole day. This test is that figure's
/// anchor: it asks the month report what the day credited before and after,
/// so the number the browser is expected to show is never just a number
/// somebody reasoned their way to.
#[tokio::test]
async fn approving_a_second_shift_credits_the_whole_days_break() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // More than six hours of work costs 30 minutes of break.
    for (key, value) in [
        (zerf::services::settings::AUTO_BREAK_ENABLED_KEY, "true"),
        (zerf::services::settings::AUTO_BREAK_THRESHOLD_HOURS_KEY, "6"),
        (
            zerf::services::settings::AUTO_BREAK_DEDUCTION_MINUTES_KEY,
            "30",
        ),
    ] {
        app.state
            .db
            .settings
            .save_setting(key, value)
            .await
            .expect("configure automatic break");
    }

    let (_lead_id, lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "whole-day-break").await;
    let lead = login_change_pw(&app, "lead-whole-day-break@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-whole-day-break@example.com", &emp_pw).await;
    let month = &monday_iso[0..7];

    // Credited minutes of that day, as the month report states them.
    async fn credited_on_the_day(
        client: &crate::common::TestClient,
        user_id: i64,
        month: &str,
        day_iso: &str,
    ) -> i64 {
        let (st, body) = client
            .get(&format!(
                "/api/v1/reports/month?user_id={user_id}&month={month}"
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "month report: {body}");
        body["days"]
            .as_array()
            .expect("days")
            .iter()
            .find(|day| day["date"].as_str() == Some(day_iso))
            .expect("the booked day")["actual_min"]
            .as_i64()
            .expect("actual_min")
    }

    async fn book_and_approve(
        employee: &crate::common::TestClient,
        lead: &crate::common::TestClient,
        day_iso: &str,
        category_id: i64,
        start: &str,
        end: &str,
    ) {
        let (st, body) = employee
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": day_iso, "start_time": start, "end_time": end,
                    "category_id": category_id
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "book {start}-{end}: {body}");
        let entry_id = id(&body);
        let (st, body) = employee
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit {start}-{end}: {body}");
        let (st, body) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve {start}-{end}: {body}");
    }

    // Five hours, comfortably under the threshold: credited in full.
    book_and_approve(&employee, &lead, &monday_iso, cat_id, "08:00", "13:00").await;
    let before = credited_on_the_day(&lead, emp_id, month, &monday_iso).await;
    assert_eq!(before, 300, "five hours, no break due yet");

    // Four more hours take the day to nine, which owes 30 minutes of break.
    book_and_approve(&employee, &lead, &monday_iso, cat_id, "13:00", "17:00").await;
    let after = credited_on_the_day(&lead, emp_id, month, &monday_iso).await;
    assert_eq!(after, 510, "nine hours less the day's 30-minute break");

    // So the second shift credits 3:30, not the 4:00 its own row shows — which
    // is exactly what the approval queue has to state before it is approved.
    assert_eq!(
        after - before,
        210,
        "the second shift adds three and a half hours, not four"
    );

    app.cleanup().await;
}
