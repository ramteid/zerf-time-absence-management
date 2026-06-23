//! Scoped self-service "assistant" (Aushilfe) management for non-admin team
//! leads, gated by the `allow_team_lead_manage_assistants` admin setting.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

async fn set_team_lead_assistant_management(app: &TestApp, admin: &crate::common::TestClient, enabled: bool) {
    let (st, _) = admin
        .put(
            "/api/v1/settings",
            &json!({
                "ui_language": "en",
                "time_format": "24h",
                "country": "DE",
                "region": "DE-BW",
                "allow_team_lead_manage_assistants": enabled
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "toggle setting");
    let _ = app; // app kept for symmetry with other helpers, unused otherwise
}

#[tokio::test]
async fn team_users_scoped_assistant_management() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let (lead_id, lead_pw, emp_id, _emp_pw, _monday, _cat) =
        bootstrap_team_with_suffix(&app, &admin, false, "tu1").await;
    let lead = login_change_pw(&app, "lead-tu1@example.com", &lead_pw).await;

    // -- Disabled by default: every /team-users endpoint is forbidden --
    {
        let (st, _) = lead.get("/api/v1/team-users").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "list forbidden while disabled");

        let (st, _) = lead
            .post(
                "/api/v1/team-users",
                &json!({"email":"asst-disabled@example.com","first_name":"Ash","last_name":"Disabled",
                    "leave_days_current_year":10,"leave_days_next_year":10,"annual_leave_days":10,
                    "start_date":"2024-01-01"}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "create forbidden while disabled");
    }

    // Only an admin may flip the setting.
    {
        let (st, _) = lead
            .put(
                "/api/v1/settings",
                &json!({"ui_language":"en","time_format":"24h","country":"DE","region":"DE-BW",
                    "allow_team_lead_manage_assistants": true}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "non-admin cannot toggle setting");
    }

    set_team_lead_assistant_management(&app, &admin, true).await;

    // -- List: lead sees self + direct report, name-only for non-assistants --
    {
        let (st, body) = lead.get("/api/v1/team-users").await;
        assert_eq!(st, StatusCode::OK);
        let rows = body.as_array().unwrap();
        assert_eq!(rows.len(), 2, "self + employee, admin excluded");

        let own_row = find_by_id(&body, lead_id).expect("lead sees own row");
        assert_eq!(own_row["can_manage"], false);
        assert!(own_row.get("email").is_none(), "own row is name-only");
        assert!(own_row.get("role").is_none());

        let emp_row = find_by_id(&body, emp_id).expect("lead sees employee row");
        assert_eq!(emp_row["can_manage"], false);
        assert!(emp_row.get("email").is_none(), "employee row is name-only");
        assert!(emp_row.get("role").is_none());
    }

    // -- Create: role and approver are always forced, client overrides ignored --
    let assistant_id;
    {
        let (st, body) = lead
            .post(
                "/api/v1/team-users",
                &json!({"email":"asst-tu1@example.com","first_name":"Ash","last_name":"Helper",
                    "leave_days_current_year":10,"leave_days_next_year":10,"annual_leave_days":10,
                    "start_date":"2024-01-01",
                    "role":"admin","approver_ids":[1]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead creates assistant");
        assistant_id = id(&body);
        assert_eq!(body["user"]["role"], "assistant", "role forced to assistant");
    }

    // -- List now includes the assistant with full fields --
    {
        let (st, body) = lead.get("/api/v1/team-users").await;
        assert_eq!(st, StatusCode::OK);
        let asst_row = find_by_id(&body, assistant_id).expect("assistant visible");
        assert_eq!(asst_row["can_manage"], true);
        assert_eq!(asst_row["role"], "assistant");
        assert_eq!(asst_row["email"], "asst-tu1@example.com");
    }

    // -- Get/update the assistant --
    {
        let (st, body) = lead
            .get(&format!("/api/v1/team-users/{}", assistant_id))
            .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["role"], "assistant");

        let (st, body) = lead
            .put(
                &format!("/api/v1/team-users/{}", assistant_id),
                &json!({"first_name":"Ashley"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead updates own assistant");
        assert_eq!(body["first_name"], "Ashley");
    }

    // -- Lead cannot manage the employee (not an assistant) via /team-users --
    {
        let (st, _) = lead.get(&format!("/api/v1/team-users/{}", emp_id)).await;
        assert_eq!(st, StatusCode::FORBIDDEN);

        let (st, _) = lead
            .put(
                &format!("/api/v1/team-users/{}", emp_id),
                &json!({"first_name":"Hacked"}),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN);

        let (st, _) = lead
            .put(&format!("/api/v1/team-users/{}", emp_id), &json!({"active": false}))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN);

        // There is no delete route at all for team leads.
        let (st, _) = lead.delete(&format!("/api/v1/team-users/{}", emp_id)).await;
        assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED);
    }

    // -- A different team lead cannot manage an assistant assigned to lead 1 --
    {
        let (lead2_id, lead2_pw, _emp2_id, _emp2_pw, _monday2, _cat2) =
            bootstrap_team_with_suffix(&app, &admin, false, "tu2").await;
        let _ = lead2_id;
        let lead2 = login_change_pw(&app, "lead-tu2@example.com", &lead2_pw).await;

        let (st, _) = lead2
            .get(&format!("/api/v1/team-users/{}", assistant_id))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "unassigned assistant is inaccessible");
    }

    // -- Deactivate, then reactivate, the assistant — the lead retains full
    //    control across the toggle, and there is no delete route at all. --
    {
        let (st, body) = lead
            .put(
                &format!("/api/v1/team-users/{}", assistant_id),
                &json!({"active": false}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead deactivates own assistant");
        assert_eq!(body["active"], false);

        // The assistant stays visible (and manageable) in the list while inactive —
        // unlike every other lead-facing list, which only shows active members.
        let (st, body) = lead.get("/api/v1/team-users").await;
        assert_eq!(st, StatusCode::OK);
        let asst_row = find_by_id(&body, assistant_id)
            .expect("deactivated assistant remains visible for reactivation");
        assert_eq!(asst_row["can_manage"], true);
        assert_eq!(asst_row["active"], false);

        // The lead can still fetch and reactivate it.
        let (st, _) = lead
            .get(&format!("/api/v1/team-users/{}", assistant_id))
            .await;
        assert_eq!(st, StatusCode::OK, "deactivated assistant still reachable");

        let (st, body) = lead
            .put(
                &format!("/api/v1/team-users/{}", assistant_id),
                &json!({"active": true}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "lead reactivates own assistant");
        assert_eq!(body["active"], true);

        // No delete route exists for team leads, active or not.
        let (st, _) = lead
            .delete(&format!("/api/v1/team-users/{}", assistant_id))
            .await;
        assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED);
    }

    // -- Admin is unaffected and keeps using the regular /users endpoints --
    {
        let (st, body) = admin
            .post(
                "/api/v1/users",
                &json!({"email":"admin-created-employee@example.com","first_name":"Reg","last_name":"Ular",
                    "role":"employee","weekly_hours":39,"leave_days_current_year":30,"leave_days_next_year":30,
                    "annual_leave_days":30,"start_date":"2024-01-01","approver_ids":[lead_id]}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "admin still creates any role");
        assert_eq!(body["user"]["role"], "employee");
    }

    app.cleanup().await;
}
