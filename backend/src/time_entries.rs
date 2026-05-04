use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};

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

fn parse_time(s: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .map_err(|_| AppError::BadRequest(format!("Invalid time: {s}")))
}

fn duration_min(start: &str, end: &str) -> AppResult<i64> {
    let b = parse_time(start)?;
    let e = parse_time(end)?;
    if e <= b {
        return Err(AppError::BadRequest(
            "End time must be after start time.".into(),
        ));
    }
    Ok((e - b).num_minutes())
}

#[derive(Deserialize)]
pub struct RangeQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub user_id: Option<i64>,
    pub status: Option<String>,
}

pub async fn list(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE user_id = ");
    builder.push_bind(u.id);
    if let Some(v) = q.from {
        builder.push(" AND entry_date >= ").push_bind(v);
    }
    if let Some(v) = q.to {
        builder.push(" AND entry_date <= ").push_bind(v);
    }
    builder.push(" ORDER BY entry_date, start_time");
    Ok(Json(
        builder
            .build_query_as::<TimeEntry>()
            .fetch_all(&s.pool)
            .await?,
    ))
}

pub async fn list_all(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<RangeQuery>,
) -> AppResult<Json<Vec<TimeEntry>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE TRUE");
    if let Some(v) = q.from {
        builder.push(" AND entry_date >= ").push_bind(v);
    }
    if let Some(v) = q.to {
        builder.push(" AND entry_date <= ").push_bind(v);
    }
    if let Some(v) = q.user_id {
        builder.push(" AND user_id = ").push_bind(v);
    }
    if let Some(v) = q.status {
        builder.push(" AND status = ").push_bind(v);
    }
    builder.push(" ORDER BY entry_date DESC, start_time");
    Ok(Json(
        builder
            .build_query_as::<TimeEntry>()
            .fetch_all(&s.pool)
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
    pool: &crate::db::DatabasePool,
    user_id: i64,
    te: &NewTimeEntry,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    if let Some(c) = &te.comment {
        if c.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long (max 2000).".into()));
        }
    }
    // Validate that the category exists and is active.
    let cat_active: Option<bool> =
        sqlx::query_scalar("SELECT active FROM categories WHERE id = $1")
            .bind(te.category_id)
            .fetch_optional(pool)
            .await?;
    match cat_active {
        None => return Err(AppError::BadRequest("Category not found.".into())),
        Some(false) => return Err(AppError::BadRequest("Category is inactive.".into())),
        Some(true) => {}
    }
    if te.entry_date > chrono::Local::now().date_naive() {
        return Err(AppError::BadRequest(
            "Entries in the future are not allowed.".into(),
        ));
    }
    let new_min = duration_min(&te.start_time, &te.end_time)?;
    let start_n = parse_time(&te.start_time)?;
    let end_n = parse_time(&te.end_time)?;

    let existing: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, start_time, end_time, status FROM time_entries WHERE user_id=$1 AND entry_date=$2",
    )
    .bind(user_id)
    .bind(te.entry_date)
    .fetch_all(pool)
    .await?;

    let mut day_total = new_min;
    for (id, b, e, status) in &existing {
        if Some(*id) == exclude_id {
            continue;
        }
        // Rejected entries are effectively void: they do not occupy a time slot
        // and must not count toward the daily 14-hour cap.
        if status == "rejected" {
            continue;
        }
        let bb = parse_time(b)?;
        let ee = parse_time(e)?;
        if start_n < ee && bb < end_n {
            return Err(AppError::BadRequest(
                "Overlap with an existing entry.".into(),
            ));
        }
        day_total += (ee - bb).num_minutes();
    }
    if day_total > 14 * 60 {
        return Err(AppError::BadRequest("Day total exceeds 14 hours.".into()));
    }
    Ok(())
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewTimeEntry>,
) -> AppResult<Json<TimeEntry>> {
    validate(&s.pool, u.id, &b, None).await?;
    let id: i64 = sqlx::query_scalar("INSERT INTO time_entries(user_id, entry_date, start_time, end_time, category_id, comment) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(u.id).bind(b.entry_date).bind(&b.start_time).bind(&b.end_time).bind(b.category_id).bind(&b.comment)
        .fetch_one(&s.pool).await?;
    let z: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "created",
        "time_entries",
        id,
        None,
        Some(serde_json::to_value(&z).unwrap()),
    )
    .await;
    Ok(Json(z))
}

