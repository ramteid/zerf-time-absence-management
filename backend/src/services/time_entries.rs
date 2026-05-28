use crate::audit;
use crate::error::AppResult;
use crate::i18n;
use crate::middleware::auth::User;
use crate::AppState;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct TimeEntry {
    pub id: i64,
    pub user_id: i64,
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub counts_as_work: Option<bool>,
    pub comment: Option<String>,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load the UI language for notification text; falls back to English on error.
/// Delegates to the canonical implementation in `services::notifications`.
pub async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    crate::services::notifications::load_language(pool).await
}

/// Map a repository-level entry to the service-level DTO.
pub fn repo_entry_to_service(e: crate::repository::TimeEntry) -> TimeEntry {
    TimeEntry {
        id: e.id,
        user_id: e.user_id,
        entry_date: e.entry_date,
        start_time: e.start_time,
        end_time: e.end_time,
        category_id: e.category_id,
        counts_as_work: None, // filled by attach_counts_as_work
        comment: e.comment,
        status: e.status,
        submitted_at: e.submitted_at,
        reviewed_by: e.reviewed_by,
        reviewed_at: e.reviewed_at,
        rejection_reason: e.rejection_reason,
        created_at: e.created_at,
        updated_at: e.updated_at,
    }
}

/// Compute the ISO week start (Monday) for a given date.
/// Delegates to the canonical implementation in `time_calc`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    crate::time_calc::week_monday(date)
}

/// Enrich entries with the `counts_as_work` flag from their category.
/// Fetches each distinct category only once to minimise DB round-trips.
pub async fn attach_counts_as_work(app_state: &AppState, entries: &mut [TimeEntry]) -> AppResult<()> {
    let category_ids: HashSet<i64> = entries.iter().map(|e| e.category_id).collect();
    let mut map: HashMap<i64, bool> = HashMap::new();
    for cat_id in category_ids {
        let flag = app_state
            .db
            .categories
            .find_by_id(cat_id)
            .await?
            .map(|c| c.counts_as_work)
            .unwrap_or(true);
        map.insert(cat_id, flag);
    }
    for entry in entries {
        entry.counts_as_work = Some(*map.get(&entry.category_id).unwrap_or(&true));
    }
    Ok(())
}

