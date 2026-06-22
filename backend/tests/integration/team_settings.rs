//! End-to-end team settings workflow tests running in a single container for efficiency.
//! All test cases run sequentially within the same app instance.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn team_settings_full_workflow() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // -- Scope and permission checks --
    {
        let (lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat) =
            bootstrap_team_with_suffix(&app, &admin, false, "1").await;
        let lead = login_change_pw(&app, "lead-1@example.com", &lead_pw).await;

        // Non-admin lead cannot update their own reopen policy (privilege escalation guard).
        let (st, _) = lead
            .put(
                &format!("/api/v1/team-settings/{}", lead_id),
                &json!({"allow_reopen_without_approval": true, "allow_submission_without_approval": false}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN);

        // Lead can update their direct report.
        let (st, _) = lead
            .put(
                &format!("/api/v1/team-settings/{}", emp_id),
                &json!({"allow_reopen_without_approval": true, "allow_submission_without_approval": false}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);

        // Lead cannot update admin (not a direct report).
        let (st, _) = lead
            .put(
                "/api/v1/team-settings/1",
                &json!({"allow_reopen_without_approval": true, "allow_submission_without_approval": false}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN);

        // Admin can update any user including the lead.
        let (st, _) = admin
            .put(
                &format!("/api/v1/team-settings/{}", lead_id),
                &json!({"allow_reopen_without_approval": true, "allow_submission_without_approval": false}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);

        // Admin can update themselves.
        let (st, _) = admin
            .put(
                "/api/v1/team-settings/1",
                &json!({"allow_reopen_without_approval": true, "allow_submission_without_approval": false}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);

        // Lead sees themselves + their direct report.
        let (_, body) = lead.get("/api/v1/team-settings").await;
        assert_eq!(body.as_array().unwrap().len(), 2);

        // Admin sees all (admin + lead + employee = 3).
        let (_, body) = admin.get("/api/v1/team-settings").await;
        assert!(body.as_array().unwrap().len() >= 3);
    }

    // -- allow_submission_without_approval round-trips independently of the
    //    reopen policy flag --
    {
        let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat) =
            bootstrap_team_with_suffix(&app, &admin, false, "2").await;

        let (st, _) = admin
            .put(
                &format!("/api/v1/team-settings/{}", emp_id),
                &json!({"allow_reopen_without_approval": false, "allow_submission_without_approval": true}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "enable submission auto-approval only");

        let (_, body) = admin.get("/api/v1/team-settings").await;
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["user_id"] == emp_id)
            .expect("employee row present");
        assert_eq!(row["allow_submission_without_approval"], true);
        assert_eq!(
            row["allow_reopen_without_approval"], false,
            "the two policies are independent"
        );
    }

    app.cleanup().await;
}