pub async fn update(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<NewTimeEntry>,
) -> AppResult<Json<TimeEntry>> {
    let prev: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    let admin_correction = u.is_admin() && (prev.status == "approved" || prev.status == "submitted");
    if !admin_correction {
        if prev.user_id != u.id {
            return Err(AppError::Forbidden);
        }
        if prev.status != "draft" {
            return Err(AppError::BadRequest(
                "Only drafts can be edited directly. Please file a change request.".into(),
            ));
        }
    }
    validate(&s.pool, prev.user_id, &b, Some(id)).await?;
    sqlx::query("UPDATE time_entries SET entry_date=$1, start_time=$2, end_time=$3, category_id=$4, comment=$5, updated_at=CURRENT_TIMESTAMP WHERE id=$6")
        .bind(b.entry_date).bind(&b.start_time).bind(&b.end_time).bind(b.category_id).bind(&b.comment).bind(id)
        .execute(&s.pool).await?;
    let next: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "updated",
        "time_entries",
        id,
        Some(serde_json::to_value(&prev).unwrap()),
        Some(serde_json::to_value(&next).unwrap()),
    )
    .await;
    Ok(Json(next))
}

pub async fn delete(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let z: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if z.user_id != u.id {
        return Err(AppError::Forbidden);
    }
    if z.status != "draft" {
        return Err(AppError::BadRequest("Only drafts can be deleted.".into()));
    }
    sqlx::query("DELETE FROM time_entries WHERE id=$1")
        .bind(id)
        .execute(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "deleted",
        "time_entries",
        id,
        Some(serde_json::to_value(&z).unwrap()),
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
    State(s): State<AppState>,
    u: User,
    Json(b): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    if b.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    // Phase 1: validate ownership for ALL entries before any writes, so a
    // mixed-ownership batch never partially submits.
    for id in &b.ids {
        let z: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
        if z.user_id != u.id {
            return Err(AppError::Forbidden);
        }
    }
    // Phase 2: atomically submit all draft entries in a single transaction.
    let mut tx = s.pool.begin().await?;
    let mut submitted: Vec<i64> = vec![];
    for id in &b.ids {
        let rows = sqlx::query(
            "UPDATE time_entries SET status='submitted', submitted_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND status='draft' AND user_id=$2",
        )
        .bind(id)
        .bind(u.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows > 0 {
            submitted.push(*id);
        }
    }
    tx.commit().await?;
    // Phase 3: audit logs (best-effort, after commit).
    for id in &submitted {
        audit::log(
            &s.pool,
            u.id,
            "status_changed",
            "time_entries",
            *id,
            Some(serde_json::json!({"status": "draft"})),
            Some(serde_json::json!({"status": "submitted"})),
        )
        .await;
    }
    let count = submitted.len();
    // Phase 4: notify the approver with the actual submitted count.
    if count > 0 {
        let approver_id: Option<i64> =
            sqlx::query_scalar("SELECT approver_id FROM users WHERE id=$1")
                .bind(u.id)
                .fetch_optional(&s.pool)
                .await?
                .flatten();
        let notify_id = approver_id.unwrap_or(u.id);
        crate::notifications::create(
            &s,
            notify_id,
            "timesheet_submitted",
            &format!("{} {} submitted a timesheet", u.first_name, u.last_name),
            &format!("{} entries submitted for approval", count),
            Some("time_entries"),
            None,
        )
        .await;
    }
    Ok(Json(serde_json::json!({"ok": true, "count": count})))
}

pub async fn approve(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let z: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if z.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    if z.status != "submitted" {
        return Err(AppError::BadRequest(
            "Only submitted entries can be approved.".into(),
        ));
    }
    sqlx::query("UPDATE time_entries SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2")
        .bind(u.id).bind(id).execute(&s.pool).await?;
    audit::log(
        &s.pool,
        u.id,
        "approved",
        "time_entries",
        id,
        Some(serde_json::to_value(&z).unwrap()),
        Some(serde_json::json!({"status": "approved", "reviewed_by": u.id})),
    )
    .await;
    crate::notifications::create(
        &s,
        z.user_id,
        "timesheet_approved",
        "Timesheet approved",
        &format!("Your timesheet entry for {} has been approved.", z.entry_date),
        Some("time_entries"),
        Some(id),
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
    let z: TimeEntry = sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if z.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    if z.status != "submitted" {
        return Err(AppError::BadRequest(
            "Only submitted entries can be rejected.".into(),
        ));
    }
    if b.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    sqlx::query("UPDATE time_entries SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3")
        .bind(u.id).bind(&b.reason).bind(id).execute(&s.pool).await?;
    audit::log(
        &s.pool,
        u.id,
        "rejected",
        "time_entries",
        id,
        Some(serde_json::to_value(&z).unwrap()),
        Some(serde_json::json!({"status": "rejected", "reason": b.reason})),
    )
    .await;
    crate::notifications::create(
        &s,
        z.user_id,
        "timesheet_rejected",
        "Timesheet rejected",
        &format!(
            "Your timesheet entry for {} was rejected: {}",
            z.entry_date, b.reason
        ),
        Some("time_entries"),
        Some(id),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn batch_approve(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<IdsBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut count = 0;
    for id in &b.ids {
        let z: Option<TimeEntry> =
            sqlx::query_as("SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at FROM time_entries WHERE id=$1 AND status='submitted'")
                .bind(id)
                .fetch_optional(&s.pool)
                .await?;
        let Some(z) = z else { continue };
        if z.user_id == u.id && !u.is_admin() {
            continue;
        }
        sqlx::query("UPDATE time_entries SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2")
            .bind(u.id).bind(id).execute(&s.pool).await?;
        audit::log(
            &s.pool,
            u.id,
            "approved",
            "time_entries",
            *id,
            Some(serde_json::to_value(&z).unwrap()),
            Some(serde_json::json!({"status": "approved", "reviewed_by": u.id})),
        )
        .await;
        crate::notifications::create(
            &s,
            z.user_id,
            "timesheet_approved",
            "Timesheet approved",
            &format!("Your timesheet entry for {} has been approved.", z.entry_date),
            Some("time_entries"),
            Some(*id),
        )
        .await;
        count += 1;
    }
    Ok(Json(serde_json::json!({"ok":true, "count": count})))
}

#[derive(Deserialize)]
pub struct BatchRejectBody {
    pub ids: Vec<i64>,
    pub reason: String,
}

pub async fn batch_reject(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<BatchRejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let reason = b.reason.trim().to_string();
    if reason.is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    if b.ids.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    // Fetch all submitted entries that this lead is allowed to reject.
    let mut to_reject: Vec<TimeEntry> = vec![];
    for id in &b.ids {
        let z: Option<TimeEntry> = sqlx::query_as(
            "SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, \
             status, submitted_at, reviewed_by, reviewed_at, rejection_reason, \
             created_at, updated_at FROM time_entries WHERE id=$1 AND status='submitted'"
        )
        .bind(id)
        .fetch_optional(&s.pool)
        .await?;
        let Some(z) = z else { continue };
        if z.user_id == u.id && !u.is_admin() {
            continue;
        }
        to_reject.push(z);
    }
    if to_reject.is_empty() {
        return Ok(Json(serde_json::json!({"ok": true, "count": 0})));
    }
    // Atomically reject all eligible entries.
    let mut tx = s.pool.begin().await?;
    for z in &to_reject {
        sqlx::query(
            "UPDATE time_entries SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3",
        )
        .bind(u.id)
        .bind(&reason)
        .bind(z.id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let count = to_reject.len();
    // Audit + notify each affected employee (best-effort, after commit).
    for z in &to_reject {
        audit::log(
            &s.pool,
            u.id,
            "rejected",
            "time_entries",
            z.id,
            Some(serde_json::to_value(z).unwrap()),
            Some(serde_json::json!({"status": "rejected", "reason": reason})),
        )
        .await;
        crate::notifications::create(
            &s,
            z.user_id,
            "timesheet_rejected",
            "Timesheet rejected",
            &format!(
                "Your timesheet entry for {} was rejected: {}",
                z.entry_date, reason
            ),
            Some("time_entries"),
            Some(z.id),
        )
        .await;
    }
    Ok(Json(serde_json::json!({"ok": true, "count": count})))
}