/// Send week-level status-change notifications consolidated per user.
///
/// Groups the affected entries by owner, computes distinct ISO weeks per owner,
/// and sends one notification per user (not per entry). When `reason` is
/// `Some`, it is included as a template parameter for rejection messages.
pub async fn notify_week_status_change(
    app_state: &AppState,
    requester_id: i64,
    entries: &[crate::repository::TimeEntry],
    category: &str,
    title_key: &str,
    body_key: &str,
    reason: Option<&str>,
) {
    let language = notification_language(&app_state.pool).await;

    // Group entries by owner and collect distinct week-starts per owner.
    let mut weeks_by_user: HashMap<i64, HashSet<NaiveDate>> = HashMap::new();
    for entry in entries {
        weeks_by_user
            .entry(entry.user_id)
            .or_default()
            .insert(week_start(entry.entry_date));
    }

    // Send one consolidated notification per affected user.
    for (user_id, weeks) in weeks_by_user {
        let mut sorted_weeks: Vec<NaiveDate> = weeks.into_iter().collect();
        sorted_weeks.sort();
        let week_list = sorted_weeks
            .iter()
            .map(|ws| i18n::format_week_label(&language, *ws))
            .collect::<Vec<_>>()
            .join("\n");
        let week_count = i18n::week_count(&language, sorted_weeks.len() as i64);
        let mut params: Vec<(&'static str, String)> =
            vec![("week_list", week_list), ("week_count", week_count)];
        if let Some(r) = reason {
            params.push(("reason", r.to_string()));
        }

        // Build JSON body for frontend rendering (weeks + optional reason).
        let week_iso_strings: Vec<String> = sorted_weeks
            .iter()
            .map(|ws| ws.format("%Y-%m-%d").to_string())
            .collect();
        let frontend_body = if let Some(r) = reason {
            format!(
                "{{\"weeks\":[{}],\"reason\":{}}}",
                week_iso_strings.iter().map(|w| format!("\"{}\"", w)).collect::<Vec<_>>().join(","),
                serde_json::json!(r),
            )
        } else {
            format!(
                "{{\"weeks\":[{}]}}",
                week_iso_strings.iter().map(|w| format!("\"{}\"", w)).collect::<Vec<_>>().join(","),
            )
        };

        let send_email = user_id != requester_id;
        crate::services::notifications::create_with_frontend_body(
            app_state, &language, user_id, category, title_key, body_key, params,
            &frontend_body, send_email, Some("time_entries"), None,
        )
        .await;
    }
}

/// Return `Forbidden` when the requesting user has time tracking disabled.
/// Delegates to the canonical implementation in `services::users`.
pub fn require_tracks_time(user: &User) -> AppResult<()> {
    crate::services::users::require_tracks_time(user)
}

pub async fn create(
    app_state: &AppState,
    requester: &User,
    entry_date: NaiveDate,
    start_time: String,
    end_time: String,
    category_id: i64,
    comment: Option<String>,
) -> AppResult<TimeEntry> {
    require_tracks_time(requester)?;
    let entry_data = crate::repository::NewEntryData {
        entry_date,
        start_time,
        end_time,
        category_id,
        comment,
    };
    let created = app_state
        .db
        .time_entries
        .create(requester.id, &entry_data)
        .await?;
    let created_entry = repo_entry_to_service(created);
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "time_entries",
        created_entry.id,
        None,
        serde_json::to_value(&created_entry).ok(),
    )
    .await;
    Ok(created_entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};

    fn sample_repo_entry(id: i64, status: &str) -> crate::repository::TimeEntry {
        let now = Utc::now();
        crate::repository::TimeEntry {
            id,
            user_id: 3,
            entry_date: NaiveDate::from_ymd_opt(2026, 5, 18).unwrap(),
            start_time: "09:00".to_string(),
            end_time: "17:00".to_string(),
            category_id: 2,
            comment: Some("deep work".to_string()),
            status: status.to_string(),
            submitted_at: Some(now),
            reviewed_by: None,
            reviewed_at: None,
            rejection_reason: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Every field from the repository row must reach the service DTO unchanged;
    /// `counts_as_work` is left as `None` because it is filled later by
    /// `attach_counts_as_work` (separate DB call per distinct category).
    #[test]
    fn repo_entry_to_service_maps_all_fields() {
        let repo = sample_repo_entry(7, "submitted");
        let svc = repo_entry_to_service(repo);
        assert_eq!(svc.id, 7);
        assert_eq!(svc.user_id, 3);
        assert_eq!(svc.entry_date, NaiveDate::from_ymd_opt(2026, 5, 18).unwrap());
        assert_eq!(svc.start_time, "09:00");
        assert_eq!(svc.end_time, "17:00");
        assert_eq!(svc.category_id, 2);
        assert_eq!(svc.comment.as_deref(), Some("deep work"));
        assert_eq!(svc.status, "submitted");
        assert!(svc.counts_as_work.is_none(), "counts_as_work is filled later by attach_counts_as_work");
    }

    /// A rejected entry carries a reviewer id and a free-text reason; both
    /// must survive the repo → service mapping without mutation.
    #[test]
    fn repo_entry_to_service_preserves_rejection_reason() {
        let mut repo = sample_repo_entry(12, "rejected");
        repo.rejection_reason = Some("incorrect category".to_string());
        repo.reviewed_by = Some(1);
        let svc = repo_entry_to_service(repo);
        assert_eq!(svc.status, "rejected");
        assert_eq!(svc.rejection_reason.as_deref(), Some("incorrect category"));
        assert_eq!(svc.reviewed_by, Some(1));
    }

    /// `require_tracks_time` is a thin delegation guard; verify that
    /// `tracks_time = true` passes and `tracks_time = false` returns Forbidden.
    #[test]
    fn require_tracks_time_delegates_to_users_service() {
        use crate::middleware::auth::User;
        use chrono::Utc;
        let tracking_user = User {
            id: 1, email: "a@b.com".to_string(), password_hash: "h".to_string(),
            first_name: "A".to_string(), last_name: "B".to_string(),
            role: "employee".to_string(), weekly_hours: 40.0, workdays_per_week: 5,
            start_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            active: true, must_change_password: false, created_at: Utc::now(),
            allow_reopen_without_approval: false, dark_mode: false,
            overtime_start_balance_min: 0, tracks_time: true,
        };
        assert!(require_tracks_time(&tracking_user).is_ok());
        let mut non_tracking = tracking_user.clone();
        non_tracking.tracks_time = false;
        assert!(require_tracks_time(&non_tracking).is_err());
    }
}

pub struct TimeEntryInput {
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
}

pub async fn update(
    app_state: &AppState,
    requester: &User,
    entry_id: i64,
    input: TimeEntryInput,
) -> AppResult<TimeEntry> {
    let owner_id = app_state.db.time_entries.get_user_id(entry_id).await?;
    if owner_id == requester.id {
        require_tracks_time(requester)?;
    }
    let entry_data = crate::repository::NewEntryData {
        entry_date: input.entry_date,
        start_time: input.start_time,
        end_time: input.end_time,
        category_id: input.category_id,
        comment: input.comment,
    };
    let (prev, updated) = app_state
        .db
        .time_entries
        .update(entry_id, requester.id, requester.is_admin(), &entry_data)
        .await?;
    let previous_entry = repo_entry_to_service(prev);
    let updated_entry = repo_entry_to_service(updated);
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "time_entries",
        entry_id,
        serde_json::to_value(&previous_entry).ok(),
        serde_json::to_value(&updated_entry).ok(),
    )
    .await;
    Ok(updated_entry)
}
