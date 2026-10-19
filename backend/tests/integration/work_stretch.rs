//! The option that refuses time entries putting more work in a row than the
//! first break tier allows (ArbZG §4) when they are created or changed, and
//! the guard behind it.

use chrono::Duration;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::{TestApp, TestClient};
use crate::helpers::*;

/// Save settings the way the admin form does: the unrelated fields stay as
/// they are, so no holiday refresh is triggered, and `extra` carries the break
/// deduction fields under test.
async fn put_break_settings(admin: &TestClient, extra: Value) -> (StatusCode, Value) {
    let (_, current) = admin.get("/api/v1/settings").await;
    let mut body = json!({
        "ui_language": current["ui_language"],
        "time_format": current["time_format"],
        "country": current["country"],
        "region": current["region"],
    });
    body.as_object_mut()
        .expect("settings body is an object")
        .extend(extra.as_object().expect("extra is an object").clone());
    admin.put("/api/v1/settings", &body).await
}

/// The migration that marks what exists when the column is added.
const WORK_STRETCH_EXEMPT_MIGRATION: &str =
    include_str!("../../migrations/051_work_stretch_exempt.sql");

async fn edit(
    client: &TestClient,
    entry_id: i64,
    day: &str,
    start: &str,
    end: &str,
    category_id: i64,
    comment: Option<&str>,
) -> (StatusCode, Value) {
    client
        .put(
            &format!("/api/v1/time-entries/{entry_id}"),
            &json!({
                "entry_date": day, "start_time": start, "end_time": end,
                "category_id": category_id, "comment": comment
            }),
        )
        .await
}

async fn exempt_flags(app: &TestApp) -> Vec<(i64, bool)> {
    sqlx::query_as("SELECT id, work_stretch_exempt FROM time_entries ORDER BY id")
        .fetch_all(&app.state.pool)
        .await
        .expect("load the exemption flags")
}

async fn book(
    client: &TestClient,
    day: &str,
    start: &str,
    end: &str,
    category_id: i64,
) -> (StatusCode, Value) {
    client
        .post(
            "/api/v1/time-entries",
            &json!({
                "entry_date": day, "start_time": start, "end_time": end,
                "category_id": category_id
            }),
        )
        .await
}

#[tokio::test]
async fn the_stretch_option_is_saved_cleared_and_kept_private() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (status, body) = admin.get("/api/v1/settings").await;
    assert_eq!(status, StatusCode::OK, "load settings: {body}");
    assert_eq!(
        body["auto_break_forbid_long_stretch"],
        json!(false),
        "off until an admin switches it on"
    );

    // Saved together with the break deduction it belongs to.
    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": true,
            "auto_break_threshold_hours": 6,
            "auto_break_deduction_minutes": 30,
            "auto_break_forbid_long_stretch": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch the option on: {body}");
    assert_eq!(body["auto_break_forbid_long_stretch"], json!(true));

    // A save that does not mention it keeps it.
    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": true,
            "auto_break_threshold_hours": 6,
            "auto_break_deduction_minutes": 30,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "save without the option: {body}");
    assert_eq!(body["auto_break_forbid_long_stretch"], json!(true));

    // An explicit false turns it off.
    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": true,
            "auto_break_threshold_hours": 6,
            "auto_break_deduction_minutes": 30,
            "auto_break_forbid_long_stretch": false,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch the option off: {body}");
    assert_eq!(body["auto_break_forbid_long_stretch"], json!(false));

    // Switching the break deduction off clears it, whatever the request says,
    // and switching the deduction back on does not bring it back.
    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": true,
            "auto_break_threshold_hours": 6,
            "auto_break_deduction_minutes": 30,
            "auto_break_forbid_long_stretch": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch the option on again: {body}");
    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": false,
            "auto_break_forbid_long_stretch": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch the deduction off: {body}");
    assert_eq!(body["auto_break_forbid_long_stretch"], json!(false));
    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": true,
            "auto_break_threshold_hours": 6,
            "auto_break_deduction_minutes": 30,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch the deduction on: {body}");
    assert_eq!(body["auto_break_forbid_long_stretch"], json!(false));

    // Only the settings form needs it, so the public settings do not carry it.
    let (status, body) = app.client().get("/api/v1/settings/public").await;
    assert_eq!(status, StatusCode::OK, "public settings: {body}");
    assert!(body.get("auto_break_forbid_long_stretch").is_none());

    app.cleanup().await;
}

