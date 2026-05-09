use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashSet;

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(error) => {
            tracing::warn!(target:"zerf::time_entries", "load notification language failed: {error}");
            i18n::Language::default()
        }
    }
}

fn repo_entry_to_service(e: crate::repository::TimeEntry) -> TimeEntry {
    TimeEntry {
        id: e.id,
        user_id: e.user_id,
        entry_date: e.entry_date,
        start_time: e.start_time,
        end_time: e.end_time,
        category_id: e.category_id,
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

#[derive(FromRow, Serialize, Clone)]
pub struct TimeEntry {
    pub id: i64,
    pub user_id: i64,
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn parse_time(time_str: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(time_str, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(time_str, "%H:%M:%S"))
        .map_err(|_| AppError::BadRequest(format!("Invalid time: {time_str}")))
}

fn duration_min(start: &str, end: &str) -> AppResult<i64> {
    let start_time = parse_time(start)?;
    let end_time = parse_time(end)?;
    if end_time <= start_time {
        return Err(AppError::BadRequest(
            "End time must be after start time.".into(),
        ));
    }
    Ok((end_time - start_time).num_minutes())
}

fn week_start(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

#[derive(Deserialize)]
pub struct RangeQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub user_id: Option<i64>,
    pub status: Option<String>,
}

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
    let mapped: Vec<TimeEntry> = entries.into_iter().map(repo_entry_to_service).collect();
    Ok(Json(mapped))
}

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
    let mapped: Vec<TimeEntry> = entries.into_iter().map(repo_entry_to_service).collect();
    Ok(Json(mapped))
}

#[derive(Deserialize)]
pub struct NewTimeEntry {
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
}

pub(crate) async fn validate(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    te: &NewTimeEntry,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    if let Some(c) = &te.comment {
        if c.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long (max 2000).".into()));
        }
    }
    // Reject entries before the user's start_date.
    let user_start: chrono::NaiveDate =
        sqlx::query_scalar("SELECT start_date FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *conn)
            .await?;
    if te.entry_date < user_start {
        return Err(AppError::BadRequest(
            "Entry date is before user start date.".into(),
        ));
    }
    // Validate that the category exists and is active.
    let cat_active: Option<bool> =
        sqlx::query_scalar("SELECT active FROM categories WHERE id = $1")
            .bind(te.category_id)
            .fetch_optional(&mut *conn)
            .await?;
    if cat_active.is_none() {
        return Err(AppError::BadRequest("Category not found.".into()));
    }
    if cat_active == Some(false) {
        return Err(AppError::BadRequest("Category is inactive.".into()));
    }
    if te.entry_date > chrono::Utc::now().date_naive() {
        return Err(AppError::BadRequest(
            "Entries in the future are not allowed.".into(),
        ));
    }
    let new_min = duration_min(&te.start_time, &te.end_time)?;
    let start_n = parse_time(&te.start_time)?;
    let end_n = parse_time(&te.end_time)?;

    let existing_entries: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, start_time, end_time, status FROM time_entries WHERE user_id=$1 AND entry_date=$2",
    )
    .bind(user_id)
    .bind(te.entry_date)
    .fetch_all(&mut *conn)
    .await?;

    let mut day_total = new_min;
    for (existing_id, start_str, end_str, status) in &existing_entries {
        // Skip the entry being edited and rejected entries (they are void).
        if Some(*existing_id) == exclude_id || status == "rejected" {
            continue;
        }
        let existing_start = parse_time(start_str)?;
        let existing_end = parse_time(end_str)?;
        if start_n < existing_end && existing_start < end_n {
            return Err(AppError::BadRequest(
                "Overlap with an existing entry.".into(),
            ));
        }
        day_total += (existing_end - existing_start).num_minutes();
    }
    if day_total > 14 * 60 {
        return Err(AppError::BadRequest("Day total exceeds 14 hours.".into()));
    }
    // Prevent time entries on days with approved absences (vacation, unpaid,
    // training, special_leave, general_absence). Sick days are excluded from
    // this check because partial sick days with work are common.
    let absence_on_day: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM absences WHERE user_id=$1 \
         AND status IN ('approved','cancellation_pending') \
         AND start_date <= $2 AND end_date >= $2 AND kind <> 'sick' LIMIT 1",
    )
    .bind(user_id)
    .bind(te.entry_date)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(kind) = absence_on_day {
        return Err(AppError::BadRequest(format!(
            "Cannot log time on a day with an approved absence ({kind}). \
             Please cancel or adjust the absence first."
        )));
    }
    Ok(())
}

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
        Some(serde_json::to_value(&created_entry).unwrap()),
    )
    .await;
    Ok(Json(created_entry))
}

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
        Some(serde_json::to_value(&previous_entry).unwrap()),
        Some(serde_json::to_value(&updated_entry).unwrap()),
    )
    .await;
    Ok(Json(updated_entry))
}

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
        Some(serde_json::to_value(&time_entry).unwrap()),
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct IdsBody {
    pub ids: Vec<i64>,
}

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
    let submitted_count = submitted_ids.len();
    let mut submitted_weeks = HashSet::new();
    for entry_id in &submitted_ids {
        if let Some(entry_date) = app_state.db.time_entries.get_date_for_entry(*entry_id).await? {
            submitted_weeks.insert(week_start(entry_date));
        }
    }
    let submitted_week_count = submitted_weeks.len();
    // Phase 4: notify the approver with the actual submitted count.
    if submitted_week_count > 0 {
        let approver_ids = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
        let language = notification_language(&app_state.pool).await;
        for approver_id in approver_ids {
            crate::notifications::create_translated(
                &app_state,
                &language,
                approver_id,
                "timesheet_submitted",
                "timesheet_submitted_title",
                "timesheet_submitted_body",
                vec![
                    (
                        "submitter_name",
                        format!("{} {}", requester.first_name, requester.last_name),
                    ),
                    (
                        "week_count",
                        i18n::week_count(&language, submitted_week_count as i64),
                    ),
                ],
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

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let entry = app_state
        .db
        .time_entries
        .approve(entry_id, requester.id, requester.is_admin())
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "time_entries",
        entry_id,
        Some(serde_json::to_value(&entry).unwrap()),
        Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    crate::notifications::create_translated(
        &app_state,
        &language,
        entry.user_id,
        "timesheet_approved",
        "timesheet_approved_title",
        "timesheet_approved_body",
        vec![("entry_date", i18n::format_date(&language, entry.entry_date))],
        Some("time_entries"),
        Some(entry_id),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

pub async fn reject(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
    Json(body): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if body.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if body.reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long (max 2000).".into()));
    }
    let entry = app_state
        .db
        .time_entries
        .reject(entry_id, requester.id, requester.is_admin(), &body.reason)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "time_entries",
        entry_id,
        Some(serde_json::to_value(&entry).unwrap()),
        Some(serde_json::json!({"status": "rejected", "reason": body.reason})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    crate::notifications::create_translated(
        &app_state,
        &language,
        entry.user_id,
        "timesheet_rejected",
        "timesheet_rejected_title",
        "timesheet_rejected_body",
        vec![
            ("entry_date", i18n::format_date(&language, entry.entry_date)),
            ("reason", body.reason.clone()),
        ],
        Some("time_entries"),
        Some(entry_id),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

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
    let approved_count = approved_entries.len();
    for entry in &approved_entries {
        audit::log(
            &app_state.pool,
            requester.id,
            "approved",
            "time_entries",
            entry.id,
            Some(serde_json::to_value(entry).unwrap()),
            Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
        )
        .await;
    }
    if approved_count > 0 {
        let language = notification_language(&app_state.pool).await;
        let mut weeks_by_user: std::collections::HashMap<i64, HashSet<NaiveDate>> =
            std::collections::HashMap::new();
        for entry in &approved_entries {
            weeks_by_user
                .entry(entry.user_id)
                .or_default()
                .insert(week_start(entry.entry_date));
        }
        for (user_id, weeks) in weeks_by_user {
            crate::notifications::create_translated(
                &app_state,
                &language,
                user_id,
                "timesheet_approved",
                "timesheet_approved_title",
                "timesheet_batch_approved_body",
                vec![("week_count", i18n::week_count(&language, weeks.len() as i64))],
                Some("time_entries"),
                None,
            )
            .await;
        }
    }
    Ok(Json(
        serde_json::json!({"ok":true, "count": approved_count}),
    ))
}

#[derive(Deserialize)]
pub struct BatchRejectBody {
    pub ids: Vec<i64>,
    pub reason: String,
}

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
        .batch_reject(&body.ids, requester.id, requester.is_admin(), &rejection_reason)
        .await?;
    let rejected_count = rejected_entries.len();
    for entry in &rejected_entries {
        audit::log(
            &app_state.pool,
            requester.id,
            "rejected",
            "time_entries",
            entry.id,
            Some(serde_json::to_value(entry).unwrap()),
            Some(serde_json::json!({"status": "rejected", "reason": rejection_reason})),
        )
        .await;
        let language = notification_language(&app_state.pool).await;
        crate::notifications::create_translated(
            &app_state,
            &language,
            entry.user_id,
            "timesheet_rejected",
            "timesheet_rejected_title",
            "timesheet_rejected_body",
            vec![
                ("entry_date", i18n::format_date(&language, entry.entry_date)),
                ("reason", rejection_reason.clone()),
            ],
            Some("time_entries"),
            Some(entry.id),
        )
        .await;
    }
    Ok(Json(
        serde_json::json!({"ok": true, "count": rejected_count}),
    ))
}
