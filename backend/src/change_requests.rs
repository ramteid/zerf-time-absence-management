use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::warn!(target:"zerf::change_requests", "load notification language failed: {e}");
            i18n::Language::default()
        }
    }
}

#[derive(FromRow, Serialize)]
pub struct ChangeRequest {
    pub id: i64,
    pub time_entry_id: i64,
    pub user_id: i64,
    pub new_date: Option<NaiveDate>,
    pub new_start_time: Option<String>,
    pub new_end_time: Option<String>,
    pub new_category_id: Option<i64>,
    pub new_comment: Option<String>,
    pub reason: String,
    pub status: String,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list(State(app_state): State<AppState>, requester: User) -> AppResult<Json<Vec<ChangeRequest>>> {
    Ok(Json(
        sqlx::query_as::<_, ChangeRequest>(
            "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE user_id=$1 ORDER BY created_at DESC",
        )
        .bind(requester.id)
        .fetch_all(&app_state.pool)
        .await?,
    ))
}

pub async fn list_all(State(app_state): State<AppState>, requester: User) -> AppResult<Json<Vec<ChangeRequest>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if requester.is_admin() {
        // Admins see all open change requests.
        return Ok(Json(
            sqlx::query_as::<_, ChangeRequest>(
                "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE status='open' ORDER BY created_at",
            )
            .fetch_all(&app_state.pool)
            .await?,
        ));
    }
    // Non-admin leads see only open change requests from their direct reports.
    Ok(Json(
        sqlx::query_as::<_, ChangeRequest>(
            "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE status='open' AND user_id IN (SELECT id FROM users WHERE approver_id = $1 AND role != 'admin') ORDER BY created_at",
        )
        .bind(requester.id)
        .fetch_all(&app_state.pool)
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct NewChangeRequest {
    pub time_entry_id: i64,
    pub new_date: Option<NaiveDate>,
    pub new_start_time: Option<String>,
    pub new_end_time: Option<String>,
    pub new_category_id: Option<i64>,
    pub new_comment: Option<String>,
    pub reason: String,
}

fn parse_change_time(time_str: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(time_str, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(time_str, "%H:%M:%S"))
        .map_err(|_| AppError::BadRequest("Invalid time format (HH:MM).".into()))
}

fn has_actual_change(
    current_date: NaiveDate,
    current_start: NaiveTime,
    current_end: NaiveTime,
    current_category_id: i64,
    current_comment: Option<&str>,
    new_date: Option<NaiveDate>,
    new_start: Option<NaiveTime>,
    new_end: Option<NaiveTime>,
    new_category_id: Option<i64>,
    new_comment: Option<&str>,
) -> bool {
    let current_comment = current_comment.filter(|v| !v.is_empty());
    let comment_changed = new_comment.is_some_and(|comment| {
        comment.is_empty().then_some(None).unwrap_or(Some(comment)) != current_comment
    });

    new_date.is_some_and(|date| date != current_date)
        || new_start.is_some_and(|start| start != current_start)
        || new_end.is_some_and(|end| end != current_end)
        || new_category_id.is_some_and(|category_id| category_id != current_category_id)
        || comment_changed
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewChangeRequest>,
) -> AppResult<Json<ChangeRequest>> {
    if body.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if body.reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    if let Some(comment) = &body.new_comment {
        if comment.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long.".into()));
        }
    }
    // Validate proposed time fields up-front so we never store a malformed
    // value that would later crash the reports / validation path. Times must
    // match HH:MM(:SS) and end > start when both are supplied. Future dates
    // are rejected — same rule as direct entry creation.
    let proposed_start = body
        .new_start_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    let proposed_end = body
        .new_end_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    if let (Some(start), Some(end)) = (proposed_start, proposed_end) {
        if end <= start {
            return Err(AppError::BadRequest(
                "End time must be after start time.".into(),
            ));
        }
    }
    if let Some(new_date) = body.new_date {
        if new_date > chrono::Local::now().date_naive() {
            return Err(AppError::BadRequest("Date cannot be in the future.".into()));
        }
        if new_date < requester.start_date {
            return Err(AppError::BadRequest(
                "Date cannot be before user start date.".into(),
            ));
        }
    }
    // Load the target time entry to check ownership and current state.
    let (entry_owner_id, entry_status, entry_date, entry_start_time, entry_end_time, entry_category_id, entry_comment): (i64, String, NaiveDate, String, String, i64, Option<String>) = sqlx::query_as(
        "SELECT user_id, status, entry_date, start_time, end_time, category_id, comment FROM time_entries WHERE id=$1",
    )
    .bind(body.time_entry_id)
    .fetch_one(&app_state.pool)
    .await?;
    if entry_owner_id != requester.id {
        return Err(AppError::Forbidden);
    }
    if entry_status == "draft" {
        return Err(AppError::BadRequest("Edit drafts directly.".into()));
    }
    if entry_status == "rejected" {
        return Err(AppError::BadRequest(
            "Rejected entries cannot have change requests. Use the reopen workflow to edit.".into(),
        ));
    }
    let current_start = parse_change_time(&entry_start_time)?;
    let current_end = parse_change_time(&entry_end_time)?;
    if !has_actual_change(
        entry_date,
        current_start,
        current_end,
        entry_category_id,
        entry_comment.as_deref(),
        body.new_date,
        proposed_start,
        proposed_end,
        body.new_category_id,
        body.new_comment.as_deref(),
    ) {
        return Err(AppError::BadRequest(
            "At least one actual change is required.".into(),
        ));
    }
    // When only one of start/end is proposed, validate the combination against
    // the existing entry's other time field to prevent storing impossible CRs.
    if proposed_start.is_some() || proposed_end.is_some() {
        let effective_start = proposed_start.unwrap_or(current_start);
        let effective_end = proposed_end.unwrap_or(current_end);
        if effective_end <= effective_start {
            return Err(AppError::BadRequest(
                "End time must be after start time.".into(),
            ));
        }
    }
    // Guard against duplicate open change requests for the same entry.
    let existing_open_cr_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM change_requests WHERE time_entry_id=$1 AND status='open'",
    )
    .bind(body.time_entry_id)
    .fetch_optional(&app_state.pool)
    .await?;
    if let Some(existing_id) = existing_open_cr_id {
        return Err(AppError::Conflict(format!(
            "An open change request already exists for this entry (id {existing_id})."
        )));
    }
    // Validate new_category_id if provided — reject nonexistent/inactive categories
    // before storing so malformed data never reaches the approval path.
    if let Some(category_id) = body.new_category_id {
        let category_active: Option<bool> =
            sqlx::query_scalar("SELECT active FROM categories WHERE id = $1")
                .bind(category_id)
                .fetch_optional(&app_state.pool)
                .await?;
        if category_active.is_none() {
            return Err(AppError::BadRequest("Category not found.".into()));
        }
        if category_active == Some(false) {
            return Err(AppError::BadRequest("Category is inactive.".into()));
        }
    }
    let new_change_request_id: i64 = sqlx::query_scalar("INSERT INTO change_requests(time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id")
        .bind(body.time_entry_id).bind(requester.id).bind(body.new_date).bind(&body.new_start_time).bind(&body.new_end_time).bind(body.new_category_id).bind(&body.new_comment).bind(&body.reason)
        .fetch_one(&app_state.pool).await?;
    let created_change_request: ChangeRequest = sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1")
        .bind(new_change_request_id)
        .fetch_one(&app_state.pool)
        .await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "change_requests",
        new_change_request_id,
        None,
        Some(serde_json::to_value(&created_change_request).unwrap()),
    )
    .await;
    // Notify approvers that a change request needs review.
    let requester_full_name = format!("{} {}", requester.first_name, requester.last_name);
    let requested_entry_date = created_change_request.new_date.unwrap_or(entry_date);
    let approver_ids = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
    let language = notification_language(&app_state.pool).await;
    for approver_id in approver_ids {
        crate::notifications::create_translated(
            &app_state,
            &language,
            approver_id,
            "change_request_created",
            "change_request_created_title",
            "change_request_created_body",
            vec![
                ("requester_name", requester_full_name.clone()),
                (
                    "entry_date",
                    i18n::format_date(&language, requested_entry_date),
                ),
            ],
            Some("change_requests"),
            Some(new_change_request_id),
        )
        .await;
    }
    Ok(Json(created_change_request))
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(change_request_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut tx = app_state.pool.begin().await?;
    // Fetch and lock the change request — fail fast if already resolved.
    let change_request: ChangeRequest =
        sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open' FOR UPDATE")
            .bind(change_request_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::Conflict("Change request was already resolved by someone else.".into()))?;
    // A lead may not review their own request; admins may.
    if change_request.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() {
        // Non-admin leads may only act on requests from their direct reports.
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(change_request.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Acquire a per-user advisory lock to serialize updates to this user's entries.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(change_request.user_id)
        .execute(&mut *tx)
        .await?;
    // Fetch the existing entry and build effective post-change values so we can
    // run the same overlap / 14-hour / category validation as direct edits do.
    let existing_entry: crate::time_entries::TimeEntry =
        sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 FOR UPDATE")
            .bind(change_request.time_entry_id)
            .fetch_one(&mut *tx)
            .await?;
    if existing_entry.user_id != change_request.user_id {
        return Err(AppError::Conflict(
            "Change request target no longer matches the entry owner.".into(),
        ));
    }
    if existing_entry.status == "draft" {
        return Err(AppError::BadRequest("Edit drafts directly.".into()));
    }
    if existing_entry.status == "rejected" {
        return Err(AppError::BadRequest(
            "Rejected entries cannot have change requests. Use the reopen workflow to edit.".into(),
        ));
    }
    let current_start = parse_change_time(&existing_entry.start_time)?;
    let current_end = parse_change_time(&existing_entry.end_time)?;
    let proposed_start = change_request
        .new_start_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    let proposed_end = change_request
        .new_end_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    if !has_actual_change(
        existing_entry.entry_date,
        current_start,
        current_end,
        existing_entry.category_id,
        existing_entry.comment.as_deref(),
        change_request.new_date,
        proposed_start,
        proposed_end,
        change_request.new_category_id,
        change_request.new_comment.as_deref(),
    ) {
        return Err(AppError::BadRequest(
            "At least one actual change is required.".into(),
        ));
    }
    // Build the effective entry state after applying the change request.
    let effective_entry = crate::time_entries::NewTimeEntry {
        entry_date: change_request.new_date.unwrap_or(existing_entry.entry_date),
        start_time: change_request
            .new_start_time
            .clone()
            .unwrap_or_else(|| existing_entry.start_time.clone()),
        end_time: change_request
            .new_end_time
            .clone()
            .unwrap_or_else(|| existing_entry.end_time.clone()),
        category_id: change_request.new_category_id.unwrap_or(existing_entry.category_id),
        comment: change_request.new_comment.clone().or(existing_entry.comment.clone()),
    };
    crate::time_entries::validate(&mut *tx, existing_entry.user_id, &effective_entry, Some(change_request.time_entry_id))
        .await?;
    // Use optimistic locking: only proceed if status is still 'open'.
    let rows_claimed = sqlx::query(
        "UPDATE change_requests SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='open'",
    )
    .bind(requester.id)
    .bind(change_request_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows_claimed == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
    let rows_entry_updated = sqlx::query("UPDATE time_entries SET entry_date=COALESCE($1,entry_date), start_time=COALESCE($2,start_time), end_time=COALESCE($3,end_time), category_id=COALESCE($4,category_id), comment=CASE WHEN $5 IS NOT NULL THEN NULLIF($5,'') ELSE comment END, updated_at=CURRENT_TIMESTAMP WHERE id=$6 AND status=$7")
        .bind(change_request.new_date).bind(&change_request.new_start_time).bind(&change_request.new_end_time).bind(change_request.new_category_id).bind(&change_request.new_comment).bind(change_request.time_entry_id).bind(&existing_entry.status)
        .execute(&mut *tx).await?
        .rows_affected();
    if rows_entry_updated == 0 {
        return Err(AppError::Conflict(
            "Change request could no longer be applied because the entry changed.".into(),
        ));
    }
    tx.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "change_requests",
        change_request_id,
        Some(serde_json::to_value(&change_request).unwrap()),
        Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
    )
    .await;
    // Notify the requester that their change request was approved.
    let language = notification_language(&app_state.pool).await;
    let affected_entry_date = change_request.new_date.unwrap_or(existing_entry.entry_date);
    crate::notifications::create_translated(
        &app_state,
        &language,
        change_request.user_id,
        "change_request_approved",
        "change_request_approved_title",
        "change_request_approved_body",
        vec![("entry_date", i18n::format_date(&language, affected_entry_date))],
        Some("change_requests"),
        Some(change_request_id),
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
    Path(change_request_id): Path<i64>,
    Json(body): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if body.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if body.reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    let mut tx = app_state.pool.begin().await?;
    // Lock the change request row to prevent concurrent rejections.
    let change_request: ChangeRequest = sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open'")
        .bind(change_request_id)
        .fetch_one(&mut *tx)
        .await?;
    // A lead may not reject their own request; admins may.
    if change_request.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on requests from their direct reports.
    if !requester.is_admin() {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(change_request.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Use optimistic locking: only proceed if status is still 'open'.
    let rows_updated = sqlx::query(
        "UPDATE change_requests SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3 AND status='open'",
    )
    .bind(requester.id)
    .bind(&body.reason)
    .bind(change_request_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
    tx.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "change_requests",
        change_request_id,
        Some(serde_json::to_value(&change_request).unwrap()),
        Some(serde_json::json!({"status": "rejected", "reason": body.reason})),
    )
    .await;
    // Notify the requester that their change request was rejected.
    let language = notification_language(&app_state.pool).await;
    let affected_entry_date: NaiveDate =
        sqlx::query_scalar("SELECT entry_date FROM time_entries WHERE id=$1")
            .bind(change_request.time_entry_id)
            .fetch_one(&app_state.pool)
            .await
            .unwrap_or(change_request.new_date.unwrap_or(chrono::Local::now().date_naive()));
    crate::notifications::create_translated(
        &app_state,
        &language,
        change_request.user_id,
        "change_request_rejected",
        "change_request_rejected_title",
        "change_request_rejected_body",
        vec![
            ("entry_date", i18n::format_date(&language, affected_entry_date)),
            ("reason", body.reason.clone()),
        ],
        Some("change_requests"),
        Some(change_request_id),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}
