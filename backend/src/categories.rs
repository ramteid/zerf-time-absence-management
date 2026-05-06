use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

const LEGACY_CORE_DUTIES_NAME_HEX: &str = "446972656374204368696c6463617265";

#[derive(FromRow, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub sort_order: i64,
    pub active: bool,
}

pub async fn ensure_initial(pool: &crate::db::DatabasePool) -> AppResult<()> {
    sqlx::query(
        "UPDATE categories SET name = $1 WHERE name = convert_from(decode($2, 'hex'), 'UTF8')",
    )
    .bind("Core Duties")
    .bind(LEGACY_CORE_DUTIES_NAME_HEX)
    .execute(pool)
    .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }
    let initial_categories = [
        ("Core Duties", "#4CAF50", 1),
        ("Preparation Time", "#2196F3", 2),
        ("Leadership Tasks", "#FF9800", 3),
        ("Team Meeting", "#9C27B0", 4),
        ("Training", "#795548", 5),
        ("Other", "#607D8B", 6),
    ];
    for (name, color, sort_order) in initial_categories {
        sqlx::query("INSERT INTO categories(name, color, sort_order) VALUES ($1,$2,$3)")
            .bind(name)
            .bind(color)
            .bind(sort_order)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn list(State(app_state): State<AppState>, _requester: User) -> AppResult<Json<Vec<Category>>> {
    let categories = sqlx::query_as::<_, Category>(
        "SELECT id, name, description, color, sort_order, active FROM categories WHERE active=TRUE ORDER BY sort_order, name",
    )
    .fetch_all(&app_state.pool)
    .await?;
    Ok(Json(categories))
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
    let new_category_id: i64 = sqlx::query_scalar(
        "INSERT INTO categories(name, description, color, sort_order) VALUES ($1,$2,$3,$4) RETURNING id",
    )
    .bind(&name)
    .bind(&body.description)
    .bind(&color)
    .bind(body.sort_order.unwrap_or(0))
    .fetch_one(&app_state.pool)
    .await
    .map_err(|_| AppError::Conflict("Name already exists".into()))?;
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name, description, color, sort_order, active FROM categories WHERE id=$1",
        )
        .bind(new_category_id)
        .fetch_one(&app_state.pool)
        .await?,
    ))
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
    let normalized_name = body.name.map(|name| name.trim().to_string());
    let normalized_color = body.color.map(|color| color.trim().to_string());
    sqlx::query("UPDATE categories SET name=COALESCE($1,name), description=COALESCE($2,description), color=COALESCE($3,color), sort_order=COALESCE($4,sort_order), active=COALESCE($5,active) WHERE id=$6")
        .bind(normalized_name).bind(body.description).bind(normalized_color).bind(body.sort_order).bind(body.active).bind(category_id)
        .execute(&app_state.pool).await?;
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name, description, color, sort_order, active FROM categories WHERE id=$1",
        )
        .bind(category_id)
        .fetch_one(&app_state.pool)
        .await?,
    ))
}
