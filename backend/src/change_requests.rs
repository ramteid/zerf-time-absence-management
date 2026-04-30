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
            "SELECT * FROM change_requests WHERE user_id=? ORDER BY created_at DESC",
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
    let z: (i64, String) = sqlx::query_as("SELECT user_id, status FROM time_entries WHERE id=?")
        .bind(b.time_entry_id)
        .fetch_one(&s.pool)
        .await?;
    if z.0 != u.id {
        return Err(AppError::Forbidden);
    }
    if z.1 == "draft" {
        return Err(AppError::BadRequest("Edit drafts directly.".into()));
    }
    let res = sqlx::query("INSERT INTO change_requests(time_entry_id, user_id, new_date, new_start_time, new_end_time, new_category_id, new_comment, reason) VALUES (?,?,?,?,?,?,?,?)")
        .bind(b.time_entry_id).bind(u.id).bind(b.new_date).bind(&b.new_start_time).bind(&b.new_end_time).bind(b.new_category_id).bind(&b.new_comment).bind(&b.reason)
        .execute(&s.pool).await?;
    let id = res.last_insert_rowid();
    let a: ChangeRequest = sqlx::query_as("SELECT * FROM change_requests WHERE id=?")
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
        sqlx::query_as("SELECT * FROM change_requests WHERE id=? AND status='open'")
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE time_entries SET entry_date=COALESCE(?,entry_date), start_time=COALESCE(?,start_time), end_time=COALESCE(?,end_time), category_id=COALESCE(?,category_id), comment=COALESCE(?,comment), updated_at=CURRENT_TIMESTAMP WHERE id=?")
        .bind(a.new_date).bind(&a.new_start_time).bind(&a.new_end_time).bind(a.new_category_id).bind(&a.new_comment).bind(a.time_entry_id)
        .execute(&s.pool).await?;
    sqlx::query("UPDATE change_requests SET status='approved', reviewed_by=?, reviewed_at=CURRENT_TIMESTAMP WHERE id=?")
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
    sqlx::query("UPDATE change_requests SET status='rejected', reviewed_by=?, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=? WHERE id=?")
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
