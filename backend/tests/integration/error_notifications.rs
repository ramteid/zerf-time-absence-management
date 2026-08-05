//! Opt-in admin error notifications: the per-user flag (create/update/coercion)
//! and the queue → worker → fan-out delivery to opted-in admins only.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::TestApp;
use crate::helpers::*;

#[tokio::test]
async fn error_notifications_opt_in_and_delivery() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // An admin who opts in to technical error notifications.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"opted-in@example.com","first_name":"Opt","last_name":"In",
                "role":"admin","weekly_hours":39,"start_date":"2024-01-01","approver_ids":[],
                "receives_error_notifications":true}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create opted-in admin");
    let opted_in_id = id(&body);

    // GET reflects the persisted flag.
    let (st, u) = admin.get(&format!("/api/v1/users/{}", opted_in_id)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        u["receives_error_notifications"], true,
        "opted-in admin flag must persist"
    );

    // A second admin who did not opt in.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"opted-out@example.com","first_name":"Opt","last_name":"Out",
                "role":"admin","weekly_hours":39,"start_date":"2024-01-01","approver_ids":[]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create opted-out admin");
    let opted_out_id = id(&body);

    // A non-admin cannot carry the flag: the service coerces it to false.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({"email":"emp-flag@example.com","first_name":"Emp","last_name":"Flag",
                "role":"employee","weekly_hours":39,"start_date":"2024-01-01","approver_ids":[1],
                "receives_error_notifications":true}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create employee");
    let emp_id = id(&body);
    let (st, u) = admin.get(&format!("/api/v1/users/{}", emp_id)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        u["receives_error_notifications"], false,
        "non-admin flag must be coerced to false"
    );

    // Enqueue a technical error and drive one worker pass (no SMTP in tests).
    app.state
        .db
        .error_queue
        .enqueue(Some("test_error_1"), "Boom", Some("something broke"), "app")
        .await
        .expect("enqueue error");
    zerf::background::error_notifications::process_pending(&app.state).await;

    // The opted-in admin received a pinned system_error notification...
    let opted_in_notes = app
        .state
        .db
        .notifications
        .list_for_user(opted_in_id)
        .await
        .unwrap();
    assert!(
        opted_in_notes
            .iter()
            .any(|n| n.kind == "system_error" && n.title == "Boom"),
        "opted-in admin must receive the error notification"
    );
    // ...while the opted-out admin (and the default-off first admin) did not...
    let opted_out_notes = app
        .state
        .db
        .notifications
        .list_for_user(opted_out_id)
        .await
        .unwrap();
    assert!(
        !opted_out_notes.iter().any(|n| n.kind == "system_error"),
        "opted-out admin must NOT receive the error notification"
    );
    // ...and the queue entry was deleted, so it is never retried.
    let pending = app.state.db.error_queue.list_pending(10).await.unwrap();
    assert!(
        pending.is_empty(),
        "queue entry must be deleted after processing even without SMTP"
    );

    // Backup producers send only a stable event key. The backend resolves all
    // visible copy from the same central templates used by app notifications.
    app.state
        .db
        .error_queue
        .enqueue(Some("backup_failed"), "", None, "backup")
        .await
        .expect("enqueue backup error");
    zerf::background::error_notifications::process_pending(&app.state).await;
    let backup_notes = app
        .state
        .db
        .notifications
        .list_for_user(opted_in_id)
        .await
        .unwrap();
    let backup_note = backup_notes
        .iter()
        .find(|notification| notification.title == "Database backup failed")
        .expect("centrally rendered backup notification");
    assert_eq!(backup_note.title, "Database backup failed");
    assert_eq!(
        backup_note.body.as_deref(),
        Some("Component: Database backup\nAction: Review the backup container logs.")
    );

    // Turning the flag off via update is honored.
    let (st, _) = admin
        .put(
            &format!("/api/v1/users/{}", opted_in_id),
            &json!({"receives_error_notifications": false}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "update opted-in admin off");
    let (st, u) = admin.get(&format!("/api/v1/users/{}", opted_in_id)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        u["receives_error_notifications"], false,
        "flag must turn off via update"
    );

    app.cleanup().await;
}
