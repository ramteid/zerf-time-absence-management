use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(error) => {
            tracing::warn!(target:"zerf::time_entries", "load notification language failed: {error}");
            i18n::Language::default()
        }
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
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE user_id = ");
    builder.push_bind(requester.id);
    if let Some(from_date) = query.from {
        builder.push(" AND entry_date >= ").push_bind(from_date);
    }
    if let Some(to_date) = query.to {
        builder.push(" AND entry_date <= ").push_bind(to_date);
    }
    builder.push(" ORDER BY entry_date, start_time");
    Ok(Json(
        builder
            .build_query_as::<TimeEntry>()
            .fetch_all(&app_state.pool)
            .await?,
    ))
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE TRUE");
    // Team leads only see entries from their direct reports; admins see all.
    if !requester.is_admin() {
        builder
            .push(" AND user_id IN (SELECT id FROM users WHERE approver_id = ")
            .push_bind(requester.id)
            .push(" AND role != 'admin')");
    }
    if let Some(from_date) = query.from {
        builder.push(" AND entry_date >= ").push_bind(from_date);
    }
    if let Some(to_date) = query.to {
        builder.push(" AND entry_date <= ").push_bind(to_date);
    }
    if let Some(filter_user_id) = query.user_id {
        builder.push(" AND user_id = ").push_bind(filter_user_id);
    }
    if let Some(filter_status) = query.status {
        builder.push(" AND status = ").push_bind(filter_status);
    }
    builder.push(" ORDER BY entry_date DESC, start_time");
    Ok(Json(
        builder
            .build_query_as::<TimeEntry>()
            .fetch_all(&app_state.pool)
            .await?,
    ))
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
    if te.entry_date > chrono::Local::now().date_naive() {
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
            return Err(AppError::BadRequest("Overlap with an existing entry.".into()));
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
        "SELECT kind FROM absences WHERE user_id=$1 AND status='approved' \
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
    let mut tx = app_state.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(requester.id)
        .execute(&mut *tx)
        .await?;
    validate(&mut *tx, requester.id, &body, None).await?;
    let new_entry_id: i64 = sqlx::query_scalar("INSERT INTO time_entries(user_id, entry_date, start_time, end_time, category_id, comment) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(requester.id).bind(body.entry_date).bind(&body.start_time).bind(&body.end_time).bind(body.category_id).bind(&body.comment)
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let created_entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(new_entry_id)
        .fetch_one(&app_state.pool)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "time_entries",
        new_entry_id,
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
    let entry_owner_id: i64 = sqlx::query_scalar("SELECT user_id FROM time_entries WHERE id=$1")
        .bind(entry_id)
        .fetch_one(&app_state.pool)
        .await?;
    let mut tx = app_state.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(entry_owner_id)
        .execute(&mut *tx)
        .await?;
    let previous_entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 FOR UPDATE")
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await?;
    let admin_correction = requester.is_admin()
        && previous_entry.user_id != requester.id
        && (previous_entry.status == "approved" || previous_entry.status == "submitted");
    if !admin_correction {
        if previous_entry.user_id != requester.id {
            return Err(AppError::Forbidden);
        }
        if previous_entry.status != "draft" {
            return Err(AppError::BadRequest(
                "Only drafts can be edited directly. Please file a change request.".into(),
            ));
        }
    }
    validate(&mut *tx, previous_entry.user_id, &body, Some(entry_id)).await?;
    sqlx::query("UPDATE time_entries SET entry_date=$1, start_time=$2, end_time=$3, category_id=$4, comment=$5, updated_at=CURRENT_TIMESTAMP WHERE id=$6")
        .bind(body.entry_date).bind(&body.start_time).bind(&body.end_time).bind(body.category_id).bind(&body.comment).bind(entry_id)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    let updated_entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(entry_id)
        .fetch_one(&app_state.pool)
        .await?;
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
    let time_entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(entry_id)
        .fetch_one(&app_state.pool)
        .await?;
    if time_entry.user_id != requester.id {
        return Err(AppError::Forbidden);
    }
    if time_entry.status != "draft" {
        return Err(AppError::BadRequest("Only drafts can be deleted.".into()));
    }
    sqlx::query("DELETE FROM time_entries WHERE id=$1")
        .bind(entry_id)
        .execute(&app_state.pool)
        .await?;
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
    // Phase 1: validate ownership for ALL entries before any writes, so a
    // mixed-ownership batch never partially submits.
    for entry_id in &body.ids {
        let entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
            .bind(entry_id)
            .fetch_one(&app_state.pool)
            .await?;
        if entry.user_id != requester.id {
            return Err(AppError::Forbidden);
        }
    }
    // Phase 2: atomically submit all draft entries in a single transaction.
    let mut tx = app_state.pool.begin().await?;
    let mut submitted_ids: Vec<i64> = vec![];
    for entry_id in &body.ids {
        let affected_rows = sqlx::query(
            "UPDATE time_entries SET status='submitted', submitted_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND status='draft' AND user_id=$2",
        )
        .bind(entry_id)
        .bind(requester.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected_rows > 0 {
            submitted_ids.push(*entry_id);
        }
    }
    tx.commit().await?;
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
    // Phase 4: notify the approver with the actual submitted count.
    if submitted_count > 0 {
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
                    ("entry_count", i18n::entry_count(&language, submitted_count as i64)),
                ],
                Some("time_entries"),
                None,
            )
            .await;
        }
    }
    Ok(Json(serde_json::json!({"ok": true, "count": submitted_count})))
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(entry_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut tx = app_state.pool.begin().await?;
    let entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 FOR UPDATE")
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await?;
    if entry.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(entry.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    if entry.status != "submitted" {
        return Err(AppError::BadRequest(
            "Only submitted entries can be approved.".into(),
        ));
    }
    let rows_updated = sqlx::query(
        "UPDATE time_entries SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='submitted'",
    )
    .bind(requester.id)
    .bind(entry_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Entry was already reviewed by someone else.".into(),
        ));
    }
    tx.commit().await?;
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
    let mut tx = app_state.pool.begin().await?;
    let entry: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 FOR UPDATE")
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await?;
    if entry.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(entry.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    if entry.status != "submitted" {
        return Err(AppError::BadRequest(
            "Only submitted entries can be rejected.".into(),
        ));
    }
    let rows_updated = sqlx::query(
        "UPDATE time_entries SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3 AND status='submitted'",
    )
    .bind(requester.id)
    .bind(&body.reason)
    .bind(entry_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Entry was already reviewed by someone else.".into(),
        ));
    }
    tx.commit().await?;
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
    // Fetch all submitted entries that this lead is allowed to approve.
    let mut entries_to_approve: Vec<TimeEntry> = vec![];
    for entry_id in &body.ids {
        let entry: Option<TimeEntry> =
            sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 AND status='submitted'")
                .bind(entry_id)
                .fetch_optional(&app_state.pool)
                .await?;
        let Some(entry) = entry else { continue };
        if entry.user_id == requester.id && !requester.is_admin() {
            continue;
        }
        if !requester.is_admin() {
            let is_direct_report: Option<bool> = sqlx::query_scalar(
                "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin'",
            )
            .bind(entry.user_id)
            .bind(requester.id)
            .fetch_optional(&app_state.pool)
            .await?;
            if is_direct_report.is_none() {
                continue;
            }
        }
        entries_to_approve.push(entry);
    }
    if entries_to_approve.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    // Atomically approve all eligible entries.
    let mut tx = app_state.pool.begin().await?;
    let mut approved_entries: Vec<TimeEntry> = Vec::with_capacity(entries_to_approve.len());
    for entry in &entries_to_approve {
        let affected_rows = sqlx::query(
            "UPDATE time_entries SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='submitted'",
        )
        .bind(requester.id)
        .bind(entry.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected_rows > 0 {
            approved_entries.push(entry.clone());
        }
    }
    tx.commit().await?;
    let approved_count = approved_entries.len();
    // Audit + notify each affected employee (best-effort, after commit).
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
            Some(entry.id),
        )
        .await;
    }
    Ok(Json(serde_json::json!({"ok":true, "count": approved_count})))
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
    // Fetch all submitted entries that this lead is allowed to reject.
    let mut entries_to_reject: Vec<TimeEntry> = vec![];
    for entry_id in &body.ids {
        let entry: Option<TimeEntry> = sqlx::query_as(
            "SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, \
             status, submitted_at, reviewed_by, reviewed_at, rejection_reason, \
             created_at, updated_at FROM time_entries WHERE id=$1 AND status='submitted'",
        )
        .bind(entry_id)
        .fetch_optional(&app_state.pool)
        .await?;
        let Some(entry) = entry else { continue };
        if entry.user_id == requester.id && !requester.is_admin() {
            continue;
        }
        if !requester.is_admin() {
            let is_direct_report: Option<bool> = sqlx::query_scalar(
                "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin'",
            )
            .bind(entry.user_id)
            .bind(requester.id)
            .fetch_optional(&app_state.pool)
            .await?;
            if is_direct_report.is_none() {
                continue;
            }
        }
        entries_to_reject.push(entry);
    }
    if entries_to_reject.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    // Atomically reject all eligible entries.
    let mut tx = app_state.pool.begin().await?;
    let mut rejected_entries: Vec<TimeEntry> = Vec::with_capacity(entries_to_reject.len());
    for entry in &entries_to_reject {
        let affected_rows = sqlx::query(
            "UPDATE time_entries SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3 AND status='submitted'",
        )
        .bind(requester.id)
        .bind(&rejection_reason)
        .bind(entry.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected_rows > 0 {
            rejected_entries.push(entry.clone());
        }
    }
    tx.commit().await?;
    let rejected_count = rejected_entries.len();
    // Audit + notify each affected employee (best-effort, after commit).
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
    Ok(Json(serde_json::json!({"ok": true, "count": rejected_count})))
}
