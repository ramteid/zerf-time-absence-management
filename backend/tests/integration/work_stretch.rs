//! The option that refuses new time entries putting more work in a row than
//! the first break tier allows (ArbZG §4), and the guard behind it.

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
async fn new_entries_cannot_extend_work_in_a_row_past_the_break_threshold() {
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

    // -- Only new entries are checked --
    let (status, body) = book(&employee, &day(6), "08:00", "12:00", work).await;
    assert_eq!(status, StatusCode::OK, "four hours: {body}");
    let (status, body) = employee
        .put(
            &format!("/api/v1/time-entries/{}", id(&body)),
            &json!({
                "entry_date": day(6), "start_time": "08:00", "end_time": "15:00",
                "category_id": work
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "editing an entry: {body}");

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
