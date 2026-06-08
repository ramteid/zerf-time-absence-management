//! End-to-end start_date enforcement tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use chrono::Datelike;

use crate::helpers::{admin_login, reference_date};

#[tokio::test]
async fn start_date_full_workflow() {
    // Capture all dates synchronously (before any await) from a single
    // reference_date() call so that concurrent tests temporarily mutating the
    // process-wide TEST_REFERENCE_DATE env var cannot skew the values we use
    // throughout this long-running test.
    let ref_date = reference_date();
    let today_str = ref_date.format("%Y-%m-%d").to_string();
    let yesterday_str = (ref_date - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    // Next Monday ≥8 days out — used as the sick-absence end date.
    let sick_end_str = {
        let start = ref_date + chrono::Duration::days(7);
        let wd = start.weekday().num_days_from_monday();
        let monday = if wd == 0 {
            start
        } else {
            start + chrono::Duration::days((7 - wd) as i64)
        };
        monday.format("%Y-%m-%d").to_string()
    };
    // Next Monday ≥8 days out as a NaiveDate — used as the flex-carry user's start_date.
    let flex_start_day = {
        let start = ref_date + chrono::Duration::days(7);
        let wd = start.weekday().num_days_from_monday();
        if wd == 0 {
            start
        } else {
            start + chrono::Duration::days((7 - wd) as i64)
        }
    };

    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, cats) = admin.get("/api/v1/categories").await;
    let cat_id = cats.as_array().unwrap()[0]["id"].as_i64().unwrap();

    // -- Time entry before start date rejected --
    // Admin's start_date is set to today during seed. Verify that creating a time
    // entry before that date is rejected.
    {
        let yesterday = yesterday_str.clone();
        let (st, body) = admin
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": yesterday,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "entry before start_date: {body}"
        );
        assert!(
            body.to_string().contains("before user start date"),
            "error message: {body}"
        );
    }

    // -- Time entry on start date accepted --
    // Time entry on the start_date itself must succeed.
    {
        let (st, _) = admin
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": today_str,
                    "start_time": "00:00",
                    "end_time": "00:01",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "entry on start_date should succeed");
    }

    // -- Absence before start date rejected --
    // Absence that starts before the user's start_date must be rejected.
    {
        let yesterday = yesterday_str.clone();
        let (st, body) = admin
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": yesterday,
                    "end_date": yesterday,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "absence before start_date: {body}"
        );
        assert!(
            body.to_string().contains("before user start date"),
            "error message: {body}"
        );
    }

    // -- Absence on start date accepted --
    // Absence on or after start_date should be accepted.
    //
    // Use next_monday(7) (the second upcoming Monday, ≥8 days out) rather than
    // next_monday(0) so that the range is never entirely consumed by the
    // immediate next Monday being a public holiday (e.g. Pfingst Montag 2026).
    {
        let (st, _) = admin
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "sick",
                    "start_date": today_str,
                    "end_date": sick_end_str,
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "absence on start_date should succeed");
    }

    // -- Overtime no months before start date --
    // Overtime report for a user whose start_date is today should show only the
    // current month (if any), not months before it, and the cumulative balance
    // must be non-positive only by at most one day's target.
    {
        let year = ref_date.format("%Y").to_string();
        let (st, body) = admin
            .get(&format!("/api/v1/reports/overtime?year={year}"))
            .await;
        assert_eq!(st, StatusCode::OK);

        let rows = body.as_array().expect("overtime should be array");
        // Admin was seeded with the reference date. Only the current month (or none) should appear.
        let current_month = ref_date.format("%Y-%m").to_string();
        for row in rows {
            let month = row["month"].as_str().unwrap();
            assert!(
                month >= current_month.as_str(),
                "month {month} is before start month {current_month}"
            );
        }
        // The cumulative balance must not be wildly negative (max 1 day deficit).
        if let Some(last) = rows.last() {
            let cum = last["cumulative_min"].as_i64().unwrap();
            // 39h/week => 468 min/day max deficit
            assert!(
                cum >= -468,
                "cumulative overtime {cum} min is too negative for a fresh user"
            );
        }
    }

    // -- Overtime start balance carries into later years --
    {
        let current_year: i32 = ref_date.year();
        let start_date = chrono::NaiveDate::from_ymd_opt(current_year - 1, 1, 1)
            .unwrap()
            .to_string();
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "carry@example.com",
                    "first_name": "Carry",
                    "last_name": "Balance",
                    "role": "admin",
                    "weekly_hours": 0,
                    "leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date": start_date,
                    "overtime_start_balance_min": 120
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create carry-balance user: {body}");
        let uid = body["id"].as_i64().unwrap();

        let (st, body) = admin
            .get(&format!(
                "/api/v1/reports/overtime?user_id={uid}&year={current_year}"
            ))
            .await;
        assert_eq!(st, StatusCode::OK);
        let rows = body.as_array().expect("overtime should be array");
        assert!(!rows.is_empty(), "current year should have overtime rows");
        assert_eq!(
            rows[0]["cumulative_min"].as_i64(),
            Some(120),
            "start balance should carry into the next year"
        );
        assert_eq!(
            rows.last().unwrap()["cumulative_min"].as_i64(),
            Some(120),
            "zero-hour user should keep the carried balance"
        );
    }

    // -- Flextime start balance begins on start date --
    {
        // Use flex_start_day (captured at the top, before any await).
        let start_day_str = flex_start_day.format("%Y-%m-%d").to_string();
        let day_before_str = (flex_start_day - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "flex-carry@example.com",
                    "first_name": "Flex",
                    "last_name": "Carry",
                    "role": "admin",
                    "weekly_hours": 0,
                    "leave_days_current_year":0,"leave_days_next_year":0,
                    "start_date": start_day_str,
                    "overtime_start_balance_min": 120
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create flex carry user: {body}");
        let uid = body["id"].as_i64().unwrap();

        // Report spans the day before start_date (Sunday) through start_date
        // (Monday).  The Sunday row should show cumulative=0 (before the user
        // exists) and the Monday row cumulative=120 (balance kicks in).
        let (st, body) = admin
            .get(&format!(
                "/api/v1/reports/flextime?user_id={uid}&from={day_before_str}&to={start_day_str}"
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "flextime report");
        let rows = body.as_array().expect("flextime should be array");
        assert_eq!(
            rows.first().unwrap()["cumulative_min"].as_i64(),
            Some(0),
            "balance should not apply before the user's start date"
        );
        assert_eq!(
            rows.last().unwrap()["cumulative_min"].as_i64(),
            Some(120),
            "balance should apply on the user's start date"
        );
    }

    // -- New user start date enforced --
    // A newly created user with a future-ish start_date should not be able to
    // create entries before that date.
    {
        // Use the top-level ref_date (captured before any await) so that
        // concurrent TEST_REFERENCE_DATE mutations by other parallel tests
        // cannot skew start_date_str and before_start_str against each other.
        let start_date_str = ref_date.format("%Y-%m-%d").to_string();
        let before_start_str = (ref_date - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        // Create a user with start_date = today
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "new@example.com",
                    "first_name": "New",
                    "last_name": "User",
                    "role": "admin",
                    "weekly_hours": 39,
                    "leave_days_current_year":30,"leave_days_next_year":30,
                    "start_date": start_date_str,
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create user: {body}");
        let pw = body["temporary_password"].as_str().unwrap().to_string();

        // Login as the new user
        let new_client = app.client();
        let (st, _) = new_client.login("new@example.com", &pw).await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = new_client.change_password(&pw, "NewPass!2345").await;
        assert_eq!(st, StatusCode::OK);

        let (_, cats) = new_client.get("/api/v1/categories").await;
        let cat_id = cats.as_array().unwrap()[0]["id"].as_i64().unwrap();

        // Entry one day before start_date should fail
        let (st, _) = new_client
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": before_start_str,
                    "start_time": "08:00",
                    "end_time": "12:00",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "entry before start_date for new user"
        );

        // Entry on start_date should succeed
        let (st, body) = new_client
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": start_date_str,
                    "start_time": "00:00",
                    "end_time": "00:01",
                    "category_id": cat_id,
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "entry on start_date for new user: {body}"
        );
    }

    app.cleanup().await;
}
