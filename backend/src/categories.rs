use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::repository::categories::Category;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;

pub async fn ensure_initial(pool: &crate::db::DatabasePool) -> AppResult<()> {
    let db = crate::repository::CategoryDb::new(pool.clone());
    db.ensure_initial().await
}

pub async fn list(
    State(app_state): State<AppState>,
    _requester: User,
) -> AppResult<Json<Vec<Category>>> {
    Ok(Json(app_state.db.categories.list_active().await?))
}

#[derive(Deserialize)]
pub struct NewCategory {
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub sort_order: Option<i64>,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewCategory>,
) -> AppResult<Json<Category>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::BadRequest("Invalid category name.".into()));
    }
    let color = body.color.trim().to_string();
    if color.is_empty() || color.len() > 30 {
        return Err(AppError::BadRequest("Invalid color.".into()));
    }
    let new_id = app_state
        .db
        .categories
        .create(&name, body.description.as_deref(), &color, body.sort_order.unwrap_or(0))
        .await?;
    let category = app_state.db.categories.find_by_id(new_id).await?
        .ok_or_else(|| AppError::Internal("Created category not found".into()))?;
    Ok(Json(category))
}

#[derive(Deserialize)]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(category_id): Path<i64>,
    Json(body): Json<UpdateCategory>,
) -> AppResult<Json<Category>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if let Some(ref new_name) = body.name {
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed.len() > 200 {
            return Err(AppError::BadRequest("Invalid category name.".into()));
        }
    }
    if let Some(ref new_color) = body.color {
        let trimmed = new_color.trim();
        if trimmed.is_empty() || trimmed.len() > 30 {
            return Err(AppError::BadRequest("Invalid color.".into()));
        }
    }
    let normalized_name = body.name.map(|n| n.trim().to_string());
    let normalized_color = body.color.map(|c| c.trim().to_string());
    app_state.db.categories.update(
        category_id, normalized_name, body.description, normalized_color,
        body.sort_order, body.active,
    ).await?;
    let category = app_state.db.categories.find_by_id(category_id).await?
        .ok_or_else(|| AppError::Internal("Category not found".into()))?;
    Ok(Json(category))
}
