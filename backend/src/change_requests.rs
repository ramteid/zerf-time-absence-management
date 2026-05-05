use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n::{self, TextKey};
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

pub async fn list(State(s): State<AppState>, u: User) -> AppResult<Json<Vec<ChangeRequest>>> {
    Ok(Json(
        sqlx::query_as::<_, ChangeRequest>(
            "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE user_id=$1 ORDER BY created_at DESC",
        )
        .bind(u.id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

pub async fn list_all(State(s): State<AppState>, u: User) -> AppResult<Json<Vec<ChangeRequest>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    if u.is_admin() {
        return Ok(Json(
            sqlx::query_as::<_, ChangeRequest>(
                "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE status='open' ORDER BY created_at",
            )
            .fetch_all(&s.pool)
            .await?,
        ));
    }
    // Non-admin leads see only open change requests from their direct reports.
    Ok(Json(
        sqlx::query_as::<_, ChangeRequest>(
            "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE status='open' AND user_id IN (SELECT id FROM users WHERE approver_id = $1 AND role != 'admin') ORDER BY created_at",
        )
        .bind(u.id)
        .fetch_all(&s.pool)
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

fn parse_change_time(s: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
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
    let current_comment = current_comment.filter(|value| !value.is_empty());
    let comment_changed = new_comment.is_some_and(|comment| {
        let normalized = if comment.is_empty() {
            None
        } else {
            Some(comment)
        };
        normalized != current_comment
    });

    new_date.is_some_and(|date| date != current_date)
        || new_start.is_some_and(|start| start != current_start)
        || new_end.is_some_and(|end| end != current_end)
        || new_category_id.is_some_and(|category_id| category_id != current_category_id)
        || comment_changed
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewChangeRequest>,
) -> AppResult<Json<ChangeRequest>> {
    if b.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if b.reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    if let Some(c) = &b.new_comment {
        if c.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long.".into()));
        }
    }
    // Validate proposed time fields up-front so we never store a malformed
    // value that would later crash the reports / validation path. Times must
    // match HH:MM(:SS) and end > start when both are supplied. Future dates
    // are rejected — same rule as direct entry creation.
    let new_start = b
        .new_start_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    let new_end = b
        .new_end_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    if let (Some(s2), Some(e2)) = (new_start, new_end) {
        if e2 <= s2 {
            return Err(AppError::BadRequest(
                "End time must be after start time.".into(),
            ));
        }
    }
    if let Some(d) = b.new_date {
        if d > chrono::Local::now().date_naive() {
            return Err(AppError::BadRequest("Date cannot be in the future.".into()));
        }
        if d < u.start_date {
            return Err(AppError::BadRequest(
                "Date cannot be before user start date.".into(),
            ));
        }
    }
    let z: (i64, String, NaiveDate, String, String, i64, Option<String>) = sqlx::query_as(
        "SELECT user_id, status, entry_date, start_time, end_time, category_id, comment FROM time_entries WHERE id=$1",
    )
    .bind(b.time_entry_id)
    .fetch_one(&s.pool)
    .await?;
    if z.0 != u.id {
        return Err(AppError::Forbidden);
    }
    if z.1 == "draft" {
        return Err(AppError::BadRequest("Edit drafts directly.".into()));
    }
    if z.1 == "rejected" {
        return Err(AppError::BadRequest(
            "Rejected entries cannot have change requests. Use the reopen workflow to edit.".into(),
        ));
    }
    let current_start = parse_change_time(&z.3)?;
    let current_end = parse_change_time(&z.4)?;
    if !has_actual_change(
        z.2,
        current_start,
        current_end,
        z.5,
        z.6.as_deref(),
        b.new_date,
        new_start,
        new_end,
        b.new_category_id,
        b.new_comment.as_deref(),
    ) {
        return Err(AppError::BadRequest(
            "At least one actual change is required.".into(),
        ));
    }
    // When only one of start/end is proposed, validate the combination against
    // the existing entry's other time field to prevent storing impossible CRs.
    if new_start.is_some() || new_end.is_some() {
        let eff_start = new_start.unwrap_or(current_start);
        let eff_end = new_end.unwrap_or(current_end);
        if eff_end <= eff_start {
            return Err(AppError::BadRequest(
                "End time must be after start time.".into(),
            ));
        }
    }
    // Guard against duplicate open change requests for the same entry.
    let open_cr: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM change_requests WHERE time_entry_id=$1 AND status='open'",
    )
    .bind(b.time_entry_id)
    .fetch_optional(&s.pool)
    .await?;
    if let Some(existing_id) = open_cr {
        return Err(AppError::Conflict(format!(
            "An open change request already exists for this entry (id {existing_id})."
        )));
    }
    // Validate new_category_id if provided — reject nonexistent/inactive categories
    // before storing so malformed data never reaches the approval path.
    if let Some(cat_id) = b.new_category_id {
        let cat_active: Option<bool> =
            sqlx::query_scalar("SELECT active FROM categories WHERE id = $1")
                .bind(cat_id)
                .fetch_optional(&s.pool)
                .await?;
        match cat_active {
            None => return Err(AppError::BadRequest("Category not found.".into())),
            Some(false) => return Err(AppError::BadRequest("Category is inactive.".into())),
            Some(true) => {}
        }
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO change_requests(time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id")
        .bind(b.time_entry_id).bind(u.id).bind(b.new_date).bind(&b.new_start_time).bind(&b.new_end_time).bind(b.new_category_id).bind(&b.new_comment).bind(&b.reason)
        .fetch_one(&s.pool).await?;
    let a: ChangeRequest = sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "created",
        "change_requests",
        id,
        None,
        Some(serde_json::to_value(&a).unwrap()),
    )
    .await;
    let requester_name = format!("{} {}", u.first_name, u.last_name);
    let requested_entry_date = a.new_date.unwrap_or(z.2);
    let recipients = crate::auth::approval_recipient_ids(&s.pool, &u).await;
    let language = notification_language(&s.pool).await;
    for recipient_id in recipients {
        crate::notifications::create_translated(
            &s,
            language,
            recipient_id,
            "change_request_created",
            TextKey::ChangeRequestCreatedTitle,
            TextKey::ChangeRequestCreatedBody,
            vec![
                ("requester_name", requester_name.clone()),
                (
                    "entry_date",
                    i18n::format_date(language, requested_entry_date),
                ),
            ],
            Some("change_requests"),
            Some(id),
        )
        .await;
    }
    Ok(Json(a))
}

pub async fn approve(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut tx = s.pool.begin().await?;
    // Fetch the change request first, then lock and validate.
    let a: ChangeRequest =
        sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open' FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::Conflict("Change request was already resolved by someone else.".into()))?;
    // A lead may not review their own request; admins may.
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !u.is_admin() {
        let is_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(a.user_id)
        .bind(u.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Fetch the existing entry and build effective post-change values so we can
    // run the same overlap / 14-hour / category validation as direct edits do.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    let entry: crate::time_entries::TimeEntry =
        sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 FOR UPDATE")
            .bind(a.time_entry_id)
            .fetch_one(&mut *tx)
            .await?;
    if entry.user_id != a.user_id {
        return Err(AppError::Conflict(
            "Change request target no longer matches the entry owner.".into(),
        ));
    }
    if entry.status == "draft" {
        return Err(AppError::BadRequest("Edit drafts directly.".into()));
    }
    if entry.status == "rejected" {
        return Err(AppError::BadRequest(
            "Rejected entries cannot have change requests. Use the reopen workflow to edit.".into(),
        ));
    }
    let current_start = parse_change_time(&entry.start_time)?;
    let current_end = parse_change_time(&entry.end_time)?;
    let new_start = a
        .new_start_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    let new_end = a
        .new_end_time
        .as_deref()
        .map(parse_change_time)
        .transpose()?;
    if !has_actual_change(
        entry.entry_date,
        current_start,
        current_end,
        entry.category_id,
        entry.comment.as_deref(),
        a.new_date,
        new_start,
        new_end,
        a.new_category_id,
        a.new_comment.as_deref(),
    ) {
        return Err(AppError::BadRequest(
            "At least one actual change is required.".into(),
        ));
    }
    let effective = crate::time_entries::NewTimeEntry {
        entry_date: a.new_date.unwrap_or(entry.entry_date),
        start_time: a
            .new_start_time
            .clone()
            .unwrap_or_else(|| entry.start_time.clone()),
        end_time: a
            .new_end_time
            .clone()
            .unwrap_or_else(|| entry.end_time.clone()),
        category_id: a.new_category_id.unwrap_or(entry.category_id),
        comment: a.new_comment.clone().or(entry.comment.clone()),
    };
    crate::time_entries::validate(&mut *tx, entry.user_id, &effective, Some(a.time_entry_id))
        .await?;
    let claimed = sqlx::query(
        "UPDATE change_requests SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='open'",
    )
    .bind(u.id)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if claimed == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
    let updated_entry = sqlx::query("UPDATE time_entries SET entry_date=COALESCE($1,entry_date), start_time=COALESCE($2,start_time), end_time=COALESCE($3,end_time), category_id=COALESCE($4,category_id), comment=CASE WHEN $5 IS NOT NULL THEN NULLIF($5,'') ELSE comment END, updated_at=CURRENT_TIMESTAMP WHERE id=$6 AND status=$7")
        .bind(a.new_date).bind(&a.new_start_time).bind(&a.new_end_time).bind(a.new_category_id).bind(&a.new_comment).bind(a.time_entry_id).bind(&entry.status)
        .execute(&mut *tx).await?
        .rows_affected();
    if updated_entry == 0 {
        return Err(AppError::Conflict(
            "Change request could no longer be applied because the entry changed.".into(),
        ));
    }
    tx.commit().await?;
    audit::log(
        &s.pool,
        u.id,
        "approved",
        "change_requests",
        id,
        Some(serde_json::to_value(&a).unwrap()),
        Some(serde_json::json!({"status": "approved", "reviewed_by": u.id})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

pub async fn reject(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    if b.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if b.reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    let mut tx = s.pool.begin().await?;
    let prev: ChangeRequest = sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open'")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    // A lead may not reject their own request; admins may.
    if prev.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on requests from their direct reports.
    if !u.is_admin() {
        let is_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(prev.user_id)
        .bind(u.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    let updated = sqlx::query(
        "UPDATE change_requests SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3 AND status='open'",
    )
    .bind(u.id)
    .bind(&b.reason)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
    tx.commit().await?;
    audit::log(
        &s.pool,
        u.id,
        "rejected",
        "change_requests",
        id,
        Some(serde_json::to_value(&prev).unwrap()),
        Some(serde_json::json!({"status": "rejected", "reason": b.reason})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}
