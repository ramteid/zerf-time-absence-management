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

        // Fix original seed color that clashed with the holiday amber (#f59e0b).
        sqlx::query(
            "UPDATE categories SET color = $1 WHERE name = 'Leadership Tasks' AND color = $2",
        )
        .bind("#84cc16")
        .bind("#FF9800")
        .execute(&self.pool)
        .await?;

        // Maximise hue distance among the six highest-frequency categories.
        // Target hues (°): sick=0, holiday=38, LT=80, PrepTime=142, vacation=217, TM=262, CoreDuties=312.
        // Only patches rows that still carry the original seed color so manual edits are preserved.
        sqlx::query("UPDATE categories SET color = $1 WHERE name = 'Core Duties' AND color = $2")
            .bind("#de35bd")
            .bind("#4CAF50")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "UPDATE categories SET color = $1 WHERE name = 'Preparation Time' AND color = $2",
        )
        .bind("#22c55e")
        .bind("#2196F3")
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE categories SET color = $1 WHERE name = 'Team Meeting' AND color = $2")
            .bind("#7c3aed")
            .bind("#9C27B0")
            .execute(&self.pool)
            .await?;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
            .fetch_one(&self.pool)
            .await?;
        if count > 0 {
            return Ok(());
        }

        let initial = [
            ("Core Duties", "#de35bd", 1i64, true),
            ("Preparation Time", "#22c55e", 2, true),
            ("Leadership Tasks", "#84cc16", 3, true),
            ("Team Meeting", "#7c3aed", 4, true),
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
        let mut tx = self.pool.begin().await?;
        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO categories(name, description, color, sort_order, counts_as_work) \
             VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(name)
        .bind(description)
        .bind(color)
        .bind(sort_order)
        .bind(counts_as_work)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| AppError::conflict("Name already exists"))?;
        // New categories default to enabled for every existing employee. Same
        // transaction as the insert above so a failure here cannot leave a
        // category with zero employees able to use it.
        sqlx::query("INSERT INTO user_category_access (user_id, category_id) SELECT id, $1 FROM users")
            .bind(new_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(new_id)
    }

    /// Enabled employee ids for a category (regardless of category.active).
    pub async fn enabled_user_ids(&self, category_id: i64) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT user_id FROM user_category_access WHERE category_id = $1",
        )
        .bind(category_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Replace the full set of employees enabled for a category. Duplicate
    /// ids in `user_ids` are tolerated (deduplicated) rather than raising a
    /// primary-key conflict; an id that doesn't correspond to a real user
    /// raises a client-facing `BadRequest` instead of a generic 500.
    pub async fn set_enabled_user_ids(&self, category_id: i64, user_ids: &[i64]) -> AppResult<()> {
        let unique_ids: std::collections::HashSet<i64> = user_ids.iter().copied().collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM user_category_access WHERE category_id = $1")
            .bind(category_id)
            .execute(&mut *tx)
            .await?;
        for user_id in unique_ids {
            sqlx::query(
                "INSERT INTO user_category_access (user_id, category_id) VALUES ($1, $2)",
            )
            .bind(user_id)
            .bind(category_id)
            .execute(&mut *tx)
            .await
            .map_err(map_user_access_error)?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn is_enabled_for_user(&self, category_id: i64, user_id: i64) -> AppResult<bool> {
        let exists: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM user_category_access WHERE category_id = $1 AND user_id = $2",
        )
        .bind(category_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    /// Active categories enabled for a specific employee, for time-entry dropdowns.
    pub async fn list_active_for_user(&self, user_id: i64) -> AppResult<Vec<Category>> {
        Ok(sqlx::query_as::<_, Category>(
            "SELECT c.id, c.name, c.description, c.color, c.sort_order, c.counts_as_work, c.active \
             FROM categories c \
             JOIN user_category_access uca ON uca.category_id = c.id AND uca.user_id = $1 \
             WHERE c.active = TRUE ORDER BY c.sort_order, c.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
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

/// Translate a foreign-key violation on `user_category_access.user_id` (a
/// stale/unknown employee id supplied by the caller) into a client-facing
/// `BadRequest` instead of the generic 500 the default mapping would produce.
fn map_user_access_error(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database_error) = &e {
        if database_error.code().as_deref() == Some("23503") {
            return AppError::bad_request("Unknown employee id.");
        }
    }
    AppError::from(e)
}
