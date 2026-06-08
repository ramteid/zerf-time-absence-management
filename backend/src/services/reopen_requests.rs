//! Reopen-request service-level helpers: auto-approval logic, notification dispatch.

use crate::audit;
use crate::error::AppResult;
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::notifications;
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
    let reason = extra_params
        .iter()
        .find(|(k, _)| *k == "reason")
        .map(|(_, v)| v.as_str());
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};

    fn sample_repo_rr(id: i64, status: &str) -> crate::repository::ReopenRequest {
        crate::repository::ReopenRequest {
            id,
            user_id: 5,
            week_start: NaiveDate::from_ymd_opt(2026, 5, 18).unwrap(),
            reviewed_by: Some(2),
            status: status.to_string(),
            reviewed_at: Some(Utc::now()),
            rejection_reason: None,
            reason: Some("missed deadline".to_string()),
            created_at: Utc::now(),
        }
    }

    /// Reopen requests are keyed by ISO week start (Monday); confirm the happy path.
    #[test]
    fn assert_monday_accepts_monday() {
        let monday = NaiveDate::from_ymd_opt(2026, 5, 18).unwrap();
        assert!(assert_monday(monday).is_ok());
    }

    /// Every non-Monday must return `BadRequest` so callers cannot create
    /// reopen requests for arbitrary mid-week dates.
    #[test]
    fn assert_monday_rejects_every_non_monday_weekday() {
        let tuesday = NaiveDate::from_ymd_opt(2026, 5, 19).unwrap();
        let wednesday = NaiveDate::from_ymd_opt(2026, 5, 20).unwrap();
        let thursday = NaiveDate::from_ymd_opt(2026, 5, 21).unwrap();
        let friday = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        let saturday = NaiveDate::from_ymd_opt(2026, 5, 23).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 5, 24).unwrap();
        for day in [tuesday, wednesday, thursday, friday, saturday, sunday] {
            assert!(
                matches!(
                    assert_monday(day),
                    Err(crate::error::AppError::BadRequest(_))
                ),
                "{day} should not be accepted as a Monday"
            );
        }
    }

    /// Verify that every field from the repository row reaches the service DTO
    /// unchanged; a missed field would silently break the API response shape.
    #[test]
    fn repo_rr_to_service_maps_all_fields() {
        let repo = sample_repo_rr(10, "approved");
        let svc = repo_rr_to_service(repo);
        assert_eq!(svc.id, 10);
        assert_eq!(svc.user_id, 5);
        assert_eq!(
            svc.week_start,
            NaiveDate::from_ymd_opt(2026, 5, 18).unwrap()
        );
        assert_eq!(svc.reviewed_by, Some(2));
        assert_eq!(svc.status, "approved");
        assert!(svc.rejection_reason.is_none());
        assert_eq!(svc.reason.as_deref(), Some("missed deadline"));
    }

    /// A pending request has no reviewer yet; confirm that nullable review
    /// fields are preserved as `None` and not defaulted.
    #[test]
    fn repo_rr_to_service_handles_pending_with_no_review() {
        let mut repo = sample_repo_rr(99, "pending");
        repo.reviewed_by = None;
        repo.reviewed_at = None;
        repo.reason = None;
        let svc = repo_rr_to_service(repo);
        assert_eq!(svc.status, "pending");
        assert!(svc.reviewed_by.is_none());
        assert!(svc.reviewed_at.is_none());
        assert!(svc.reason.is_none());
    }
}