#[tokio::test]
async fn entries_cannot_put_more_work_in_a_row_than_the_break_threshold_allows() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, _emp_id, emp_pw, _monday_iso, _cat_id) =
        bootstrap_team_with_suffix(&app, &admin, false, "stretch").await;
    let lead = login_change_pw(&app, "lead-stretch@example.com", &lead_pw).await;
    let employee = login_change_pw(&app, "emp-stretch@example.com", &emp_pw).await;

    let (status, categories) = employee.get("/api/v1/categories").await;
    assert_eq!(status, StatusCode::OK, "load categories: {categories}");
    let category_where = |counts_as_work: bool| {
        categories
            .as_array()
            .expect("categories array")
            .iter()
            .find(|row| row["counts_as_work"].as_bool() == Some(counts_as_work))
            .and_then(|row| row["id"].as_i64())
            .expect("category exists")
    };
    let work = category_where(true);
    let no_work = category_where(false);

    // Every scenario gets a day of its own: Monday to Sunday of one past week,
    // plus two days of the week before.
    let monday = next_monday(-14);
    let day = |offset: i64| {
        (monday + Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string()
    };

    let (status, body) = put_break_settings(
        &admin,
        json!({
            "auto_break_enabled": true,
            "auto_break_threshold_hours": 6,
            "auto_break_deduction_minutes": 30,
            "auto_break_forbid_long_stretch": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch the option on: {body}");

    // -- Exactly six hours is fine, a minute more is not --
    let (status, body) = book(&employee, &day(0), "08:00", "14:00", work).await;
    assert_eq!(status, StatusCode::OK, "six hours in a row: {body}");
    let (status, body) = book(&employee, &day(0), "14:00", "14:01", work).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "one minute too many: {body}"
    );
    assert_eq!(
        body["error"].as_str(),
        Some(
            "This entry would mean 361 min of work in a row without a break; \
             at most 360 min are allowed. \
             Leave a break of at least 15 min or shorten the entry."
        ),
        "the refusal states what it measured and what is allowed"
    );
    let (status, stored) = employee
        .get(&format!(
            "/api/v1/time-entries?from={}&to={}",
            day(0),
            day(0)
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "list the day: {stored}");
    assert_eq!(
        stored.as_array().map(Vec::len),
        Some(1),
        "the refused entry was not stored"
    );
    // A pause of 15 minutes ends the stretch; what follows is a new one.
    let (status, body) = book(&employee, &day(0), "14:15", "15:00", work).await;
    assert_eq!(status, StatusCode::OK, "after a 15-minute pause: {body}");

    // -- A shorter pause is not a break --
    let (status, body) = book(&employee, &day(1), "08:00", "12:00", work).await;
    assert_eq!(status, StatusCode::OK, "four hours: {body}");
    let (status, body) = book(&employee, &day(1), "12:14", "15:00", work).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "14-minute pause: {body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("420 min")),
        "the pause counts as part of the stretch: {body}"
    );
    let (status, body) = book(&employee, &day(1), "12:15", "15:00", work).await;
    assert_eq!(status, StatusCode::OK, "15-minute pause: {body}");

    // -- An entry that joins two stretches is measured as one --
    let (status, body) = book(&employee, &day(2), "08:00", "11:00", work).await;
    assert_eq!(status, StatusCode::OK, "first stretch: {body}");
    let (status, body) = book(&employee, &day(2), "11:30", "14:30", work).await;
    assert_eq!(status, StatusCode::OK, "second stretch: {body}");
    let (status, body) = book(&employee, &day(2), "11:00", "11:30", work).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "filling the pause: {body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("390 min")),
        "both stretches and the entry between them: {body}"
    );

    // -- What exists today is never judged, and does not block other entries --
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "false",
        )
        .await
        .expect("switch the guard off");
    let (status, body) = book(&employee, &day(3), "07:45", "14:00", work).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "booked while the option was off: {body}"
    );
    let long_entry_id = id(&body);
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "true",
        )
        .await
        .expect("switch the guard on");
    let (status, body) = book(&employee, &day(3), "15:00", "16:00", work).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an entry the long stretch does not reach: {body}"
    );
    let (status, body) = book(&employee, &day(3), "14:00", "14:30", work).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an entry that extends the long stretch: {body}"
    );
    let (status, body) = employee
        .put(
            &format!("/api/v1/time-entries/{long_entry_id}"),
            &json!({
                "entry_date": day(3), "start_time": "07:45", "end_time": "14:00",
                "category_id": work, "comment": "edited afterwards"
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the existing entry stays editable: {body}"
    );
    let (status, body) = employee
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [long_entry_id]}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the existing entry stays submittable: {body}"
    );

    // -- Only work forms a stretch --
    let (status, body) = book(&employee, &day(4), "08:00", "14:00", work).await;
    assert_eq!(status, StatusCode::OK, "six hours of work: {body}");
    let (status, body) = book(&employee, &day(4), "14:00", "17:00", no_work).await;
    assert_eq!(status, StatusCode::OK, "time that is not work: {body}");
    let (status, body) = book(&employee, &day(4), "17:00", "17:30", work).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "work after time that is not work starts a new stretch: {body}"
    );

    // -- A rejected entry is not part of any stretch --
    let (status, body) = book(&employee, &day(5), "08:00", "14:00", work).await;
    assert_eq!(status, StatusCode::OK, "six hours: {body}");
    let rejected_id = id(&body);
    let (status, body) = employee
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [rejected_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "submit: {body}");
    let (status, body) = lead
        .post(
            "/api/v1/time-entries/batch-reject",
            &json!({"ids": [rejected_id], "reason": "Wrong times"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "reject: {body}");
    let (status, body) = book(&employee, &day(5), "14:00", "14:30", work).await;
    assert_eq!(status, StatusCode::OK, "next to a rejected entry: {body}");

    // -- A new entry cannot be stretched past the limit afterwards either --
    let (status, body) = book(&employee, &day(6), "08:00", "12:00", work).await;
    assert_eq!(status, StatusCode::OK, "four hours: {body}");
    let stretched_id = id(&body);
    let (status, body) = edit(
        &employee,
        stretched_id,
        &day(6),
        "08:00",
        "15:00",
        work,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "stretching it to seven hours: {body}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("420 min")),
        "the refusal measures the entry as it would be: {body}"
    );
    let (status, body) = edit(
        &employee,
        stretched_id,
        &day(6),
        "08:00",
        "14:00",
        work,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "stretching it to exactly six hours: {body}"
    );

    // -- Booked while the option was off: checked once it changes, not for a note --
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "false",
        )
        .await
        .expect("switch the guard off");
    let (status, body) = book(&employee, &day(-5), "08:00", "14:30", work).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "booked while the option was off: {body}"
    );
    let over_id = id(&body);
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "true",
        )
        .await
        .expect("switch the guard on");
    let (status, body) = edit(
        &employee,
        over_id,
        &day(-5),
        "08:00",
        "14:30",
        work,
        Some("a note"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a note is no change to what is worked: {body}"
    );
    let (status, body) = edit(&employee, over_id, &day(-5), "08:00", "14:15", work, None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "still over the limit after the change: {body}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("375 min")),
        "{body}"
    );
    let (status, body) = edit(&employee, over_id, &day(-5), "08:00", "14:00", work, None).await;
    assert_eq!(status, StatusCode::OK, "back within the limit: {body}");

    // -- What predates the guard is never refused when it changes --
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "false",
        )
        .await
        .expect("switch the guard off");
    let (status, body) = book(&employee, &day(-4), "08:00", "14:30", work).await;
    assert_eq!(status, StatusCode::OK, "booked before the guard: {body}");
    let legacy_id = id(&body);
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "true",
        )
        .await
        .expect("switch the guard on");
    sqlx::query("UPDATE time_entries SET work_stretch_exempt = TRUE WHERE id = $1")
        .bind(legacy_id)
        .execute(&app.state.pool)
        .await
        .expect("mark the entry as predating the guard");
    let (status, body) = edit(&employee, legacy_id, &day(-4), "08:00", "16:00", work, None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an entry that predates the guard may still be stretched: {body}"
    );

    // -- An admin's correction follows the same rule --
    let (status, body) = book(&employee, &day(-3), "08:00", "12:00", work).await;
    assert_eq!(status, StatusCode::OK, "four hours: {body}");
    let corrected_id = id(&body);
    let (status, body) = employee
        .post(
            "/api/v1/time-entries/submit",
            &json!({"ids": [corrected_id]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "submit: {body}");
    let (status, body) = edit(&admin, corrected_id, &day(-3), "08:00", "15:00", work, None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "correcting it to seven hours: {body}"
    );
    let (status, body) = edit(&admin, corrected_id, &day(-3), "08:00", "13:00", work, None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "correcting it to five hours: {body}"
    );

    // -- Off means off --
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "false",
        )
        .await
        .expect("switch the guard off");
    let (status, body) = book(&employee, &day(-7), "08:00", "16:00", work).await;
    assert_eq!(status, StatusCode::OK, "option off: {body}");

    // The option is meaningless without the break deduction it is measured by.
    app.state
        .db
        .settings
        .save_setting(
            zerf::services::settings::AUTO_BREAK_FORBID_LONG_STRETCH_KEY,
            "true",
        )
        .await
        .expect("switch the guard on");
    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::AUTO_BREAK_ENABLED_KEY, "false")
        .await
        .expect("switch the break deduction off");
    let (status, body) = book(&employee, &day(-6), "08:00", "16:00", work).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "break deduction off, so no break threshold to measure against: {body}"
    );

    app.cleanup().await;
}

