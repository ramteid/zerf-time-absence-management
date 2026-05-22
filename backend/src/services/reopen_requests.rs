//! Reopen-request service-level helpers: auto-approval logic, notification dispatch.

use crate::audit;
use crate::error::AppResult;
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::notifications as notifications;
use crate::AppState;
use chrono::{Datelike, NaiveDate};
use serde::Serialize;

#[derive(Serialize)]
pub struct ReopenRequest {
    pub id: i64,
    pub user_id: i64,
    pub week_start: NaiveDate,
    /// Set once the request is approved or rejected (NULL while pending).
    pub reviewed_by: Option<i64>,
    pub status: String,
    pub reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub rejection_reason: Option<String>,
    pub reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub fn repo_rr_to_service(r: crate::repository::ReopenRequest) -> ReopenRequest {
    ReopenRequest {
        id: r.id,
        user_id: r.user_id,
        week_start: r.week_start,
        reviewed_by: r.reviewed_by,
        status: r.status,
        reviewed_at: r.reviewed_at,
        rejection_reason: r.rejection_reason,
        reason: r.reason,
        created_at: r.created_at,
    }
}

pub async fn audit_reopened_entries(
    pool: &crate::db::DatabasePool,
    actor_id: i64,
    affected: &[(i64, String)],
) {
    for (entry_id, prev_status) in affected {
        audit::log(
            pool,
            actor_id,
            "reopened",
            "time_entries",
            *entry_id,
            Some(serde_json::json!({"status": prev_status})),
            Some(serde_json::json!({"status":"draft"})),
        )
        .await;
    }
}

/// Collect all user-ids that should be notified as "approver" for a reopen
/// request created by `requester`.
pub async fn approver_ids_to_notify(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    let mut ids: std::collections::BTreeSet<i64> = Default::default();
    ids.extend(crate::services::auth::approval_recipient_ids(pool, requester).await);
    // Only exclude the requester when they are NOT an admin.  An admin who
    // requests a reopen for their own week still needs a notification so
    // they can approve it from the dashboard (especially when they are the
    // only admin).
    if !requester.is_admin() {
        ids.remove(&requester.id);
    }
    ids.into_iter().collect()
}

pub async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    crate::services::notifications::load_language(pool).await
}

/// If an admin acted on a request, notify all other explicitly assigned
/// approvers for the request's user so they know the item left their pending
/// queue.
#[allow(clippy::too_many_arguments)]
pub async fn notify_assigned_approvers_if_admin_acted(
    app_state: &AppState,
    language: &i18n::Language,
    requester: &User,
    request_user_id: i64,
    request_id: i64,
    action_key: &str,
    action_title_key: &str,
    action_body_key: &str,
    week_label: String,
    week_iso: &str,
    extra_params: Vec<(&'static str, String)>,
) {
    if !requester.is_admin() {
        return;
    }
    let approver_ids: Vec<i64> = match app_state.db.users.get_approver_ids(request_user_id).await {
        Ok(ids) => ids
            .into_iter()
            .filter(|approver_id| *approver_id != requester.id)
            .collect(),
        Err(_) => return,
    };
    if approver_ids.is_empty() {
        return;
    }
    let employee_full_name: String = app_state
        .db
        .reopen_requests
        .get_user_full_name(request_user_id)
        .await
        .unwrap_or_else(|_| format!("User {request_user_id}"));

    // Build frontend JSON with the employee's name (not the admin's).
    let reason = extra_params.iter().find(|(k, _)| *k == "reason").map(|(_, v)| v.as_str());
    let frontend_body = if let Some(r) = reason {
        serde_json::json!({"week": week_iso, "requester_name": employee_full_name, "reason": r})
    } else {
        serde_json::json!({"week": week_iso, "requester_name": employee_full_name})
    }
    .to_string();

    let mut params = vec![
        ("requester_name", employee_full_name),
        ("week_label", week_label),
    ];
    params.extend(extra_params);
    for approver_id in approver_ids {
        notifications::create_with_frontend_body(
            app_state,
            language,
            approver_id,
            action_key,
            action_title_key,
            action_body_key,
            params.clone(),
            &frontend_body,
            true,
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    }
}

pub fn assert_monday(d: NaiveDate) -> AppResult<()> {
    if d.weekday() != chrono::Weekday::Mon {
        return Err(crate::error::AppError::BadRequest(
            "week_start must be a Monday (ISO).".into(),
        ));
    }
    Ok(())
}
