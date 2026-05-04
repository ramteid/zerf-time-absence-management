use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::{Postgres, QueryBuilder};

pub async fn log(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    action: &str,
    table_name: &str,
    record_id: i64,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
) {
    let v = before.map(|x| x.to_string());
    let n = after.map(|x| x.to_string());
    let _ = sqlx::query("INSERT INTO audit_log(user_id, action, table_name, record_id, before_data, after_data) VALUES ($1,$2,$3,$4,$5,$6)")
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

pub async fn list(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<LogQuery>,
) -> AppResult<Json<Vec<LogEntry>>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, action, table_name, record_id, before_data, after_data, occurred_at FROM audit_log WHERE TRUE");
    if q.table_name.is_some() {
        builder.push(" AND table_name = ").push_bind(q.table_name);
    }
    if q.record_id.is_some() {
        builder.push(" AND record_id = ").push_bind(q.record_id);
    }
    if q.user_id.is_some() {
        builder.push(" AND user_id = ").push_bind(q.user_id);
    }
    builder.push(" ORDER BY occurred_at DESC LIMIT 500");
    Ok(Json(
        builder
            .build_query_as::<LogEntry>()
            .fetch_all(&s.pool)
            .await?,
    ))
}
