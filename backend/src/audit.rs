use crate::error::{AppError, AppResult};
use crate::auth::User;
use crate::AppState;
use axum::{extract::{State, Query}, Json};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

pub async fn log(pool: &sqlx::SqlitePool, user_id: i64, action: &str, table_name: &str, record_id: i64, before: Option<serde_json::Value>, after: Option<serde_json::Value>) {
    let v = before.map(|x| x.to_string());
    let n = after.map(|x| x.to_string());
    let _ = sqlx::query("INSERT INTO audit_log(user_id, action, table_name, record_id, before_data, after_data) VALUES (?,?,?,?,?,?)")
        .bind(user_id).bind(action).bind(table_name).bind(record_id).bind(v).bind(n)
        .execute(pool).await;
}

#[derive(FromRow, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub user_id: i64,
    pub action: String,
    pub table_name: String,
    pub record_id: i64,
    pub before_data: Option<String>,
    pub after_data: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub table_name: Option<String>,
    pub record_id: Option<i64>,
    pub user_id: Option<i64>,
}

pub async fn list(State(s): State<AppState>, u: User, Query(q): Query<LogQuery>) -> AppResult<Json<Vec<LogEntry>>> {
    if !u.is_admin() { return Err(AppError::Forbidden); }
    let mut sql = "SELECT * FROM audit_log WHERE 1=1".to_string();
    if q.table_name.is_some() { sql += " AND table_name = ?"; }
    if q.record_id.is_some() { sql += " AND record_id = ?"; }
    if q.user_id.is_some() { sql += " AND user_id = ?"; }
    sql += " ORDER BY occurred_at DESC LIMIT 500";
    let mut qx = sqlx::query_as::<_, LogEntry>(&sql);
    if let Some(t) = q.table_name { qx = qx.bind(t); }
    if let Some(id) = q.record_id { qx = qx.bind(id); }
    if let Some(id) = q.user_id { qx = qx.bind(id); }
    Ok(Json(qx.fetch_all(&s.pool).await?))
}
