use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::{
    admin_login, bootstrap_team_with_suffix, id, login_change_pw, next_monday, year,
};

fn balance_for_category(
    balances: &serde_json::Value,
    category_id: i64,
) -> Option<&serde_json::Value> {
    balances.as_array().and_then(|rows| {
        rows.iter()
            .find(|row| row["category_id"].as_i64() == Some(category_id))
    })
}

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
    let (st, body) = admin
        .get(&format!("/api/v1/categories/{cat_id}/users"))
        .await;
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
        .put("/api/v1/categories/9999999/users", &json!({"user_ids": []}))
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
    assert_eq!(
        st,
        StatusCode::OK,
        "re-enabled category accepts new entries"
    );

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
        .get(&format!(
            "/api/v1/absence-categories/{training_cat_id}/users"
        ))
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
        .get(&format!(
            "/api/v1/absence-categories/{training_cat_id}/users"
        ))
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
    assert_eq!(
        st,
        StatusCode::OK,
        "re-enabled absence category accepts requests: {body}"
    );
    let requested_absence_id = id(&body);

    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{training_cat_id}/users"),
            &json!({"user_ids": []}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "disable category again with live absence"
    );

    let (st, category_metadata) = emp.get("/api/v1/absence-categories").await;
    assert_eq!(st, StatusCode::OK);
    let training_metadata = category_metadata
        .as_array()
        .expect("category metadata array")
        .iter()
        .find(|c| c["id"].as_i64() == Some(training_cat_id))
        .expect("live absence category remains available for behavior lookup");
    assert_eq!(
        training_metadata["active"].as_bool(),
        Some(false),
        "access-revoked live absence category must not remain selectable"
    );
    assert_eq!(
        training_metadata["auto_approve_past"].as_bool(),
        Some(false),
        "behavior metadata must still be present for frontend lookups"
    );

    let (_, absences) = emp.get("/api/v1/absences").await;
    assert!(
        absences
            .as_array()
            .expect("absences array")
            .iter()
            .any(|absence| absence["id"].as_i64() == Some(requested_absence_id)),
        "access changes must not hide the existing absence"
    );

    app.cleanup().await;
}

