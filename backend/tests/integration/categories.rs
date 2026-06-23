use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{admin_login, bootstrap_team_with_suffix, id, login_change_pw, next_monday};

#[tokio::test]
async fn categories_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "cat").await;
    let emp = login_change_pw(&app, "emp-cat@example.com", &emp_pw).await;

    let (st, _) = emp.get("/api/v1/categories/all").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins can list all");

    let (st, _) = emp
        .post(
            "/api/v1/categories",
            &json!({"name": "Blocked", "color": "#112233"}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "only admins can create categories"
    );

    let (st, _) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "", "color": "#112233"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "name must be non-empty");

    let (st, _) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "Domain Focus", "color": "bad-color"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "color must be hex");

    let (st, body) = admin
        .post(
            "/api/v1/categories",
            &json!({
                "name": "Domain Focus",
                "description": "Used in integration tests",
                "color": "#112233",
                "sort_order": 99,
                "counts_as_work": true
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can create valid category");
    let category_id = id(&body);

    let (st, body) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "Domain Focus", "color": "#445566"}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::CONFLICT,
        "duplicate category names are rejected"
    );
    assert!(body.to_string().contains("Name already exists"));

    let (st, body) = admin
        .put(
            &format!("/api/v1/categories/{category_id}"),
            &json!({
                "name": " Domain Focus Updated ",
                "description": null,
                "color": "#a1B2c3",
                "sort_order": 7,
                "counts_as_work": false,
                "active": false
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin can update category");
    assert_eq!(body["name"], "Domain Focus Updated");
    assert_eq!(body["active"], false);
    assert_eq!(body["counts_as_work"], false);

    let (st, active_list) = admin.get("/api/v1/categories").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        active_list
            .as_array()
            .expect("active list")
            .iter()
            .all(|c| c["id"].as_i64() != Some(category_id)),
        "inactive categories must not appear in active list"
    );

    let (st, all_list) = admin.get("/api/v1/categories/all").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        all_list
            .as_array()
            .expect("all list")
            .iter()
            .any(|c| c["id"].as_i64() == Some(category_id) && c["active"] == false),
        "admin all-list must include inactive categories"
    );

    app.cleanup().await;
}

/// Per-employee category access: new categories default to enabled for
/// everyone, new employees default to every existing category, disabling a
/// category for one employee removes it from their dropdown and blocks new
/// time entries in it (but leaves their existing entries untouched), and
/// only admins may read/write the access list.
#[tokio::test]
async fn category_per_user_access_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, monday_iso, cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "catacc").await;
    let emp = login_change_pw(&app, "emp-catacc@example.com", &emp_pw).await;

    // A newly created employee defaults to every existing category enabled.
    let (st, body) = admin.get(&format!("/api/v1/categories/{cat_id}/users")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        body.as_array()
            .expect("user ids array")
            .iter()
            .any(|v| v.as_i64() == Some(emp_id)),
        "new employee defaults to enabled for existing categories"
    );

    // A newly created category defaults to enabled for every existing employee.
    let (st, body) = admin
        .post(
            "/api/v1/categories",
            &json!({"name": "Extra Duties", "color": "#abcdef"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let new_cat_id = id(&body);
    let (st, body) = admin
        .get(&format!("/api/v1/categories/{new_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        body.as_array()
            .expect("user ids array")
            .iter()
            .any(|v| v.as_i64() == Some(emp_id)),
        "new category defaults to enabled for existing employees"
    );

    // Non-admins cannot read or write the access list.
    let (st, _) = emp.get(&format!("/api/v1/categories/{cat_id}/users")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins read access lists");
    let (st, _) = emp
        .put(
            &format!("/api/v1/categories/{cat_id}/users"),
            &json!({"user_ids": []}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only admins write access lists");

    // A nonexistent category id is reported as 404, not silently accepted.
    let (st, _) = admin.get("/api/v1/categories/9999999/users").await;
    assert_eq!(st, StatusCode::NOT_FOUND, "unknown category id on read");
    let (st, _) = admin
        .put(
            "/api/v1/categories/9999999/users",
            &json!({"user_ids": []}),
        )
        .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "unknown category id on write");

    // An unknown employee id in the payload is rejected, not a 500.
    let (st, _) = admin
        .put(
            &format!("/api/v1/categories/{cat_id}/users"),
            &json!({"user_ids": [9999999]}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "unknown employee id in payload is rejected"
    );

    // An existing entry created before the category is disabled stays untouched.
    let work_day = next_monday(-7).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": &work_day, "start_time":"08:00","end_time":"12:00",
                "category_id": cat_id, "comment":"pre-existing"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create entry while category enabled");
    let existing_entry_id = id(&body);

    // Admin disables the category for this employee.
    let (st, _) = admin
        .put(
            &format!("/api/v1/categories/{cat_id}/users"),
            &json!({"user_ids": []}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "disable category for everyone");

    // The dropdown no longer offers it, and new entries in it are rejected.
    let (st, active_list) = emp.get("/api/v1/categories").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        active_list
            .as_array()
            .expect("active list")
            .iter()
            .all(|c| c["id"].as_i64() != Some(cat_id)),
        "disabled category must not appear in employee's dropdown"
    );
    let (st, _) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": &monday_iso, "start_time":"08:00","end_time":"12:00",
                "category_id": cat_id, "comment":"blocked"
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "disabled category rejects new entries"
    );

    // The pre-existing entry is untouched.
    let (st, body) = emp
        .get(&format!(
            "/api/v1/time-entries?from={work_day}&to={work_day}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "list existing entries: {body}");
    let still_there = body
        .as_array()
        .expect("entries array")
        .iter()
        .find(|e| e["id"].as_i64() == Some(existing_entry_id))
        .expect("pre-existing entry must still be present");
    assert_eq!(still_there["category_id"].as_i64(), Some(cat_id));

    // Re-enabling restores both.
    let (st, _) = admin
        .put(
            &format!("/api/v1/categories/{cat_id}/users"),
            &json!({"user_ids": [emp_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "re-enable category for employee");
    let (st, _) = emp
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": &monday_iso, "start_time":"13:00","end_time":"15:00",
                "category_id": cat_id, "comment":"allowed again"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "re-enabled category accepts new entries");

    app.cleanup().await;
}

/// Mirrors `category_per_user_access_workflow` for absence categories:
/// default-enabled for new employees/categories, admin-only access list, and
/// new absence requests blocked once disabled for an employee.
#[tokio::test]
async fn absence_category_per_user_access_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "abscatacc").await;
    let emp = login_change_pw(&app, "emp-abscatacc@example.com", &emp_pw).await;

    let (_, cats_body) = admin.get("/api/v1/absence-categories/all").await;
    let training_cat_id = cats_body
        .as_array()
        .expect("categories array")
        .iter()
        .find(|c| c["slug"].as_str() == Some("training"))
        .expect("training seeded category exists")["id"]
        .as_i64()
        .expect("id is number");

    // New employees default to enabled for existing absence categories.
    let (st, body) = admin
        .get(&format!("/api/v1/absence-categories/{training_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        body.as_array()
            .expect("user ids array")
            .iter()
            .any(|v| v.as_i64() == Some(emp_id)),
        "new employee defaults to enabled for existing absence categories"
    );

    // Non-admins cannot read or write the access list.
    let (st, _) = emp
        .get(&format!("/api/v1/absence-categories/{training_cat_id}/users"))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Admin disables "training" for this employee.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}/users"),
            &json!({"user_ids": []}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);

    let (st, active_list) = emp.get("/api/v1/absence-categories").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        active_list
            .as_array()
            .expect("active list")
            .iter()
            .all(|c| c["id"].as_i64() != Some(training_cat_id)),
        "disabled absence category must not appear in employee's dropdown"
    );

    let day = next_monday(40).format("%Y-%m-%d").to_string();
    let (st, _) = emp
        .post(
            "/api/v1/absences",
            &json!({"category_id": training_cat_id, "start_date": day, "end_date": day}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "disabled absence category rejects new requests"
    );

    // Re-enabling restores the ability to request it.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}/users"),
            &json!({"user_ids": [emp_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"category_id": training_cat_id, "start_date": day, "end_date": day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "re-enabled absence category accepts requests: {body}");

    app.cleanup().await;
}
