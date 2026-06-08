//! Integration tests for the tracks_time pure-admin mode feature.
//! Covers: creation guards, endpoint blocking, data deletion on disable,
//! auto-restore on demotion, admin approval functions unaffected, reports guards.

use chrono::Datelike;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn tracks_time_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (_, categories) = admin.get("/api/v1/categories").await;
    let cat_id = categories.as_array().unwrap()[0]["id"].as_i64().unwrap();
    let month_label = format!("{}-{:02}", year(), reference_date().month());

    // -- Create admin with tracks_time=false succeeds; non-admin tracks_time=false fails --
    let (pure_admin_id, pure_admin_pw) = {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "pure-admin@example.com",
                    "first_name": "Pure",
                    "last_name": "Admin",
                    "role": "admin",
                    "tracks_time": false,
                    "weekly_hours": 39,
                    "leave_days_current_year": 30,
                    "leave_days_next_year": 30,
                    "start_date": "2024-01-01"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin with tracks_time=false");
        let uid = id(&body);
        let pw = temp_pw(&body);

        let (st, user_body) = admin.get(&format!("/api/v1/users/{uid}")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            user_body["tracks_time"], false,
            "GET user shows tracks_time=false"
        );

        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "no-time-emp@example.com",
                    "first_name": "No",
                    "last_name": "Time",
                    "role": "employee",
                    "tracks_time": false,
                    "weekly_hours": 39,
                    "leave_days_current_year": 30,
                    "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "non-admin with tracks_time=false rejected"
        );

        let (st, _) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "no-time-lead@example.com",
                    "first_name": "No",
                    "last_name": "TimeLead",
                    "role": "team_lead",
                    "tracks_time": false,
                    "weekly_hours": 39,
                    "leave_days_current_year": 30,
                    "leave_days_next_year": 30,
                    "start_date": "2024-01-01",
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "team_lead with tracks_time=false rejected"
        );

        (uid, pw)
    };

    // -- auth/me for pure-admin exposes tracks_time=false and reduced nav --
    let pure_admin = login_change_pw(&app, "pure-admin@example.com", &pure_admin_pw).await;
    {
        let (st, me) = pure_admin.get("/api/v1/auth/me").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(me["tracks_time"], false, "me tracks_time=false");
        assert_eq!(
            me["home"], "/dashboard",
            "pure-admin home is /dashboard so they land on approvals first"
        );

        let nav_hrefs: Vec<&str> = me["nav"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["href"].as_str().unwrap())
            .collect();
        assert!(!nav_hrefs.contains(&"/time"), "pure-admin nav lacks /time");
        assert!(
            !nav_hrefs.contains(&"/absences"),
            "pure-admin nav lacks /absences"
        );
        assert!(
            nav_hrefs.contains(&"/calendar"),
            "pure-admin nav has /calendar (team absence coordination)"
        );
        assert!(
            nav_hrefs.contains(&"/dashboard"),
            "pure-admin nav has /dashboard (approvals + team views)"
        );
        assert!(
            nav_hrefs.contains(&"/reports"),
            "pure-admin nav has /reports (team reports)"
        );
        assert!(
            nav_hrefs.contains(&"/admin/settings"),
            "pure-admin nav has /admin/settings"
        );
    }

    // -- Pure-admin cannot access own time tracking endpoints --
    {
        let (st, _) = pure_admin.get("/api/v1/time-entries").await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "list time entries blocked for pure-admin"
        );

        let (st, _) = pure_admin
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": today(),
                    "start_time": "08:00",
                    "end_time": "09:00",
                    "category_id": cat_id
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "create time entry blocked for pure-admin"
        );
    }

    // -- Pure-admin cannot access own absence endpoints --
    {
        let (st, _) = pure_admin.get("/api/v1/absences").await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "list absences blocked for pure-admin"
        );

        let (st, _) = pure_admin
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": date_offset(30),
                    "end_date": date_offset(31)
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "create absence blocked for pure-admin"
        );
    }

    // -- Pure-admin cannot manage own reopen requests --
    {
        let monday = next_monday(-14).format("%Y-%m-%d").to_string();

        let (st, _) = pure_admin
            .post(
                "/api/v1/reopen-requests",
                &json!({
                    "week_start": monday,
                    "reason": "test"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "create reopen request blocked for pure-admin"
        );

        let (st, _) = pure_admin.get("/api/v1/reopen-requests").await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "list own reopen requests blocked for pure-admin"
        );
    }

    // -- Admin approval/view endpoints remain available for pure-admin --
    {
        let (st, _) = pure_admin.get("/api/v1/time-entries/all").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "list_all time entries accessible for pure-admin"
        );

        let (st, _) = pure_admin.get("/api/v1/absences/all").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "list_all absences accessible for pure-admin"
        );

        let (st, _) = pure_admin.get("/api/v1/reopen-requests/pending").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "pending reopen requests accessible for pure-admin"
        );
    }

    // -- Reports: own data blocked, other users data remains accessible --
    {
        let (_lead_id, _lead_pw, emp_id, emp_pw, _monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "rt").await;
        let _emp = login_change_pw(&app, "emp-rt@example.com", &emp_pw).await;

        let (st, _) = pure_admin
            .get(&format!("/api/v1/reports/month?month={month_label}"))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "own report blocked for pure-admin (no user_id)"
        );

        let (st, _) = pure_admin
            .get(&format!(
                "/api/v1/reports/month?month={month_label}&user_id={pure_admin_id}"
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "own report blocked for pure-admin (explicit user_id)"
        );

        let (st, _) = pure_admin
            .get(&format!(
                "/api/v1/reports/month?month={month_label}&user_id={emp_id}"
            ))
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "other user report accessible for pure-admin"
        );
    }

    // -- Disabling tracks_time deletes time entries and absences for that user --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "admin2@example.com",
                    "first_name": "Admin",
                    "last_name": "Two",
                    "role": "admin",
                    "tracks_time": true,
                    "weekly_hours": 39,
                    "leave_days_current_year": 30,
                    "leave_days_next_year": 30,
                    "start_date": "2024-01-01"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin2");
        let admin2_id = id(&body);
        let admin2_pw = temp_pw(&body);

        let admin2 = login_change_pw(&app, "admin2@example.com", &admin2_pw).await;

        let (st, _) = admin2
            .post(
                "/api/v1/time-entries",
                &json!({
                    "entry_date": date_offset(-7),
                    "start_time": "08:00",
                    "end_time": "09:00",
                    "category_id": cat_id
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin2 creates time entry");

        let (st, _) = admin2
            .post(
                "/api/v1/absences",
                &json!({
                    "kind": "vacation",
                    "start_date": date_offset(30),
                    "end_date": date_offset(31)
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin2 creates absence");

        let (st, entries) = admin2.get("/api/v1/time-entries").await;
        assert_eq!(st, StatusCode::OK);
        assert!(
            !entries.as_array().unwrap().is_empty(),
            "admin2 has time entries before disable"
        );

        let (st, absences) = admin2.get("/api/v1/absences").await;
        assert_eq!(st, StatusCode::OK);
        assert!(
            !absences.as_array().unwrap().is_empty(),
            "admin2 has absences before disable"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{admin2_id}"),
                &json!({ "tracks_time": false }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin disables admin2 tracks_time");

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{admin2_id}"),
                &json!({ "tracks_time": true }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin re-enables admin2 tracks_time");

        // Re-enabling tracks_time must reset start_date to today so the admin
        // doesn't suddenly accrue years of missed expected hours / minus flextime
        // from before they were tracking. The reset only happens when the caller
        // does not pass an explicit start_date.
        let (st, user_body) = admin.get(&format!("/api/v1/users/{admin2_id}")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            user_body["start_date"].as_str().unwrap(),
            today(),
            "start_date reset to today when re-enabling tracks_time"
        );

        let (st, entries) = admin2.get("/api/v1/time-entries").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            entries.as_array().unwrap().len(),
            0,
            "time entries deleted after tracks_time=false"
        );

        let (st, absences) = admin2.get("/api/v1/absences").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            absences.as_array().unwrap().len(),
            0,
            "absences deleted after tracks_time=false"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{admin2_id}"),
                &json!({ "tracks_time": false }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "re-disabling tracks_time is safe (idempotent on data)"
        );
    }

    // -- Demoting pure-admin to non-admin auto-restores tracks_time=true --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({
                    "email": "admin3@example.com",
                    "first_name": "Admin",
                    "last_name": "Three",
                    "role": "admin",
                    "tracks_time": false,
                    "weekly_hours": 39,
                    "leave_days_current_year": 30,
                    "leave_days_next_year": 30,
                    "start_date": "2024-01-01"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create admin3 with tracks_time=false");
        let admin3_id = id(&body);

        let (st, user_body) = admin.get(&format!("/api/v1/users/{admin3_id}")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            user_body["tracks_time"], false,
            "admin3 starts with tracks_time=false"
        );

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{admin3_id}"),
                &json!({
                    "role": "employee",
                    "approver_ids": [1]
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "demote admin3 to employee");

        let (st, user_body) = admin.get(&format!("/api/v1/users/{admin3_id}")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            user_body["tracks_time"], true,
            "tracks_time auto-restored on demotion"
        );
        assert_eq!(user_body["role"], "employee", "role demoted to employee");
    }

    // -- Existing non-admin cannot be changed to tracks_time=false --
    {
        let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
            bootstrap_team_with_suffix(&app, &admin, false, "rt2").await;

        let (st, _) = admin
            .put(
                &format!("/api/v1/users/{emp_id}"),
                &json!({ "tracks_time": false }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "setting tracks_time=false on employee rejected"
        );
    }
}

#[tokio::test]
async fn pure_admin_team_report_accessible_and_excludes_pure_admin_rows() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "pure-admin-team-report@example.com",
                "first_name": "Pure",
                "last_name": "AdminReport",
                "role": "admin",
                "tracks_time": false,
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "start_date": "2024-01-01"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create pure-admin");
    let pure_admin_id = id(&body);
    let pure_admin_pw = temp_pw(&body);

    // Ensure there is at least one tracked non-admin team member in the dataset.
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "teambug").await;

    let pure_admin =
        login_change_pw(&app, "pure-admin-team-report@example.com", &pure_admin_pw).await;
    let month_label = format!("{}-{:02}", year(), reference_date().month());

    let (st, body) = pure_admin
        .get(&format!("/api/v1/reports/team?month={month_label}"))
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "pure-admin should be able to access team report"
    );

    let rows = body.as_array().expect("team report response array");
    assert!(
        rows.iter()
            .any(|row| row["user_id"].as_i64() == Some(emp_id)),
        "team report contains tracked non-admin users"
    );
    assert!(
        rows.iter()
            .all(|row| row["user_id"].as_i64() != Some(pure_admin_id)),
        "pure-admin users are excluded from team report rows"
    );
}

/// Bug B10: team-categories endpoint must exclude pure-admin users (tracks_time=FALSE)
/// the same way the team overview report does. Before the fix, `team_category_members`
/// fetched all active users without filtering on `tracks_time`, so pure-admin users
/// appeared as empty rows in the team category breakdown while being absent from the
/// team overview — an inconsistency visible to admins.
#[tokio::test]
async fn pure_admin_excluded_from_team_category_report() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "pure-admin-cat@example.com",
                "first_name": "Pure",
                "last_name": "AdminCat",
                "role": "admin",
                "tracks_time": false,
                "weekly_hours": 39,
                "leave_days_current_year": 30,
                "leave_days_next_year": 30,
                "start_date": "2024-01-01"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create pure-admin for cat test");
    let pure_admin_cat_id = id(&body);

    // Ensure at least one tracked team member exists.
    let (_lead_id, _lead_pw, emp_id, _emp_pw, monday, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "catbug").await;

    let (st, body) = admin
        .get(&format!(
            "/api/v1/reports/team-categories?from={monday}&to={monday}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "team-categories request succeeds");
    let rows = body.as_array().expect("team-categories response array");

    assert!(
        rows.iter()
            .any(|row| row["user_id"].as_i64() == Some(emp_id)),
        "time-tracking employee is included in team category report"
    );
    assert!(
        rows.iter()
            .all(|row| row["user_id"].as_i64() != Some(pure_admin_cat_id)),
        "pure-admin users are excluded from team category report rows"
    );
}
