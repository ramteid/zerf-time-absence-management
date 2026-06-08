use reqwest::StatusCode;
use serde_json::json;
use sqlx::query;

use crate::common::TestApp;
use crate::helpers::{admin_login, id, login_change_pw, next_monday, temp_pw};

#[tokio::test]
async fn audit_log_is_forbidden_for_non_admin_users() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "audit-employee@example.com",
                "first_name": "Eva",
                "last_name": "Employee",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let employee_pw = temp_pw(&body);

    let employee = login_change_pw(&app, "audit-employee@example.com", &employee_pw).await;
    let (st, _) = employee.get("/api/v1/audit-log").await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "employee must not read audit log"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn audit_log_supports_table_and_record_filters() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "audit-filter@example.com",
                "first_name": "Uwe",
                "last_name": "Filter",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let created_user_id = id(&body);

    let (st, body) = admin
        .get(&format!(
            "/api/v1/audit-log?table_name=users&record_id={created_user_id}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "audit log query");

    let rows = body.as_array().expect("audit response must be an array");
    assert!(
        !rows.is_empty(),
        "filtered audit query must return at least one row"
    );
    for row in rows {
        assert_eq!(
            row["table_name"].as_str(),
            Some("users"),
            "table_name filter must be applied"
        );
        assert_eq!(
            row["record_id"].as_i64(),
            Some(created_user_id),
            "record_id filter must be applied"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn audit_log_supports_user_id_filter() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "audit-user-filter@example.com",
                "first_name": "Tina",
                "last_name": "Time",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let employee_id = id(&body);
    let employee_pw = temp_pw(&body);

    let employee = login_change_pw(&app, "audit-user-filter@example.com", &employee_pw).await;
    let (st, cats) = employee.get("/api/v1/categories").await;
    assert_eq!(st, StatusCode::OK, "read categories");
    let category_id = cats
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["id"].as_i64())
        .expect("at least one category id");

    let monday = next_monday(-14).format("%Y-%m-%d").to_string();
    let (st, _) = employee
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": category_id,
                "comment": "audit test"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create time entry");

    let (st, body) = admin
        .get(&format!("/api/v1/audit-log?user_id={employee_id}"))
        .await;
    assert_eq!(st, StatusCode::OK, "audit log query by user_id");

    let rows = body.as_array().expect("audit response must be an array");
    assert!(
        !rows.is_empty(),
        "user_id filter must return rows for employee actions"
    );
    assert!(
        rows.iter()
            .any(|row| row["table_name"].as_str() == Some("time_entries")),
        "expected at least one time_entries audit row for employee"
    );
    for row in rows {
        assert_eq!(
            row["user_id"].as_i64(),
            Some(employee_id),
            "user_id filter must be applied"
        );
    }

    app.cleanup().await;
}

#[tokio::test]
async fn audit_log_combines_all_filters_with_and_semantics() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "audit-combined-filter@example.com",
                "first_name": "Iris",
                "last_name": "Inspect",
                "role": "employee",
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "start_date": "2024-01-01",
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let employee_id = id(&body);
    let employee_pw = temp_pw(&body);

    let employee = login_change_pw(&app, "audit-combined-filter@example.com", &employee_pw).await;
    let (st, cats) = employee.get("/api/v1/categories").await;
    assert_eq!(st, StatusCode::OK, "read categories");
    let category_id = cats
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["id"].as_i64())
        .expect("at least one category id");

    let monday = next_monday(-14).format("%Y-%m-%d").to_string();
    let (st, body) = employee
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": monday,
                "start_time": "08:00",
                "end_time": "12:00",
                "category_id": category_id,
                "comment": "combined filter test"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create time entry");
    let entry_id = id(&body);

    let (st, body) = admin
        .get(&format!(
            "/api/v1/audit-log?table_name=time_entries&record_id={entry_id}&user_id={employee_id}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "audit query with all filters");

    let rows = body.as_array().expect("audit response must be an array");
    assert_eq!(
        rows.len(),
        1,
        "combined filters should match exactly one row"
    );
    assert_eq!(rows[0]["table_name"].as_str(), Some("time_entries"));
    assert_eq!(rows[0]["record_id"].as_i64(), Some(entry_id));
    assert_eq!(rows[0]["user_id"].as_i64(), Some(employee_id));

    app.cleanup().await;
}

#[tokio::test]
async fn audit_log_returns_empty_array_for_non_matching_filters() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .get("/api/v1/audit-log?table_name=does_not_exist&record_id=999999999")
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "non-matching filters should still be OK"
    );

    let rows = body.as_array().expect("audit response must be an array");
    assert!(
        rows.is_empty(),
        "expected empty result for non-matching filters"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn audit_log_rejects_invalid_record_id_query_param() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, _) = admin.get("/api/v1/audit-log?record_id=not-a-number").await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "invalid query parameter type should return 400"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn audit_log_is_sorted_desc_and_capped_to_500_rows() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let base = chrono::Utc::now();
    for i in 0_i64..520_i64 {
        query(
            "INSERT INTO audit_log(user_id, action, table_name, record_id, before_data, after_data, occurred_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(1_i64)
        .bind("updated")
        .bind("audit_limit_test")
        .bind(i)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(base + chrono::Duration::milliseconds(i))
        .execute(&app.state.pool)
        .await
        .expect("insert audit row");
    }

    let (st, body) = admin
        .get("/api/v1/audit-log?table_name=audit_limit_test")
        .await;
    assert_eq!(st, StatusCode::OK, "limit/sort query");

    let rows = body.as_array().expect("audit response must be an array");
    assert_eq!(rows.len(), 500, "audit list must be capped at 500 rows");
    assert_eq!(rows[0]["record_id"].as_i64(), Some(519));
    assert_eq!(rows[499]["record_id"].as_i64(), Some(20));

    for pair in rows.windows(2) {
        let current = pair[0]["occurred_at"].as_str().expect("occurred_at string");
        let next = pair[1]["occurred_at"].as_str().expect("occurred_at string");
        assert!(
            current >= next,
            "rows must be sorted descending by occurred_at"
        );
    }

    app.cleanup().await;
}
