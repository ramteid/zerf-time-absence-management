use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::services::app_logs::{self, AppLogPage};
use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AppLogQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// GET /logs — captured warn/error application logs, newest first (admin only).
pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<AppLogQuery>,
) -> AppResult<Json<AppLogPage>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(
        app_logs::list_page(&app_state, query.limit, query.offset).await?,
    ))
}
