//! End-to-end reports workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::{TestApp, TestClient};
use crate::helpers::*;

async fn assert_get_forbidden(client: &TestClient, path: &str, label: &str) {
    let (status, _) = client.get(path).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{label}");
}

#[tokio::test]
async fn reports_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Range CSV and category totals for booked entries --
    {
        let (lead_id, lead_pw, emp_id, emp_pw, monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "1").await;
        let lead = login_change_pw(&app, "lead-1@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-1@example.com", &emp_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "=draft formula"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create draft report entry");
        let entry_id = id(&body);

        let (st, _) = lead
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "13:00",
                    "end_time": "17:00",
                    "category_id": cat_id,
                    "comment": "lead own time"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create lead draft entry");

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "pure-admin-category-scope@example.com",
                    "first_name": "Pure",
                    "last_name": "CategoryScope",
                    "role": "admin",
                    "weekly_hours": 0,
                    "leave_days_current_year": 0,
                    "leave_days_next_year": 0,
                    "annual_leave_days": 0,
                    "start_date": "2024-01-01",
                    "approver_ids": [1],
                    "tracks_time": false
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create pure-admin scope fixture");
        let pure_admin_id = id(&body);

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "inactive-category-scope@example.com",
                    "first_name": "Inactive",
                    "last_name": "CategoryScope",
                    "role": "employee",
                    "weekly_hours": 39,
                    "leave_days_current_year": 30,
                    "leave_days_next_year": 30,
                    "annual_leave_days": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [lead_id]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create inactive scope fixture");
        let inactive_id = id(&body);
        // Set user inactive via direct DB mutation (deactivation feature removed).
        sqlx::query("UPDATE users SET active=FALSE WHERE id=$1")
            .bind(inactive_id)
            .execute(&app.state.pool)
            .await
            .expect("set scope fixture inactive");

        for excluded_user_id in [pure_admin_id, inactive_id] {
            sqlx::query(
                "INSERT INTO time_entries(user_id, entry_date, start_time, end_time, category_id, status, reviewed_by, reviewed_at) \
                 VALUES ($1,$2,'08:00','12:00',$3,'approved',$4,CURRENT_TIMESTAMP)",
            )
            .bind(excluded_user_id)
            .bind(chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap())
            .bind(cat_id)
            .bind(lead_id)
            .execute(&app.state.pool)
            .await
            .unwrap();
        }

        // Draft entries are booked time and should appear in category totals.
        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report with only draft");
        assert_eq!(body.as_array().unwrap()[0]["minutes"], 240);

        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead aggregate category report");
        assert_eq!(
            body.as_array().unwrap()[0]["minutes"],
            480,
            "aggregate must include lead + direct report booked time"
        );

        let (st, body) = admin
            .get(&format!(
                "/api/v1/reports/categories?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "admin aggregate category scope");
        assert_eq!(
            body.as_array().unwrap()[0]["minutes"],
            480,
            "admin aggregate must exclude pure-admin and inactive legacy entries"
        );

        // Submit and approve the entry
        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit entry");
        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve entry");

        // Approved entries remain visible in category totals.
        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report with approved");
        assert_eq!(body.as_array().unwrap()[0]["minutes"], 240);

        let (st, csv_body) = lead
            .get_raw(&format!(
                "/api/v1/reports/csv?user_id={}&from={}&to={}",
                emp_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "range CSV export");
        assert!(csv_body.contains("08:00"));
        assert!(csv_body.contains("'=draft formula"));

        let (st, _) = lead
            .get(&format!(
                "/api/v1/reports/csv?user_id={}&from=2026-05-02&to=2026-05-01",
                emp_id
            ))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "CSV inverted range rejected");

        let too_far = (chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap()
            + chrono::Duration::days(367))
        .format("%Y-%m-%d")
        .to_string();
        let (st, _) = lead
            .get(&format!(
                "/api/v1/reports/csv?user_id={}&from={}&to={}",
                emp_id, monday, too_far
            ))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "CSV max range rejected");

        let (st, _) = emp
            .get(&format!(
                "/api/v1/reports/csv?user_id={}&from={}&to={}",
                lead_id, monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "employee cannot export lead CSV");

        let month = &monday[..7];
        let (st, _) = lead
            .get_raw(&format!(
                "/api/v1/reports/month/csv?user_id={}&month={}",
                emp_id, month
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "legacy month CSV remains available");
    }

    // -- Flextime reduction blocks the day but does not credit hours or submission coverage --
    {
        let (_lead_id, lead_pw, emp_id, emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "5").await;
        let lead = login_change_pw(&app, "lead-5@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-5@example.com", &emp_pw).await;
        let tuesday = (chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap()
            + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

        let (_, categories_body) = admin.get("/api/v1/categories").await;
        let flextime_reduction_category_id =
            category_id_by_name(&categories_body, "Flextime Reduction")
                .expect("flextime reduction category exists");

        // Give the employee a large opening flextime balance so B8
        // (validate_flextime_balance) passes. The integration test user is
        // created with start_date=2024-01-01 but has no approved hours, so
        // without a positive seed the balance would be deeply negative.
        sqlx::query("UPDATE users SET overtime_start_balance_min = 9999999 WHERE id = $1")
            .bind(emp_id)
            .execute(&app.state.pool)
            .await
            .expect("seed flextime balance");

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "flextime_reduction",
                    "start_date": monday,
                    "end_date": monday,
                    "comment": "use balance"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create flextime reduction absence");
        let absence_id = id(&body);

        let (st, _) = lead
            .post(
                &format!("/api/v1/absences/{absence_id}/approve"),
                &json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve flextime reduction absence");

        let (st, _) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": flextime_reduction_category_id,
                    "comment": "should still be blocked"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "approved flextime reduction absence blocks the day"
        );

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": tuesday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": flextime_reduction_category_id,
                    "comment": "flex reduction entry"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create flextime reduction entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit flextime reduction entry");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve flextime reduction entry");

        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report with flextime reduction");

        let monday_row = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert_eq!(monday_row["absence"], "flextime_reduction");
        assert_eq!(monday_row["target_min"], per_day_target_minutes(39));
        assert_eq!(monday_row["actual_min"], 0);

        let tuesday_row = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == tuesday)
            .unwrap();
        assert_eq!(tuesday_row["actual_min"], 0);
        assert_eq!(tuesday_row["entries"].as_array().unwrap().len(), 1);
        assert_eq!(body["submitted_min"], 0);
        // Category totals include all non-rejected entries regardless of
        // crediting status (user-guide: "not only crediting categories").
        // The approved flextime-reduction entry (4h = 240 min) appears here.
        let cat_totals = body["category_totals"].as_object().unwrap();
        assert_eq!(cat_totals.len(), 1, "one category in totals");
        assert_eq!(
            cat_totals
                .get("Flextime Reduction")
                .and_then(|v| v.as_i64()),
            Some(240)
        );
        assert_eq!(body["weeks_all_submitted"], false);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "flextime report with flextime reduction"
        );
        let rows = body.as_array().unwrap();
        assert_eq!(rows[0]["target_min"], per_day_target_minutes(39));
        assert_eq!(rows[0]["actual_min"], 0);
        assert_eq!(rows[1]["target_min"], per_day_target_minutes(39));
        assert_eq!(rows[1]["actual_min"], 0);

        let (st, _body) = emp
            .get(&format!(
                "/api/v1/reports/categories?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "employee still needs user_id for category report"
        );

        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "category report includes non-crediting entries"
        );
        // Category breakdowns include all non-rejected entries regardless of
        // crediting status (user-guide: "not only crediting categories").
        let cat_arr = body.as_array().unwrap();
        assert_eq!(cat_arr.len(), 1, "one category in report");
        assert_eq!(cat_arr[0]["category"], "Flextime Reduction");
        assert_eq!(cat_arr[0]["minutes"], 240);

        let (st, csv_body) = lead
            .get_raw(&format!(
                "/api/v1/reports/month/csv?user_id={}&month={}",
                emp_id, month
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "month CSV with flextime reduction");
        assert!(
            csv_body.contains(",Total,,,,0,"),
            "CSV total must ignore non-crediting flextime reduction entries: {csv_body}"
        );
    }

    // -- Partial sick day counts worked time and removes target --
    {
        let (_lead_id, lead_pw, _emp_id, emp_pw, monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;
        let lead = login_change_pw(&app, "lead-2@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-2@example.com", &emp_pw).await;

        let (st, body) = emp
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "sick",
                    "start_date": monday,
                    "end_date": monday,
                    "comment": "cold"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create sick leave");
        assert_eq!(body["status"], "approved");

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "worked half day"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create partial sick-day entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit partial sick-day entry");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve partial sick-day entry");

        // Sick leave removes the target for that day. Actual remains the approved
        // worked time only; absence credit is shown separately in absence reporting.
        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={}", month))
            .await;
        assert_eq!(st, StatusCode::OK, "month report");
        let day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert_eq!(day["absence"], "sick");
        assert_eq!(day["target_min"], 0);
        assert_eq!(day["actual_min"], 240);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime report");
        assert_eq!(body.as_array().unwrap()[0]["target_min"], 0);
        assert_eq!(body.as_array().unwrap()[0]["actual_min"], 240);
    }

    // -- Reports include current day in hours and categories --
    {
        let (_lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3").await;
        let lead = login_change_pw(&app, "lead-3@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-3@example.com", &emp_pw).await;
        let today = today();

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": today,
                    "start_time": "00:00",
                    "end_time": "00:01",
                    "category_id": cat_id,
                    "comment": "today should report"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create today entry");
        let entry_id = id(&body);

        let (st, _) = emp
            .post("/api/v1/time-entries/submit", &json!({"ids": [entry_id]}))
            .await;
        assert_eq!(st, StatusCode::OK, "submit today entry");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve today entry");

        let month = &today[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report");
        // Month report is now month-to-date and therefore includes today's approved entries.
        assert_eq!(body["actual_min"], 1);
        assert!(!body["category_totals"].as_object().unwrap().is_empty());
        let today_row = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == today)
            .unwrap();
        assert_eq!(today_row["actual_min"], 1);
        assert_eq!(today_row["entries"].as_array().unwrap().len(), 1);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, today, today
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report for today");
        let rows = body.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["minutes"], 1);
    }

    // -- cancellation_pending absences remove day target like approved absences --
    {
        let (_lead_id, _lead_pw, emp_id, emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3b").await;
        let emp = login_change_pw(&app, "emp-3b@example.com", &emp_pw).await;

        // Insert a cancellation_pending vacation absence directly to pin report semantics.
        // Time-entry validation treats this status as blocking, so reports/flextime must
        // also remove target minutes for the covered day.
        sqlx::query(
            "INSERT INTO absences(user_id, category_id, start_date, end_date, status, created_at) \
             SELECT $1, id, $2, $2, 'cancellation_pending', CURRENT_TIMESTAMP \
             FROM absence_categories WHERE slug='vacation'",
        )
        .bind(emp_id)
        .bind(chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap())
        .execute(&app.state.pool)
        .await
        .unwrap();

        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report for cancellation_pending");
        let day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert_eq!(day["absence"], "vacation");
        assert_eq!(day["target_min"], 0);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime for cancellation_pending");
        assert_eq!(body.as_array().unwrap()[0]["absence"], "vacation");
        assert_eq!(body.as_array().unwrap()[0]["target_min"], 0);
    }

    // -- requested absences do not remove day target before approval --
    {
        let expected_day_target = per_day_target_minutes(39);
        let (_lead_id, _lead_pw, emp_id, emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "3c").await;
        let emp = login_change_pw(&app, "emp-3c@example.com", &emp_pw).await;

        // Insert a requested vacation absence directly to pin report semantics.
        // Requested absences are not yet approved and therefore must NOT remove
        // target minutes in month/flextime views.
        sqlx::query(
            "INSERT INTO absences(user_id, category_id, start_date, end_date, status, created_at) \
             SELECT $1, id, $2, $2, 'requested', CURRENT_TIMESTAMP \
             FROM absence_categories WHERE slug='vacation'",
        )
        .bind(emp_id)
        .bind(chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap())
        .execute(&app.state.pool)
        .await
        .unwrap();

        let month = &monday[..7];
        let (st, body) = emp
            .get(&format!("/api/v1/reports/month?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "month report for requested absence");
        let day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == monday)
            .unwrap();
        assert!(day["absence"].is_null());
        assert_eq!(day["target_min"], expected_day_target);

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime for requested absence");
        assert!(body.as_array().unwrap()[0]["absence"].is_null());
        assert_eq!(
            body.as_array().unwrap()[0]["target_min"],
            expected_day_target
        );
    }

    // -- Reports ignore legacy time before user start date --
    {
        let (lead_id, lead_pw, emp_id, emp_pw, _monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "4").await;
        let lead = login_change_pw(&app, "lead-4@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-4@example.com", &emp_pw).await;
        let legacy_date = chrono::NaiveDate::from_ymd_opt(2023, 12, 29).unwrap();
        let legacy_date_iso = legacy_date.format("%Y-%m-%d").to_string();

        sqlx::query(
            "INSERT INTO time_entries(user_id, entry_date, start_time, end_time, category_id, status, reviewed_by, reviewed_at) \
             VALUES ($1,$2,$3,$4,$5,'approved',$6,CURRENT_TIMESTAMP)",
        )
        .bind(emp_id)
        .bind(legacy_date)
        .bind("08:00")
        .bind("12:00")
        .bind(cat_id)
        .bind(lead_id)
        .execute(&app.state.pool)
        .await
        .unwrap();

        let (st, body) = emp.get("/api/v1/reports/month?month=2023-12").await;
        assert_eq!(st, StatusCode::OK, "month report before start date");
        assert_eq!(body["actual_min"], 0);
        assert!(body["category_totals"].as_object().unwrap().is_empty());
        let legacy_day = body["days"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["date"] == legacy_date_iso)
            .unwrap();
        assert_eq!(legacy_day["target_min"], 0);
        assert_eq!(legacy_day["actual_min"], 0);
        assert!(legacy_day["entries"].as_array().unwrap().is_empty());

        let (st, body) = emp
            .get(&format!(
                "/api/v1/reports/flextime?from={}&to={}",
                legacy_date_iso, legacy_date_iso
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime before start date");
        assert_eq!(body.as_array().unwrap()[0]["actual_min"], 0);
        assert_eq!(body.as_array().unwrap()[0]["target_min"], 0);

        let (st, body) = lead
            .get(&format!(
                "/api/v1/reports/categories?user_id={}&from={}&to={}",
                emp_id, legacy_date_iso, legacy_date_iso
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "category report before start date");
        assert!(body.as_array().unwrap().is_empty());
    }

    // -- Assistant behavior is role-based, not weekly_hours-based --
    {
        let (lead_id, _lead_pw, _emp_id, _emp_pw, _monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "assistant-role").await;
        let month = today()[..7].to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"assistant-reports@example.com",
                    "first_name":"Role",
                    "last_name":"Assistant",
                    "role":"assistant",
                    "weekly_hours":0,
                    "leave_days_current_year":0,
                    "leave_days_next_year":0,
                    "annual_leave_days": 0,
                    "start_date":"2024-01-01",
                    "approver_ids":[lead_id]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create assistant for reports");
        let assistant_id = id(&body);

        // Simulate legacy/imported inconsistency that bypasses API validation.
        sqlx::query(
            "UPDATE users SET weekly_hours = 39.0, overtime_start_balance_min = 120 WHERE id = $1",
        )
        .bind(assistant_id)
        .execute(&app.state.pool)
        .await
        .unwrap();

        let (st, body) = admin
            .get(&format!(
                "/api/v1/reports/month?user_id={assistant_id}&month={month}"
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "assistant month report");
        assert_eq!(
            body["target_min"], 0,
            "assistant month target must remain 0"
        );
        assert_eq!(
            body["full_month_target_min"], 0,
            "assistant full-month target must remain 0"
        );

        let (st, body) = admin
            .get(&format!("/api/v1/reports/team?month={month}"))
            .await;
        assert_eq!(st, StatusCode::OK, "team report for assistant checks");
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["user_id"].as_i64() == Some(assistant_id))
            .expect("assistant row present in team report");
        assert!(
            row["flextime_balance_min"].is_null(),
            "assistant team flextime balance must be null"
        );
        assert!(
            row["diff_min"].is_null(),
            "assistant team monthly diff must be null"
        );
    }

    // -- Range, overtime, and team category reports enforce scope and aggregate correctly --
    {
        let (lead_id, lead_pw, emp_id, emp_pw, monday, cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "6").await;
        let lead = login_change_pw(&app, "lead-6@example.com", &lead_pw).await;
        let emp = login_change_pw(&app, "emp-6@example.com", &emp_pw).await;
        let tuesday = (chrono::NaiveDate::parse_from_str(&monday, "%Y-%m-%d").unwrap()
            + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": monday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                    "comment": "range approved"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create monday entry");
        let monday_entry = id(&body);

        let (st, body) = emp
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": tuesday,
                    "start_time": "09:00",
                    "end_time": "11:00",
                    "category_id": cat_id,
                    "comment": "range draft"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create tuesday entry");
        let tuesday_entry = id(&body);

        let (st, _) = emp
            .post(
                "/api/v1/time-entries/submit",
                &json!({"ids": [monday_entry, tuesday_entry]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "submit both entries");

        let (st, _) = lead
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [monday_entry]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "approve monday entry");

        let (st, range_body) = emp
            .get(&format!(
                "/api/v1/reports/range?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "own range report");
        assert_eq!(
            range_body["actual_min"], 240,
            "range actual counts only approved time"
        );
        assert_eq!(
            range_body["submitted_min"], 360,
            "submitted_min includes submitted-but-not-yet-approved work"
        );

        let (st, _) = emp
            .get(&format!(
                "/api/v1/reports/range?user_id={lead_id}&from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "employee cannot read another user's range report"
        );

        let monday_year = &monday[..4];
        let (st, overtime_body) = emp
            .get(&format!("/api/v1/reports/overtime?year={monday_year}"))
            .await;
        assert_eq!(st, StatusCode::OK, "own overtime report");
        assert!(
            overtime_body
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["month"] == monday[..7]),
            "overtime contains the active month"
        );

        let (st, lead_team_categories) = lead
            .get(&format!(
                "/api/v1/reports/team-categories?from={}&to={}",
                monday, tuesday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead team category report");
        let rows = lead_team_categories.as_array().unwrap();
        assert!(rows
            .iter()
            .any(|row| row["user_id"].as_i64() == Some(emp_id)));
        let emp_row = rows
            .iter()
            .find(|row| row["user_id"].as_i64() == Some(emp_id))
            .expect("employee row in team categories");
        assert!(
            emp_row["categories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|cat| cat["minutes"].as_i64().unwrap_or(0) >= 360),
            "team categories aggregate submitted and approved entry minutes"
        );
    }

    // -- Assistant overtime is empty and admin subjects are excluded from lead-scoped team categories --
    {
        let (lead_id, lead_pw, _emp_id, _emp_pw, monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "7").await;
        let lead = login_change_pw(&app, "lead-7@example.com", &lead_pw).await;

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"assistant-report@example.com",
                    "first_name":"Assist",
                    "last_name":"Report",
                    "role":"assistant",
                    "weekly_hours":0,
                    "leave_days_current_year":0,
                    "leave_days_next_year":0,
                    "annual_leave_days": 0,
                    "start_date":"2024-01-01",
                    "approver_ids":[lead_id]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create assistant report user");
        let assistant_id = id(&body);
        let assistant_pw = temp_pw(&body);
        let assistant = login_change_pw(&app, "assistant-report@example.com", &assistant_pw).await;

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email":"admin-report-subject@example.com",
                    "first_name":"Admin",
                    "last_name":"Subject",
                    "role":"admin",
                    "weekly_hours":39,
                    "leave_days_current_year":30,
                    "leave_days_next_year":30,
                    "annual_leave_days": 30,
                    "start_date":"2024-01-01",
                    "approver_ids":[1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin subject");
        let admin_subject_id = id(&body);

        let monday_year = &monday[..4];
        let (st, assistant_overtime) = assistant
            .get(&format!("/api/v1/reports/overtime?year={monday_year}"))
            .await;
        assert_eq!(st, StatusCode::OK, "assistant overtime request succeeds");
        assert_eq!(
            assistant_overtime.as_array().unwrap().len(),
            0,
            "assistants have no overtime rows"
        );

        let (st, team_categories) = lead
            .get(&format!(
                "/api/v1/reports/team-categories?from={}&to={}",
                monday, monday
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "lead team categories loads");
        let rows = team_categories.as_array().unwrap();
        assert!(
            rows.iter()
                .any(|row| row["user_id"].as_i64() == Some(assistant_id)),
            "assistant direct report stays visible"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row["user_id"].as_i64() == Some(admin_subject_id)),
            "admin subjects are excluded from lead-scoped team category reports"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn report_permission_guards_reject_non_reportable_users_on_every_personal_endpoint() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, _emp_id, _emp_pw, monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "permission-guard").await;
    let month = monday[..7].to_string();
    let year = &monday[..4];

    let (status, pure_admin_body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "reports-pure-admin-target@example.com",
                "first_name": "Pure",
                "last_name": "ReportTarget",
                "role": "admin",
                "tracks_time": false,
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "annual_leave_days": 30,
                "start_date": "2024-01-01"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create pure-admin report target");
    let pure_admin_id = id(&pure_admin_body);
    let pure_admin_password = temp_pw(&pure_admin_body);
    let pure_admin = login_change_pw(
        &app,
        "reports-pure-admin-target@example.com",
        &pure_admin_password,
    )
    .await;

    let (status, inactive_body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "reports-inactive-target@example.com",
                "first_name": "Inactive",
                "last_name": "ReportTarget",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "annual_leave_days": 30,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create inactive report target");
    let inactive_id = id(&inactive_body);
    // Archive the report target (archive sets active=FALSE; archived users are
    // still included in historical reports since they had time data).
    let (status, _) = admin
        .post(
            &format!("/api/v1/users/{inactive_id}/archive"),
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "archive report target");

    let personal_paths = |target_id: i64| {
        vec![
            format!("/api/v1/reports/month?user_id={target_id}&month={month}"),
            format!("/api/v1/reports/month/csv?user_id={target_id}&month={month}"),
            format!("/api/v1/reports/range?user_id={target_id}&from={monday}&to={monday}"),
            format!("/api/v1/reports/csv?user_id={target_id}&from={monday}&to={monday}"),
            format!("/api/v1/reports/categories?user_id={target_id}&from={monday}&to={monday}"),
            format!("/api/v1/reports/overtime?user_id={target_id}&year={year}"),
            format!("/api/v1/reports/flextime?user_id={target_id}&from={monday}&to={monday}"),
        ]
    };

    for path in personal_paths(pure_admin_id) {
        assert_get_forbidden(&admin, &path, "admin cannot report on a pure-admin account").await;
    }
    for path in personal_paths(inactive_id) {
        assert_get_forbidden(&admin, &path, "admin cannot report on an inactive account").await;
    }

    let self_paths = vec![
        format!("/api/v1/reports/month?month={month}"),
        format!("/api/v1/reports/month/csv?month={month}"),
        format!("/api/v1/reports/range?from={monday}&to={monday}"),
        format!("/api/v1/reports/csv?from={monday}&to={monday}"),
        format!("/api/v1/reports/categories?user_id={pure_admin_id}&from={monday}&to={monday}"),
        format!("/api/v1/reports/overtime?year={year}"),
        format!("/api/v1/reports/flextime?from={monday}&to={monday}"),
    ];
    for path in self_paths {
        assert_get_forbidden(
            &pure_admin,
            &path,
            "pure-admin cannot default or explicitly report on themselves",
        )
        .await;
    }

    app.cleanup().await;
}
