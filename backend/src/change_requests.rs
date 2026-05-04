use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
            "SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE status='open' AND user_id IN (SELECT id FROM users WHERE approver_id = $1) ORDER BY created_at",
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
    let parse_t = |s: &str| -> AppResult<chrono::NaiveTime> {
        chrono::NaiveTime::parse_from_str(s, "%H:%M")
            .or_else(|_| chrono::NaiveTime::parse_from_str(s, "%H:%M:%S"))
            .map_err(|_| AppError::BadRequest("Invalid time format (HH:MM).".into()))
    };
    let new_start = b.new_start_time.as_deref().map(parse_t).transpose()?;
    let new_end = b.new_end_time.as_deref().map(parse_t).transpose()?;
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
    }
    let z: (i64, String, String, String) = sqlx::query_as(
        "SELECT user_id, status, start_time, end_time FROM time_entries WHERE id=$1",
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
    // When only one of start/end is proposed, validate the combination against
    // the existing entry's other time field to prevent storing impossible CRs.
    if new_start.is_some() || new_end.is_some() {
        let eff_start = new_start
            .or_else(|| parse_t(&z.2).ok())
            .unwrap_or(chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let eff_end = new_end
            .or_else(|| parse_t(&z.3).ok())
            .unwrap_or(chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
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
    let a: ChangeRequest =
        sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open'")
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    // A lead may not review their own request; admins may.
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on requests from their direct reports.
    if !u.is_admin() {
        let is_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2",
        )
        .bind(a.user_id)
        .bind(u.id)
        .fetch_optional(&s.pool)
        .await?;
        if is_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Fetch the existing entry and build effective post-change values so we can
    // run the same overlap / 14-hour / category validation as direct edits do.
    let entry: crate::time_entries::TimeEntry =
        sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
            .bind(a.time_entry_id)
            .fetch_one(&s.pool)
            .await?;
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
    crate::time_entries::validate(&s.pool, entry.user_id, &effective, Some(a.time_entry_id))
        .await?;
    let mut tx = s.pool.begin().await?;
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
    let prev: ChangeRequest = sqlx::query_as("SELECT id, time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM change_requests WHERE id=$1 AND status='open'")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    // A lead may not reject their own request; admins may.
    if prev.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on requests from their direct reports.
    if !u.is_admin() {
        let is_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2",
        )
        .bind(prev.user_id)
        .bind(u.id)
        .fetch_optional(&s.pool)
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
    .execute(&s.pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::Conflict(
            "Change request was already resolved by someone else.".into(),
        ));
    }
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