/// Revoking/granting access to a leave-account category (the "Available to
/// employees" list on the category dialog) must reconcile the affected
/// user's `user_leave_accounts` row in the same request: revoke zeroes the
/// entitlement, grant restores the category default. It must also drive
/// whether the balance tile appears on `/leave-balances`: hidden once access
/// is gone and no active-or-future booking remains, but kept visible while an
/// active future booking still exists.
#[tokio::test]
async fn leave_account_access_revoke_grant_reconciles_entitlement_and_tile() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, lead_pw, emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "leaveacc").await;
    let emp = login_change_pw(&app, "emp-leaveacc@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-leaveacc@example.com", &lead_pw).await;

    // Create a brand-new leave-account category. Creation defaults every
    // existing user to enabled, so emp_id starts with the category default.
    let (st, body) = admin
        .post(
            "/api/v1/absence-categories",
            &json!({
                "name": "Leaveacc Test Account", "color": "#336699",
                "cost_type": "vacation",
                "leave_account_default_days": 4,
                "leave_account_carryover_expiry": "12-31"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create leave-account category: {body}");
    let leave_cat_id = id(&body);

    let (st, accounts) = admin
        .get(&format!("/api/v1/users/{emp_id}/leave-accounts"))
        .await;
    assert_eq!(st, StatusCode::OK);
    let emp_account = accounts
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["category_id"].as_i64() == Some(leave_cat_id))
        })
        .expect("new employee has the new leave account seeded");
    assert_eq!(
        emp_account["base_days"].as_i64(),
        Some(4),
        "new employee defaults to the category's entitlement"
    );

    let today_year = year();

    // Revoke emp's access (keep the lead and admin enabled).
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{leave_cat_id}/users"),
            &json!({"user_ids": [1, lead_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "revoke employee access");

    let (st, accounts) = admin
        .get(&format!("/api/v1/users/{emp_id}/leave-accounts"))
        .await;
    assert_eq!(st, StatusCode::OK);
    let emp_account = accounts
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["category_id"].as_i64() == Some(leave_cat_id))
        })
        .expect("revoked account row is kept, not deleted");
    assert_eq!(
        emp_account["base_days"].as_i64(),
        Some(0),
        "revoking access zeroes the base entitlement"
    );
    assert_eq!(
        emp_account["current_year_days"].as_i64(),
        Some(0),
        "revoking access clears the current-year override back to zero"
    );

    let (st, balances) = emp
        .get(&format!(
            "/api/v1/leave-balances/{emp_id}?year={today_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        balance_for_category(&balances, leave_cat_id).is_none(),
        "revoked account with no booking must not render a tile: {balances}"
    );

    // Re-grant access: entitlement must return to the category default.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{leave_cat_id}/users"),
            &json!({"user_ids": [1, lead_id, emp_id]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "re-grant employee access");

    let (st, accounts) = admin
        .get(&format!("/api/v1/users/{emp_id}/leave-accounts"))
        .await;
    assert_eq!(st, StatusCode::OK);
    let emp_account = accounts
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["category_id"].as_i64() == Some(leave_cat_id))
        })
        .expect("account row still present after re-grant");
    assert_eq!(
        emp_account["base_days"].as_i64(),
        Some(4),
        "re-granting access restores the category default"
    );
    assert_eq!(
        emp_account["current_year_days"].as_i64(),
        Some(4),
        "re-granting access seeds the current-year override to the default"
    );

    let (st, balances) = emp
        .get(&format!(
            "/api/v1/leave-balances/{emp_id}?year={today_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        balance_for_category(&balances, leave_cat_id).is_some(),
        "granted account must render a tile again: {balances}"
    );

    // Book a future absence against the new account, approved by the lead.
    let future_day = next_monday(60).format("%Y-%m-%d").to_string();
    let future_year = &future_day[..4];
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"category_id": leave_cat_id, "start_date": future_day, "end_date": future_day}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book future absence: {body}");
    let future_absence_id = id(&body);
    let (st, _) = lead
        .post(
            &format!("/api/v1/absences/{future_absence_id}/approve"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "approve future absence");

    // Revoke access again: entitlement is zeroed, but the tile must remain
    // visible because an active, future-dated booking still charges it.
    let (st, _) = admin
        .put(
            &format!("/api/v1/absence-categories/{leave_cat_id}/users"),
            &json!({"user_ids": [1, lead_id]}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "revoke employee access with a live booking"
    );

    let (st, accounts) = admin
        .get(&format!("/api/v1/users/{emp_id}/leave-accounts"))
        .await;
    assert_eq!(st, StatusCode::OK);
    let emp_account = accounts
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["category_id"].as_i64() == Some(leave_cat_id))
        })
        .expect("account row still present");
    assert_eq!(
        emp_account["base_days"].as_i64(),
        Some(0),
        "revoking access zeroes entitlement even with a live booking"
    );

    let (st, balances) = emp
        .get(&format!(
            "/api/v1/leave-balances/{emp_id}?year={future_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        balance_for_category(&balances, leave_cat_id).is_some(),
        "revoked account with an active future booking must keep its tile visible: {balances}"
    );

    app.cleanup().await;
}

/// A new user created with an explicit `absence_category_ids` list that
/// excludes a leave-account category must seed that account at zero, not the
/// category default — access and entitlement must agree from the moment of
/// creation, the same rule enforced by the category-dialog revoke/grant path.
#[tokio::test]
async fn new_user_excluded_from_leave_account_seeds_zero_entitlement() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/absence-categories",
            &json!({
                "name": "Newuser Leave Account", "color": "#996633",
                "cost_type": "vacation",
                "leave_account_default_days": 6,
                "leave_account_carryover_expiry": "12-31"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create leave-account category: {body}");
    let leave_cat_id = id(&body);

    let (st, all_categories) = admin.get("/api/v1/absence-categories/all").await;
    assert_eq!(st, StatusCode::OK);
    let granted_ids: Vec<i64> = all_categories
        .as_array()
        .expect("categories array")
        .iter()
        .filter_map(|category| category["id"].as_i64())
        .filter(|category_id| *category_id != leave_cat_id)
        .collect();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "emp-newuserleave@example.com", "first_name": "New", "last_name": "Excluded",
                "role": "employee", "weekly_hours": 39, "start_date": "2024-01-01",
                "approver_ids": [1], "absence_category_ids": granted_ids
            }),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "create user excluded from new account: {body}"
    );
    let new_user_id = id(&body);

    let (st, accounts) = admin
        .get(&format!("/api/v1/users/{new_user_id}/leave-accounts"))
        .await;
    assert_eq!(st, StatusCode::OK);
    let account = accounts
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["category_id"].as_i64() == Some(leave_cat_id))
        })
        .expect("excluded account row is still seeded, just at zero");
    assert_eq!(
        account["base_days"].as_i64(),
        Some(0),
        "a category excluded at creation seeds zero, not the category default"
    );

    let today_year = year();
    let (st, balances) = admin
        .get(&format!(
            "/api/v1/leave-balances/{new_user_id}?year={today_year}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        balance_for_category(&balances, leave_cat_id).is_none(),
        "excluded account with no access and no booking must not render a tile: {balances}"
    );

    app.cleanup().await;
}
