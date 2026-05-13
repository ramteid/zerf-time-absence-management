use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
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
    pub counts_as_work: bool,
    pub active: bool,
}

#[derive(Clone)]
pub struct CategoryDb {
    pool: DatabasePool,
}

impl CategoryDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Rename the legacy "Direct Childcare" category if it still exists,
    /// then seed the initial category list if none exist yet.
    pub async fn ensure_initial(&self) -> AppResult<()> {
        sqlx::query(
            "UPDATE categories SET name = $1 \
             WHERE name = convert_from(decode($2, 'hex'), 'UTF8')",
        )
        .bind("Core Duties")
        .bind(LEGACY_CORE_DUTIES_NAME_HEX)
        .execute(&self.pool)
        .await?;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
            .fetch_one(&self.pool)
            .await?;
        if count > 0 {
            return Ok(());
        }

        let initial = [
            ("Core Duties", "#4CAF50", 1i64, true),
            ("Preparation Time", "#2196F3", 2, true),
            ("Leadership Tasks", "#FF9800", 3, true),
            ("Team Meeting", "#9C27B0", 4, true),
            ("Training", "#795548", 5, true),
            ("Other", "#607D8B", 6, true),
            ("Flextime Reduction", "#6D4C41", 7, false),
        ];
        for (name, color, sort_order, counts_as_work) in initial {
            sqlx::query(
                "INSERT INTO categories(name, color, sort_order, counts_as_work) VALUES ($1,$2,$3,$4)",
            )
            .bind(name)
            .bind(color)
            .bind(sort_order)
            .bind(counts_as_work)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn list_active(&self) -> AppResult<Vec<Category>> {
        Ok(sqlx::query_as::<_, Category>(
            "SELECT id, name, description, color, sort_order, counts_as_work, active \
             FROM categories WHERE active=TRUE ORDER BY sort_order, name",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_all(&self) -> AppResult<Vec<Category>> {
        Ok(sqlx::query_as::<_, Category>(
            "SELECT id, name, description, color, sort_order, counts_as_work, active \
             FROM categories ORDER BY active DESC, sort_order, name",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn find_by_id(&self, id: i64) -> AppResult<Option<Category>> {
        Ok(sqlx::query_as::<_, Category>(
            "SELECT id, name, description, color, sort_order, counts_as_work, active \
             FROM categories WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Returns `Some(active_flag)` if the category exists, or `None`.
    pub async fn get_active_flag(&self, id: i64) -> AppResult<Option<bool>> {
        Ok(
            sqlx::query_scalar("SELECT active FROM categories WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        color: &str,
        sort_order: i64,
        counts_as_work: bool,
    ) -> AppResult<i64> {
        sqlx::query_scalar(
            "INSERT INTO categories(name, description, color, sort_order, counts_as_work) \
             VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(name)
        .bind(description)
        .bind(color)
        .bind(sort_order)
        .bind(counts_as_work)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| AppError::Conflict("Name already exists".into()))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        id: i64,
        name: Option<String>,
        description: Option<Option<String>>,
        color: Option<String>,
        sort_order: Option<i64>,
        counts_as_work: Option<bool>,
        active: Option<bool>,
    ) -> AppResult<()> {
        let update_description = description.is_some();
        let description = description.flatten();
        let result = sqlx::query(
            "UPDATE categories \
             SET name=COALESCE($1,name), description=CASE WHEN $7 THEN $2 ELSE description END, \
                 color=COALESCE($3,color), sort_order=COALESCE($4,sort_order), \
                 counts_as_work=COALESCE($5,counts_as_work), active=COALESCE($6,active) \
             WHERE id=$8",
        )
        .bind(name)
        .bind(description)
        .bind(color)
        .bind(sort_order)
        .bind(counts_as_work)
        .bind(active)
        .bind(update_description)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }
}
