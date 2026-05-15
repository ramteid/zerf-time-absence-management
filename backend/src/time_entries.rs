use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load the UI language for notification text; falls back to English on error.
async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(lang) => lang,
        Err(err) => {
            tracing::warn!(target: "zerf::time_entries", "load notification language failed: {err}");
            i18n::Language::default()
        }
    }
}

/// Map a repository-level entry to the handler-level DTO.
fn repo_entry_to_service(e: crate::repository::TimeEntry) -> TimeEntry {
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
fn week_start(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// Enrich entries with the `counts_as_work` flag from their category.
/// Fetches each distinct category only once to minimise DB round-trips.
async fn attach_counts_as_work(app_state: &AppState, entries: &mut [TimeEntry]) -> AppResult<()> {
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
async fn notify_week_status_change(
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
        crate::notifications::create_with_frontend_body(
            app_state, &language, user_id, category, title_key, body_key, params,
            &frontend_body, send_email, Some("time_entries"), None,
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(FromRow, Serialize, Clone)]
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

#[derive(Deserialize)]
pub struct RangeQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub user_id: Option<i64>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct NewTimeEntry {
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct IdsBody {
    pub ids: Vec<i64>,
}

#[derive(Deserialize)]
pub struct BatchRejectBody {
    pub ids: Vec<i64>,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// CRUD handlers
// ---------------------------------------------------------------------------

/// List time entries for the requesting user, optionally filtered by date range.
pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    let entries = app_state
        .db
        .time_entries
        .list_for_user(requester.id, query.from, query.to)
        .await?;
    let mut mapped: Vec<TimeEntry> = entries.into_iter().map(repo_entry_to_service).collect();
    attach_counts_as_work(&app_state, &mut mapped).await?;
    Ok(Json(mapped))
}

/// List time entries across all users (leads/admins only).
/// Admins see everything; team leads see only their direct reports.
pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let entries = app_state
        .db
        .time_entries
        .list_all(
            requester.is_admin(),
            requester.id,
            query.from,
            query.to,
            query.user_id,
            query.status,
        )
        .await?;
    let mut mapped: Vec<TimeEntry> = entries.into_iter().map(repo_entry_to_service).collect();
    attach_counts_as_work(&app_state, &mut mapped).await?;
    Ok(Json(mapped))
}

/// Validate a time entry payload against business rules (overlap, date constraints).
/// Used by both create and change-request flows.
pub(crate) async fn validate(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    te: &NewTimeEntry,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    let entry = crate::repository::NewEntryData {
        entry_date: te.entry_date,
        start_time: te.start_time.clone(),
        end_time: te.end_time.clone(),
        category_id: te.category_id,
        comment: te.comment.clone(),
    };
    crate::repository::time_entries::validate_entry(conn, user_id, &entry, exclude_id).await
}

/// Create a new draft time entry for the requesting user.
pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewTimeEntry>,
) -> AppResult<Json<TimeEntry>> {
    let entry_data = crate::repository::NewEntryData {
        entry_date: body.entry_date,
        start_time: body.start_time,
        end_time: body.end_time,
        category_id: body.category_id,
        comment: body.comment,
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
    Ok(Json(created_entry))
}

/// Update a draft time entry. Only the owner (or an admin) may edit.
pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
    Json(body): Json<NewTimeEntry>,
) -> AppResult<Json<TimeEntry>> {
    let entry_data = crate::repository::NewEntryData {
        entry_date: body.entry_date,
        start_time: body.start_time,
        end_time: body.end_time,
        category_id: body.category_id,
        comment: body.comment,
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
    Ok(Json(updated_entry))
}

/// Delete a draft time entry. Only the owner may delete their own entries.
pub async fn delete(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let owner_id = app_state.db.time_entries.get_user_id(entry_id).await?;
    if owner_id != requester.id {
        return Err(AppError::Forbidden);
    }
    let deleted = app_state.db.time_entries.delete(entry_id).await?;
    let time_entry = repo_entry_to_service(deleted);
    audit::log(
        &app_state.pool,
        requester.id,
        "deleted",
        "time_entries",
        entry_id,
        serde_json::to_value(&time_entry).ok(),
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------------------------------------------------------------------------
// Week-level submission, approval, and rejection
// ---------------------------------------------------------------------------

/// Submit draft entries for approval. The employee selects entries by ID;
/// the backend transitions them from draft → submitted in a single transaction
/// and notifies all assigned approvers.
pub async fn submit(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    if body.ids.len() > 500 {
        return Err(AppError::BadRequest("Too many entries (max 500).".into()));
    }
    // Phase 1: validate ownership for ALL entries before any writes, so a
    // mixed-ownership batch never partially submits.
    for entry_id in &body.ids {
        let owner_id = app_state.db.time_entries.get_user_id(*entry_id).await?;
        if owner_id != requester.id {
            return Err(AppError::Forbidden);
        }
    }
    // Phase 2: atomically submit all draft entries in a single transaction.
    let submitted_ids = app_state
        .db
        .time_entries
        .submit_batch(requester.id, &body.ids)
        .await?;
    // Phase 3: audit logs (best-effort, after commit).
    for entry_id in &submitted_ids {
        audit::log(
            &app_state.pool,
            requester.id,
            "status_changed",
            "time_entries",
            *entry_id,
            Some(serde_json::json!({"status": "draft"})),
            Some(serde_json::json!({"status": "submitted"})),
        )
        .await;
    }
    // Phase 4: notify approvers with a consolidated week count.
    let submitted_count = submitted_ids.len();
    let mut submitted_weeks = HashSet::new();
    for entry_id in &submitted_ids {
        if let Some(entry_date) = app_state
            .db
            .time_entries
            .get_date_for_entry(*entry_id)
            .await?
        {
            submitted_weeks.insert(week_start(entry_date));
        }
    }
    if !submitted_weeks.is_empty() {
        let approver_ids =
            crate::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
        let language = notification_language(&app_state.pool).await;
        let mut sorted_weeks: Vec<NaiveDate> = submitted_weeks.into_iter().collect();
        sorted_weeks.sort();
        let week_list = sorted_weeks
            .iter()
            .map(|ws| i18n::format_week_label(&language, *ws))
            .collect::<Vec<_>>()
            .join("\n");
        let week_count = i18n::week_count(&language, sorted_weeks.len() as i64);
        let submitter_name = format!("{} {}", requester.first_name, requester.last_name);

        // Build JSON body for frontend rendering.
        let week_iso_strings: Vec<String> = sorted_weeks
            .iter()
            .map(|ws| ws.format("%Y-%m-%d").to_string())
            .collect();
        let frontend_body = format!(
            "{{\"submitter_name\":{},\"weeks\":[{}]}}",
            serde_json::json!(&submitter_name),
            week_iso_strings.iter().map(|w| format!("\"{}\"", w)).collect::<Vec<_>>().join(","),
        );

        for approver_id in approver_ids {
            crate::notifications::create_with_frontend_body(
                &app_state,
                &language,
                approver_id,
                "timesheet_submitted",
                "timesheet_submitted_title",
                "timesheet_submitted_body",
                vec![
                    ("submitter_name", submitter_name.clone()),
                    ("week_list", week_list.clone()),
                    ("week_count", week_count.clone()),
                ],
                &frontend_body,
                true,
                Some("time_entries"),
                None,
            )
            .await;
        }
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": submitted_count}),
    ))
}

/// Approve submitted entries in batch (week-level approval).
/// Only leads (team_lead / admin) may approve. Admins can approve any user;
/// team leads can only approve their direct reports. Entries that are not in
/// "submitted" status or not under the reviewer's purview are silently skipped.
pub async fn batch_approve(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    if body.ids.len() > 500 {
        return Err(AppError::BadRequest("Too many entries (max 500).".into()));
    }
    let approved_entries = app_state
        .db
        .time_entries
        .batch_approve(&body.ids, requester.id, requester.is_admin())
        .await?;
    // Audit each entry individually for traceability.
    for entry in &approved_entries {
        audit::log(
            &app_state.pool,
            requester.id,
            "approved",
            "time_entries",
            entry.id,
            serde_json::to_value(entry).ok(),
            Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
        )
        .await;
    }
    // Send one consolidated notification per affected user.
    if !approved_entries.is_empty() {
        notify_week_status_change(
            &app_state,
            requester.id,
            &approved_entries,
            "timesheet_approved",
            "timesheet_approved_title",
            "timesheet_batch_approved_body",
            None,
        )
        .await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": approved_entries.len()}),
    ))
}

/// Reject submitted entries in batch (week-level rejection).
/// Same authorization rules as batch_approve. A rejection reason is required.
pub async fn batch_reject(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<BatchRejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rejection_reason = body.reason.trim().to_string();
    if rejection_reason.is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if rejection_reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    if body.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    if body.ids.len() > 500 {
        return Err(AppError::BadRequest("Too many entries (max 500).".into()));
    }
    let rejected_entries = app_state
        .db
        .time_entries
        .batch_reject(
            &body.ids,
            requester.id,
            requester.is_admin(),
            &rejection_reason,
        )
        .await?;
    // Audit each rejected entry individually for traceability.
    for entry in &rejected_entries {
        audit::log(
            &app_state.pool,
            requester.id,
            "rejected",
            "time_entries",
            entry.id,
            serde_json::to_value(entry).ok(),
            Some(serde_json::json!({"status": "rejected", "reason": rejection_reason})),
        )
        .await;
    }
    // Send one consolidated rejection notification per affected user.
    if !rejected_entries.is_empty() {
        notify_week_status_change(
            &app_state,
            requester.id,
            &rejected_entries,
            "timesheet_rejected",
            "timesheet_rejected_title",
            "timesheet_batch_rejected_body",
            Some(&rejection_reason),
        )
        .await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": rejected_entries.len()}),
    ))
}
