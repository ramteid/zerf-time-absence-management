//! End-to-end admin workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn admin_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Self submission is visible, but does not self-notify without an explicit approver --
    {
        let (_, body) = admin.get("/api/v1/categories").await;
        let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();
        let monday = next_monday(-14).format("%Y-%m-%d").to_string();
        // Admin is seeded with start_date=today; move it back so past entries work.
        let (st, _) = admin
            .put("/api/v1/users/1", &json!({"start_date": "2024-01-01"}))
            .await;
        assert_eq!(st, StatusCode::OK, "update admin start_date");
        let entry_id = create_and_submit_entry(&admin, &monday, cat_id).await;

        let (st, body) = admin.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK, "admin notifications");
        assert!(
            !body
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "timesheet_submitted"),
            "admin without explicit approver must not receive self-submission notification"
        );

        let (st, body) = admin.get("/api/v1/time-entries/all?status=submitted").await;
        assert_eq!(st, StatusCode::OK, "admin submitted entries visible");
        assert!(has_id(&body, entry_id));

        let (st, _) = admin
            .post(
                "/api/v1/time-entries/batch-approve",
                &json!({"ids": [entry_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin can approve self-submitted entry");
    }

    // -- Settings validate and persist user defaults --
    {
        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "country": "DE",
                    "region": "DE-BW",
                    "default_weekly_hours": 169,
                    "default_annual_leave_days": 30
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "invalid default hours rejected"
        );

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "de",
                    "time_format": "24h",
                    "country": "DE",
                    "region": "DE-BW",
                    "default_weekly_hours": 35.5,
                    "default_annual_leave_days": 28
                }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "valid defaults saved");

        let anon = app.client();
        let (st, body) = anon.get("/api/v1/settings/public").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["ui_language"], "de");
        assert_eq!(body["default_weekly_hours"], 35.5);
        assert_eq!(body["default_annual_leave_days"], 28);
    }

    // -- Lead with admin approver notifies admin on self submission --
    {
        let (_, body) = admin.get("/api/v1/categories").await;
        let cat_id = body.as_array().unwrap()[0]["id"].as_i64().unwrap();
        let monday = next_monday(-14).format("%Y-%m-%d").to_string();

        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"lead-with-admin-approver@example.com","first_name":"Nora","last_name":"Lead",
                    "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30, "annual_leave_days": 30,
                    "start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create lead");
        let lead_pw = temp_pw(&body);
        let lead = login_change_pw(&app, "lead-with-admin-approver@example.com", &lead_pw).await;

        let _entry_id = create_and_submit_entry(&lead, &monday, cat_id).await;

        let (st, body) = admin.get("/api/v1/notifications").await;
        assert_eq!(st, StatusCode::OK, "admin notifications");
        assert!(
            body.as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "timesheet_submitted"),
            "admin received lead submission notification"
        );
    }

    // -- Settings and SMTP permissions + validation branches --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"lead-settings@example.com","first_name":"Set","last_name":"Lead",
                    "role":"team_lead","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30, "annual_leave_days": 30,
                    "start_date":"2024-01-01","approver_ids":[1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "create lead for settings permissions");
        let lead = login_change_pw(&app, "lead-settings@example.com", &temp_pw(&body)).await;

        let (st, _) = lead.get("/api/v1/settings").await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "non-admin cannot read admin settings"
        );
        let (st, _) = lead
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03-31",
                    "organization_name": "Lead cannot update"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "non-admin cannot update admin settings"
        );
        let (st, _) = lead
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": false,
                    "smtp_host": "",
                    "smtp_from": ""
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "non-admin cannot update smtp settings"
        );
        let (st, _) = lead
            .post(
                "/api/v1/settings/smtp/test",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "smtp.example.com",
                    "smtp_from": "ops@example.com"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "non-admin cannot test smtp settings"
        );

        let (st, settings) = admin.get("/api/v1/settings").await;
        assert_eq!(st, StatusCode::OK, "admin reads full settings");
        assert!(settings.get("smtp_password_set").is_some());

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Invalid/Timezone",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03-31"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid timezone rejected");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DEU",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03-31"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "invalid country rejected");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "R".repeat(21),
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03-31"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "region length guard");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "13-01"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "invalid carryover date rejected"
        );

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03-31",
                    "organization_name": "X".repeat(201)
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "organization name length guard"
        );

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 367,
                    "carryover_expiry_date": "03-31"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "default annual leave days upper bound guard"
        );

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "carryover expiry format guard");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "DE",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "13-40"
                }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "carryover expiry range guard");

        let (st, _) = admin
            .put(
                "/api/v1/settings",
                &json!({
                    "ui_language": "en",
                    "time_format": "24h",
                    "timezone": "Europe/Berlin",
                    "country": "AT",
                    "region": "",
                    "default_weekly_hours": 39,
                    "default_annual_leave_days": 30,
                    "carryover_expiry_date": "03-31"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "settings refresh holidays when country changes"
        );

        let (st, body) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": false,
                    "smtp_host": "smtp.example.com",
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "bogus"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "invalid smtp encryption rejected"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("smtp_encryption"));

        let (st, _) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "",
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "starttls"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "smtp host required when enabling"
        );

        let (st, _) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "smtp.example.com",
                    "smtp_from": "",
                    "smtp_encryption": "starttls"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "smtp from required when enabling"
        );

        let (st, _) = admin
            .post(
                "/api/v1/settings/smtp/test",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "",
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "starttls"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "test endpoint requires smtp host"
        );

        let (st, _) = admin
            .post(
                "/api/v1/settings/smtp/test",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "smtp.example.com",
                    "smtp_from": "",
                    "smtp_encryption": "starttls"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "test endpoint requires smtp from"
        );

        let (st, _) = admin
            .post(
                "/api/v1/settings/smtp/test",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "smtp.example.com",
                    "smtp_from": "not-an-email",
                    "smtp_encryption": "starttls"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "invalid smtp from rejected by test endpoint"
        );

        let (st, body) = admin
            .post(
                "/api/v1/settings/smtp/test",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "127.0.0.1",
                    "smtp_port": 1,
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "none"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "test endpoint surfaces connection failures"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("SMTP_CONNECTION_FAILED"));

        let (st, _) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": false,
                    "smtp_host": "smtp.example.com",
                    "smtp_port": 587,
                    "smtp_username": "mailer",
                    "smtp_password": "secret-password",
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "starttls",
                    "submission_reminders_enabled": false,
                    "approval_reminders_enabled": false
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "smtp settings can store password and toggles when disabled"
        );

        let (st, settings) = admin.get("/api/v1/settings").await;
        assert_eq!(st, StatusCode::OK, "admin settings after smtp save");
        assert_eq!(settings["smtp_password_set"], true);
        assert_eq!(settings["submission_reminders_enabled"], false);
        assert_eq!(settings["approval_reminders_enabled"], false);

        let (st, _) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": false,
                    "smtp_host": "smtp.example.com",
                    "smtp_port": 587,
                    "smtp_username": "mailer",
                    "smtp_password": "",
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "starttls"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "smtp password can be explicitly cleared"
        );

        let (st, settings) = admin.get("/api/v1/settings").await;
        assert_eq!(
            st,
            StatusCode::OK,
            "admin settings after smtp password clear"
        );
        assert_eq!(settings["smtp_password_set"], false);

        let (st, _) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": false,
                    "smtp_host": "smtp.example.com",
                    "smtp_port": 587,
                    "smtp_username": "mailer",
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "starttls",
                    "submission_reminders_enabled": true,
                    "approval_reminders_enabled": true
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "smtp update falls back to stored password when omitted"
        );

        let (st, body) = admin
            .put(
                "/api/v1/settings/smtp",
                &json!({
                    "smtp_enabled": true,
                    "smtp_host": "127.0.0.1",
                    "smtp_port": 1,
                    "smtp_from": "ops@example.com",
                    "smtp_encryption": "none"
                }),
            )
            .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "smtp connection failure is surfaced when enabling"
        );
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("SMTP_CONNECTION_FAILED"));
    }

    app.cleanup().await;
}
