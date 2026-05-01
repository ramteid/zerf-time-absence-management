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
            "SELECT * FROM change_requests WHERE user_id=$1 ORDER BY created_at DESC",
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
    Ok(Json(
        sqlx::query_as::<_, ChangeRequest>(
            "SELECT * FROM change_requests WHERE status='open' ORDER BY created_at",
        )
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
    let z: (i64, String) = sqlx::query_as("SELECT user_id, status FROM time_entries WHERE id=$1")
        .bind(b.time_entry_id)
        .fetch_one(&s.pool)
        .await?;
    if z.0 != u.id {
        return Err(AppError::Forbidden);
    }
    if z.1 == "draft" {
        return Err(AppError::BadRequest("Edit drafts directly.".into()));
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO change_requests(time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id")
        .bind(b.time_entry_id).bind(u.id).bind(b.new_date).bind(&b.new_start_time).bind(&b.new_end_time).bind(b.new_category_id).bind(&b.new_comment).bind(&b.reason)
        .fetch_one(&s.pool).await?;
    let a: ChangeRequest = sqlx::query_as("SELECT * FROM change_requests WHERE id=$1")
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
        sqlx::query_as("SELECT * FROM change_requests WHERE id=$1 AND status='open'")
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE time_entries SET entry_date=COALESCE($1,entry_date), start_time=COALESCE($2,start_time), end_time=COALESCE($3,end_time), category_id=COALESCE($4,category_id), comment=COALESCE($5,comment), updated_at=CURRENT_TIMESTAMP WHERE id=$6")
        .bind(a.new_date).bind(&a.new_start_time).bind(&a.new_end_time).bind(a.new_category_id).bind(&a.new_comment).bind(a.time_entry_id)
        .execute(&s.pool).await?;
    sqlx::query("UPDATE change_requests SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2")
        .bind(u.id).bind(id).execute(&s.pool).await?;
    audit::log(&s.pool, u.id, "approved", "change_requests", id, None, None).await;
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
    sqlx::query("UPDATE change_requests SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3")
        .bind(u.id).bind(&b.reason).bind(id).execute(&s.pool).await?;
    audit::log(
        &s.pool,
        u.id,
        "rejected",
        "change_requests",
        id,
        None,
        Some(serde_json::json!({"reason": b.reason})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}
