use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::time_calc;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    crate::notifications::load_language(pool).await
}

const ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL: &str = "\
    SELECT TRUE \
    FROM user_approvers ua \
    JOIN users subject ON subject.id = ua.user_id \
    JOIN users approver ON approver.id = ua.approver_id \
    WHERE ua.user_id = $1 AND ua.approver_id = $2 \
    AND subject.active=TRUE AND subject.role != 'admin' \
    AND approver.active=TRUE AND approver.role IN ('team_lead','admin') \
    FOR UPDATE OF ua";

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

fn repo_cr_to_service(cr: crate::repository::ChangeRequest) -> ChangeRequest {
    ChangeRequest {
        id: cr.id,
        time_entry_id: cr.time_entry_id,
        user_id: cr.user_id,
        new_date: cr.new_date,
        new_start_time: cr.new_start_time,
        new_end_time: cr.new_end_time,
        new_category_id: cr.new_category_id,
        new_comment: cr.new_comment,
        reason: cr.reason,
        status: cr.status,
        reviewed_by: cr.reviewed_by,
        reviewed_at: cr.reviewed_at,
        rejection_reason: cr.rejection_reason,
        created_at: cr.created_at,
    }
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ChangeRequest>>> {
    let crs = app_state
        .db
        .change_requests
        .list_for_user(requester.id)
        .await?;
    Ok(Json(crs.into_iter().map(repo_cr_to_service).collect()))
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ChangeRequest>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let crs = if requester.is_admin() {
        app_state.db.change_requests.list_open_all().await?
    } else {
        app_state
            .db
            .change_requests
            .list_open_for_lead(requester.id)
            .await?
    };
    Ok(Json(crs.into_iter().map(repo_cr_to_service).collect()))
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
    time_calc::parse_hhmm_or_hhmmss(time_str)
        .ok_or_else(|| AppError::BadRequest("Invalid time format (HH:MM).".into()))
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn hhmm(time_str: &str) -> String {
    time_str.chars().take(5).collect()
}

fn empty_text(language: &i18n::Language) -> String {
    i18n::translate(language, "text_empty", &[])
}

async fn category_label(
    pool: &crate::db::DatabasePool,
    language: &i18n::Language,
    category_id: i64,
) -> String {
    let raw_name: Option<String> = sqlx::query_scalar("SELECT name FROM categories WHERE id=$1")
        .bind(category_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    match raw_name {
        Some(name) => i18n::work_category_label(language, &name),
        None => format!("#{category_id}"),
    }
}

fn entry_label(
    language: &i18n::Language,
    entry_date: NaiveDate,
    start_time: &str,
    end_time: &str,
    category: &str,
) -> String {
    format!(
        "{} · {}-{} · {}",
        i18n::format_date(language, entry_date),
        hhmm(start_time),
        hhmm(end_time),
        category
    )
}

#[allow(clippy::too_many_arguments)]
fn change_diff(
    language: &i18n::Language,
    current_date: NaiveDate,
    current_start: &str,
    current_end: &str,
    current_category: &str,
    current_comment: Option<&str>,
    new_date: Option<NaiveDate>,
    new_start: Option<&str>,
    new_end: Option<&str>,
    new_category: Option<&str>,
    new_comment: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    let date_label = i18n::translate(language, "change_diff_label_date", &[]);
    let time_label = i18n::translate(language, "change_diff_label_time", &[]);
    let type_label = i18n::translate(language, "change_diff_label_type", &[]);
    let comment_label = i18n::translate(language, "change_diff_label_comment", &[]);

    if let Some(next_date) = new_date {
        if next_date != current_date {
            lines.push(format!(
                "- {date_label}: {} -> {}",
                i18n::format_date(language, current_date),
                i18n::format_date(language, next_date)
            ));
        }
    }

    let next_start = new_start.map(hhmm).unwrap_or_else(|| hhmm(current_start));
    let next_end = new_end.map(hhmm).unwrap_or_else(|| hhmm(current_end));
    let current_start_hhmm = hhmm(current_start);
    let current_end_hhmm = hhmm(current_end);
    if next_start != current_start_hhmm || next_end != current_end_hhmm {
        lines.push(format!(
            "- {time_label}: {}-{} -> {}-{}",
            current_start_hhmm, current_end_hhmm, next_start, next_end
        ));
    }

    let next_category = new_category.unwrap_or(current_category);
    if next_category != current_category {
        lines.push(format!(
            "- {type_label}: {current_category} -> {next_category}"
        ));
    }

    if let Some(comment) = new_comment {
        let before = current_comment.filter(|value| !value.is_empty());
        let after = if comment.is_empty() {
            None
        } else {
            Some(comment)
        };
        if before != after {
            lines.push(format!(
                "- {comment_label}: {} -> {}",
                before
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| empty_text(language)),
                after
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| empty_text(language))
            ));
        }
    }

    if lines.is_empty() {
        i18n::translate(language, "change_diff_no_effective_change", &[])
    } else {
        lines.join("\n")
    }
}

#[allow(clippy::too_many_arguments)]
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
        (if comment.is_empty() {
            None
        } else {
            Some(comment)
        }) != current_comment
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
    if let Some(new_date) = body.new_date {
        let today = crate::settings::app_today(&app_state.pool).await;
        if new_date > today {
            return Err(AppError::BadRequest("Date cannot be in the future.".into()));
        }
        if new_date < requester.start_date {
            return Err(AppError::BadRequest(
                "Date cannot be before user start date.".into(),
            ));
        }
    }
    // Load the target time entry to check ownership and current state.
    let (
        entry_owner_id,
        entry_status,
        entry_date,
        entry_start_time,
        entry_end_time,
        entry_category_id,
        entry_comment,
    ) = app_state
        .db
        .change_requests
        .get_entry_info(body.time_entry_id)
        .await?
        .ok_or(AppError::NotFound)?;
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
    // Validate new_category_id if provided — reject nonexistent/inactive categories
    // before storing so malformed data never reaches the approval path.
    if let Some(category_id) = body.new_category_id {
        let category_active = app_state
            .db
            .change_requests
            .check_category_active(category_id)
            .await?;
        if category_active.is_none() {
            return Err(AppError::BadRequest("Category not found.".into()));
        }
        if category_active == Some(false) {
            return Err(AppError::BadRequest("Category is inactive.".into()));
        }
    }
    let created_change_request = repo_cr_to_service(
        app_state
            .db
            .change_requests
            .create(
                body.time_entry_id,
                requester.id,
                body.new_date,
                body.new_start_time.as_deref(),
                body.new_end_time.as_deref(),
                body.new_category_id,
                body.new_comment.as_deref(),
                &body.reason,
            )
            .await?,
    );
    let new_change_request_id = created_change_request.id;
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "change_requests",
        new_change_request_id,
        None,
        serde_json::to_value(&created_change_request).ok(),
    )
    .await;
    // Notify approvers that a change request needs review.
    let requester_full_name = requester.full_name();
    let approver_ids =
        crate::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, monday_of(entry_date));
    let current_category = category_label(&app_state.pool, &language, entry_category_id).await;
    let proposed_category = match body.new_category_id {
        Some(category_id) => Some(category_label(&app_state.pool, &language, category_id).await),
        None => None,
    };
    let entry_label_text = entry_label(
        &language,
        entry_date,
        &entry_start_time,
        &entry_end_time,
        &current_category,
    );
    let diff_text = change_diff(
        &language,
        entry_date,
        &entry_start_time,
        &entry_end_time,
        &current_category,
        entry_comment.as_deref(),
        body.new_date,
        body.new_start_time.as_deref(),
        body.new_end_time.as_deref(),
        proposed_category.as_deref(),
        body.new_comment.as_deref(),
    );
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
                ("week_label", week_label.clone()),
                ("entry_label", entry_label_text.clone()),
                ("reason", body.reason.trim().to_string()),
                ("change_diff", diff_text.clone()),
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
    let mut transaction = app_state.pool.begin().await?;
    let change_request_user_id: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM change_requests WHERE id=$1")
            .bind(change_request_id)
            .fetch_optional(&mut *transaction)
            .await?;
    let Some(change_request_user_id) = change_request_user_id else {
        return Err(AppError::NotFound);
    };
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(change_request_user_id)
        .execute(&mut *transaction)
        .await?;
    // Fetch and lock the change request — fail fast if already resolved.
    let change_request: ChangeRequest =
        sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open' FOR UPDATE")
            .bind(change_request_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| AppError::Conflict("Change request was already resolved by someone else.".into()))?;
    // No user may review their own request; admins may.
    if change_request.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() {
        // Non-admin approvers may only act if explicitly assigned.
        let is_assigned_approver: Option<bool> =
            sqlx::query_scalar(ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL)
                .bind(change_request.user_id)
                .bind(requester.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if is_assigned_approver.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Fetch the existing entry and build effective post-change values so we can
    // run the same overlap / 14-hour / category validation as direct edits do.
    let existing_entry: crate::time_entries::TimeEntry =
        sqlx::query_as("SELECT te.id, te.user_id, te.entry_date, te.start_time, te.end_time, te.category_id, c.counts_as_work, te.comment, te.status, te.submitted_at, te.reviewed_by, te.reviewed_at, te.rejection_reason, te.created_at, te.updated_at FROM time_entries te JOIN categories c ON c.id = te.category_id WHERE te.id=$1 FOR UPDATE")
            .bind(change_request.time_entry_id)
            .fetch_one(&mut *transaction)
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
        category_id: change_request
            .new_category_id
            .unwrap_or(existing_entry.category_id),
        comment: match change_request.new_comment.as_deref() {
            Some("") => None,
            Some(comment) => Some(comment.to_string()),
            None => existing_entry.comment.clone(),
        },
    };
    // Validate the resulting state on the effective (possibly changed) entry date.
    // The source day can only lose minutes when moving/changing an entry, so it
    // cannot newly violate overlap/day-total constraints.
    crate::time_entries::validate(
        &mut transaction,
        existing_entry.user_id,
        &effective_entry,
        Some(change_request.time_entry_id),
    )
    .await?;
    // Use optimistic locking: only proceed if status is still 'open'.
    let rows_claimed = sqlx::query(
        "UPDATE change_requests SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='open'",
    )
    .bind(requester.id)
    .bind(change_request_id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if rows_claimed == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
    let rows_entry_updated = sqlx::query("UPDATE time_entries SET entry_date=COALESCE($1,entry_date), start_time=COALESCE($2,start_time), end_time=COALESCE($3,end_time), category_id=COALESCE($4,category_id), comment=CASE WHEN $5 IS NOT NULL THEN NULLIF($5,'') ELSE comment END, updated_at=CURRENT_TIMESTAMP WHERE id=$6 AND status=$7")
        .bind(change_request.new_date).bind(&change_request.new_start_time).bind(&change_request.new_end_time).bind(change_request.new_category_id).bind(&change_request.new_comment).bind(change_request.time_entry_id).bind(&existing_entry.status)
        .execute(&mut *transaction).await?
        .rows_affected();
    if rows_entry_updated == 0 {
        return Err(AppError::Conflict(
            "Change request could no longer be applied because the entry changed.".into(),
        ));
    }
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "change_requests",
        change_request_id,
        serde_json::to_value(&change_request).ok(),
        Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
    )
    .await;
    // Notify the requester (including admins acting on their own requests).
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, monday_of(change_request.new_date.unwrap_or(existing_entry.entry_date)));
    let current_category =
        category_label(&app_state.pool, &language, existing_entry.category_id).await;
    let effective_category = match change_request.new_category_id {
        Some(category_id) => category_label(&app_state.pool, &language, category_id).await,
        None => current_category.clone(),
    };
    let entry_label_text = entry_label(
        &language,
        change_request.new_date.unwrap_or(existing_entry.entry_date),
        change_request
            .new_start_time
            .as_deref()
            .unwrap_or(&existing_entry.start_time),
        change_request
            .new_end_time
            .as_deref()
            .unwrap_or(&existing_entry.end_time),
        &effective_category,
    );
    let diff_text = change_diff(
        &language,
        existing_entry.entry_date,
        &existing_entry.start_time,
        &existing_entry.end_time,
        &current_category,
        existing_entry.comment.as_deref(),
        change_request.new_date,
        change_request.new_start_time.as_deref(),
        change_request.new_end_time.as_deref(),
        Some(&effective_category),
        change_request.new_comment.as_deref(),
    );
    // Skip email when an admin approved their own change request.
    let params = vec![
        ("week_label", week_label),
        ("entry_label", entry_label_text),
        ("change_diff", diff_text),
    ];
    if change_request.user_id == requester.id {
        crate::notifications::create_translated_inapp_only(
            &app_state, &language, change_request.user_id,
            "change_request_approved", "change_request_approved_title",
            "change_request_approved_body", params,
            Some("change_requests"), Some(change_request_id),
        ).await;
    } else {
        crate::notifications::create_translated(
            &app_state, &language, change_request.user_id,
            "change_request_approved", "change_request_approved_title",
            "change_request_approved_body", params,
            Some("change_requests"), Some(change_request_id),
        ).await;
    }
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
    let mut transaction = app_state.pool.begin().await?;
    // Lock the open change request row and fail fast if it was already resolved.
    let change_request: ChangeRequest = sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open' FOR UPDATE")
        .bind(change_request_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::Conflict("Change request was already resolved by someone else.".into()))?;
    // No user may reject their own request; admins may.
    if change_request.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin approvers may only act if explicitly assigned.
    if !requester.is_admin() {
        let is_assigned_approver: Option<bool> =
            sqlx::query_scalar(ACTIVE_ASSIGNED_APPROVER_FOR_UPDATE_SQL)
                .bind(change_request.user_id)
                .bind(requester.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if is_assigned_approver.is_none() {
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
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "change_requests",
        change_request_id,
        serde_json::to_value(&change_request).ok(),
        Some(serde_json::json!({"status": "rejected", "reason": body.reason})),
    )
    .await;
    // Notify the requester (including admins acting on their own requests).
    let language = notification_language(&app_state.pool).await;
    if let Some((
        _entry_owner_id,
        _entry_status,
        entry_date,
        entry_start_time,
        entry_end_time,
        entry_category_id,
        entry_comment,
    )) = app_state
        .db
        .change_requests
        .get_entry_info(change_request.time_entry_id)
        .await?
    {
        let week_label = i18n::format_week_label(&language, monday_of(entry_date));
        let current_category = category_label(&app_state.pool, &language, entry_category_id).await;
        let requested_category = match change_request.new_category_id {
            Some(category_id) => category_label(&app_state.pool, &language, category_id).await,
            None => current_category.clone(),
        };
        let entry_label_text = entry_label(
            &language,
            entry_date,
            &entry_start_time,
            &entry_end_time,
            &current_category,
        );
        let diff_text = change_diff(
            &language,
            entry_date,
            &entry_start_time,
            &entry_end_time,
            &current_category,
            entry_comment.as_deref(),
            change_request.new_date,
            change_request.new_start_time.as_deref(),
            change_request.new_end_time.as_deref(),
            Some(&requested_category),
            change_request.new_comment.as_deref(),
        );
        // Skip email when an admin rejected their own change request.
        let params = vec![
            ("week_label", week_label),
            ("entry_label", entry_label_text),
            ("reason", body.reason.clone()),
            ("change_diff", diff_text),
        ];
        if change_request.user_id == requester.id {
            crate::notifications::create_translated_inapp_only(
                &app_state, &language, change_request.user_id,
                "change_request_rejected", "change_request_rejected_title",
                "change_request_rejected_body", params,
                Some("change_requests"), Some(change_request_id),
            ).await;
        } else {
            crate::notifications::create_translated(
                &app_state, &language, change_request.user_id,
                "change_request_rejected", "change_request_rejected_title",
                "change_request_rejected_body", params,
                Some("change_requests"), Some(change_request_id),
            ).await;
        }
    } else {
        tracing::warn!(
            target: "zerf::change_requests",
            "entry {} no longer exists after rejecting change request {}; skipping notification",
            change_request.time_entry_id, change_request_id
        );
    }
    Ok(Json(serde_json::json!({"ok":true})))
}
