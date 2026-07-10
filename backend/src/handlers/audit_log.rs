use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::services::audit_log::{self, AuditLogPage};
use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AuditLogQuery {
    pub table_name: Option<String>,
    pub record_id: Option<i64>,
    pub user_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// GET /audit-log — paginated audit history with optional filters (admin only).
pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<AuditLogQuery>,
) -> AppResult<Json<AuditLogPage>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(
        audit_log::list_page(
            &app_state,
            query.table_name,
            query.record_id,
            query.user_id,
            query.limit,
            query.offset,
        )
        .await?,
    ))
}
