use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(FromRow, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub sort_order: i64,
    pub active: bool,
}

pub async fn ensure_initial(pool: &sqlx::SqlitePool) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }
    let init = [
        ("Direct Childcare", "#4CAF50", 1),
        ("Preparation Time", "#2196F3", 2),
        ("Leadership Tasks", "#FF9800", 3),
        ("Team Meeting", "#9C27B0", 4),
        ("Training", "#795548", 5),
        ("Other", "#607D8B", 6),
    ];
    for (n, c, s) in init {
        sqlx::query("INSERT INTO categories(name, color, sort_order) VALUES (?,?,?)")
            .bind(n)
            .bind(c)
            .bind(s)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn list(State(s): State<AppState>, _u: User) -> AppResult<Json<Vec<Category>>> {
    let r = sqlx::query_as::<_, Category>(
        "SELECT * FROM categories WHERE active=1 ORDER BY sort_order, name",
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(r))
}

#[derive(Deserialize)]
pub struct NewCategory {
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub sort_order: Option<i64>,
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewCategory>,
) -> AppResult<Json<Category>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    let res = sqlx::query(
        "INSERT INTO categories(name, description, color, sort_order) VALUES (?,?,?,?)",
    )
    .bind(&b.name)
    .bind(&b.description)
    .bind(&b.color)
    .bind(b.sort_order.unwrap_or(0))
    .execute(&s.pool)
    .await
    .map_err(|_| AppError::Conflict("Name already exists".into()))?;
    let id = res.last_insert_rowid();
    Ok(Json(
        sqlx::query_as("SELECT * FROM categories WHERE id=?")
            .bind(id)
            .fetch_one(&s.pool)
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
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<UpdateCategory>,
) -> AppResult<Json<Category>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE categories SET name=COALESCE(?,name), description=COALESCE(?,description), color=COALESCE(?,color), sort_order=COALESCE(?,sort_order), active=COALESCE(?,active) WHERE id=?")
        .bind(b.name).bind(b.description).bind(b.color).bind(b.sort_order).bind(b.active).bind(id)
        .execute(&s.pool).await?;
    Ok(Json(
        sqlx::query_as("SELECT * FROM categories WHERE id=?")
            .bind(id)
            .fetch_one(&s.pool)
            .await?,
    ))
}
