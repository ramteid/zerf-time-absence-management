use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::repository::audit::LogEntry;
use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

pub async fn log(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    action: &str,
    table_name: &str,
    record_id: i64,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
) {
    let db = crate::repository::AuditDb::new(pool.clone());
    db.log(user_id, action, table_name, record_id, before, after)
        .await;
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub table_name: Option<String>,
    pub record_id: Option<i64>,
    pub user_id: Option<i64>,
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<LogQuery>,
) -> AppResult<Json<Vec<LogEntry>>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(
        app_state
            .db
            .audit
            .list(query.table_name, query.record_id, query.user_id)
            .await?,
    ))
}