#[tokio::test]
async fn entries_that_exist_when_the_column_is_added_are_exempt_and_later_ones_are_not() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, _emp_id, emp_pw, _monday_iso, work) =
        bootstrap_team_with_suffix(&app, &admin, false, "exempt").await;
    let employee = login_change_pw(&app, "emp-exempt@example.com", &emp_pw).await;
    let monday = next_monday(-14);
    let day = |offset: i64| {
        (monday + Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string()
    };

    // Two entries booked before the guard existed ...
    let (status, body) = book(&employee, &day(0), "08:00", "10:00", work).await;
    assert_eq!(status, StatusCode::OK, "first entry: {body}");
    let first_id = id(&body);
    let (status, body) = book(&employee, &day(1), "08:00", "10:00", work).await;
    assert_eq!(status, StatusCode::OK, "second entry: {body}");
    let second_id = id(&body);

    // ... which is the database as it was before the migration: the entries,
    // without the column.
    sqlx::query("ALTER TABLE time_entries DROP COLUMN work_stretch_exempt")
        .execute(&app.state.pool)
        .await
        .expect("take the column away");
    sqlx::raw_sql(WORK_STRETCH_EXEMPT_MIGRATION)
        .execute(&app.state.pool)
        .await
        .expect("run the migration");
    assert_eq!(
        exempt_flags(&app).await,
        vec![(first_id, true), (second_id, true)],
        "what exists when the column is added is exempt"
    );

    // An entry booked afterwards is not, and running the migration again does
    // not change that.
    let (status, body) = book(&employee, &day(2), "08:00", "10:00", work).await;
    assert_eq!(status, StatusCode::OK, "third entry: {body}");
    let third_id = id(&body);
    sqlx::raw_sql(WORK_STRETCH_EXEMPT_MIGRATION)
        .execute(&app.state.pool)
        .await
        .expect("run the migration again");
    assert_eq!(
        exempt_flags(&app).await,
        vec![(first_id, true), (second_id, true), (third_id, false)],
        "a second run exempts nothing that was booked later"
    );

    app.cleanup().await;
}
