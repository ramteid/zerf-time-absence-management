use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Where the cost of an approved absence is charged. Replaces the pre-019
/// boolean pair (`counts_as_vacation`, `keeps_work_target`) — those were
/// always mutex (enforced by the dropped `abs_cat_only_one_cost` CHECK), so
/// they expressed one logical concept with three states, not two
/// independent flags. The single field makes the invariant impossible to
/// violate by construction.
///
/// Stored as TEXT with a CHECK constraint at the DB level (see migration
/// 019). We keep the Rust side as a `String` for two reasons:
/// (1) `sqlx::FromRow` derive plays cleanly with `String` and no enum
///     `Type`/`Encode`/`Decode` boilerplate is needed.
/// (2) The constants and helpers below give the same exhaustiveness in
///     practice — every consumer goes through `is_vacation_cost` /
///     `is_flextime_cost` rather than matching raw strings.
pub const COST_TYPE_NONE: &str = "none";
pub const COST_TYPE_VACATION: &str = "vacation";
pub const COST_TYPE_FLEXTIME: &str = "flextime";

/// Validate a user-supplied cost_type string against the DB CHECK whitelist.
/// Centralizes the membership test so callers don't compare raw strings.
pub fn validate_cost_type(value: &str) -> AppResult<()> {
    match value {
        COST_TYPE_NONE | COST_TYPE_VACATION | COST_TYPE_FLEXTIME => Ok(()),
        other => Err(AppError::BadRequest(format!(
            "Invalid cost_type {other:?}; expected 'none', 'vacation', or 'flextime'."
        ))),
    }
}

/// Configurable absence category. The legacy hardcoded kinds
/// (vacation/sick/training/special_leave/unpaid/general_absence/flextime_reduction)
/// are seeded as rows; admins can add/rename/recolor/deactivate freely. The
/// behavior fields drive the application logic that used to be wired to
/// magic slug constants.
#[derive(FromRow, Serialize, Deserialize, Clone, Debug)]
pub struct AbsenceCategory {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub color: String,
    pub sort_order: i64,
    pub active: bool,
    /// Where the cost of an approved absence is charged. One of
    /// `'none'` (no deduction), `'vacation'` (annual leave balance), or
    /// `'flextime'` (keeps work target; debits flextime balance).
    /// See `COST_TYPE_*` constants and the helpers below.
    pub cost_type: String,
    /// Sick-like behavior: auto-approve when start_date <= today, allow
    /// backdating up to 30 days, and coexist with logged time on the same day.
    pub auto_approve_past: bool,
}

impl AbsenceCategory {
    /// True when an approved absence in this category deducts from the
    /// employee's annual vacation balance (carryover + expiry rules apply).
    pub fn is_vacation_cost(&self) -> bool {
        self.cost_type == COST_TYPE_VACATION
    }
    /// True when an approved absence in this category keeps the day's work
    /// target — the absence "costs" the employee's flextime balance.
    pub fn is_flextime_cost(&self) -> bool {
        self.cost_type == COST_TYPE_FLEXTIME
    }
}

const ABS_CAT_COLUMNS: &str =
    "id, slug, name, color, sort_order, active, cost_type, auto_approve_past";

#[derive(Clone)]
pub struct AbsenceCategoryDb {
    pool: DatabasePool,
}

impl AbsenceCategoryDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    pub async fn list_active(&self) -> AppResult<Vec<AbsenceCategory>> {
        // AssertSqlSafe: the format interpolates only ABS_CAT_COLUMNS (a compile-time
        // constant), never user input.
        Ok(
            sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
                "SELECT {ABS_CAT_COLUMNS} FROM absence_categories \
             WHERE active=TRUE ORDER BY sort_order, name"
            )))
            .fetch_all(&self.pool)
            .await?,
        )
    }

    pub async fn list_all(&self) -> AppResult<Vec<AbsenceCategory>> {
        Ok(
            sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
                "SELECT {ABS_CAT_COLUMNS} FROM absence_categories \
             ORDER BY active DESC, sort_order, name"
            )))
            .fetch_all(&self.pool)
            .await?,
        )
    }

    pub async fn find_by_id(&self, id: i64) -> AppResult<Option<AbsenceCategory>> {
        Ok(
            sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
                "SELECT {ABS_CAT_COLUMNS} FROM absence_categories WHERE id=$1"
            )))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?,
        )
    }

    pub async fn find_by_slug(&self, slug: &str) -> AppResult<Option<AbsenceCategory>> {
        Ok(
            sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
                "SELECT {ABS_CAT_COLUMNS} FROM absence_categories WHERE slug=$1"
            )))
            .bind(slug)
            .fetch_optional(&self.pool)
            .await?,
        )
    }

    /// Loads every category (including inactive ones) so callers can resolve
    /// behavior fields by slug or id without re-querying per category. Used
    /// by the reports/flextime pipelines that look up `cost_type` for each
    /// absence row in a hot loop.
    pub async fn behavior_map(&self) -> AppResult<Vec<AbsenceCategory>> {
        // Behavior decisions ignore the active flag: an existing absence row
        // whose category was deactivated must still be processed correctly.
        Ok(
            sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
                "SELECT {ABS_CAT_COLUMNS} FROM absence_categories"
            )))
            .fetch_all(&self.pool)
            .await?,
        )
    }

    pub async fn create(&self, input: NewAbsenceCategory<'_>) -> AppResult<i64> {
        let mut tx = self.pool.begin().await?;
        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO absence_categories \
             (slug, name, color, sort_order, active, cost_type, auto_approve_past) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id",
        )
        .bind(input.slug)
        .bind(input.name)
        .bind(input.color)
        .bind(input.sort_order)
        .bind(input.active)
        .bind(input.cost_type)
        .bind(input.auto_approve_past)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
        // New absence categories default to enabled for every existing
        // employee. Same transaction as the insert above so a failure here
        // cannot leave a category with zero employees able to use it.
        sqlx::query(
            "INSERT INTO user_absence_category_access (user_id, category_id) SELECT id, $1 FROM users",
        )
        .bind(new_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(new_id)
    }

    /// Enabled employee ids for an absence category (regardless of category.active).
    pub async fn enabled_user_ids(&self, category_id: i64) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT user_id FROM user_absence_category_access WHERE category_id = $1",
        )
        .bind(category_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Replace the full set of employees enabled for an absence category.
    /// Duplicate ids in `user_ids` are tolerated (deduplicated) rather than
    /// raising a primary-key conflict; an id that doesn't correspond to a
    /// real user raises a client-facing `BadRequest` instead of a generic 500.
    pub async fn set_enabled_user_ids(&self, category_id: i64, user_ids: &[i64]) -> AppResult<()> {
        let unique_ids: std::collections::HashSet<i64> = user_ids.iter().copied().collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM user_absence_category_access WHERE category_id = $1")
            .bind(category_id)
            .execute(&mut *tx)
            .await?;
        for user_id in unique_ids {
            sqlx::query(
                "INSERT INTO user_absence_category_access (user_id, category_id) VALUES ($1, $2)",
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
            "SELECT 1 FROM user_absence_category_access WHERE category_id = $1 AND user_id = $2",
        )
        .bind(category_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    /// Active absence categories enabled for a specific employee, for absence-request dropdowns.
    pub async fn list_active_for_user(&self, user_id: i64) -> AppResult<Vec<AbsenceCategory>> {
        Ok(sqlx::query_as::<_, AbsenceCategory>(
            "SELECT c.id, c.slug, c.name, c.color, c.sort_order, c.active, c.cost_type, c.auto_approve_past \
             FROM absence_categories c \
             JOIN user_absence_category_access uaca ON uaca.category_id = c.id AND uaca.user_id = $1 \
             WHERE c.active = TRUE ORDER BY c.sort_order, c.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn update(&self, id: i64, input: UpdateAbsenceCategory<'_>) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE absence_categories SET \
                name=COALESCE($1,name), \
                color=COALESCE($2,color), \
                sort_order=COALESCE($3,sort_order), \
                active=COALESCE($4,active), \
                cost_type=COALESCE($5,cost_type), \
                auto_approve_past=COALESCE($6,auto_approve_past) \
             WHERE id=$7",
        )
        .bind(input.name)
        .bind(input.color)
        .bind(input.sort_order)
        .bind(input.active)
        .bind(input.cost_type)
        .bind(input.auto_approve_past)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_constraint_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// Count absences referencing a category. Used to decide whether a
    /// deactivation is safe (rows can stay but the category disappears from
    /// new-request menus) and surfaced in admin warnings.
    pub async fn usage_count(&self, id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM absences WHERE category_id=$1")
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }
}

pub struct NewAbsenceCategory<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub sort_order: i64,
    pub active: bool,
    /// One of `'none'` | `'vacation'` | `'flextime'`. Service-layer code
    /// validates via `validate_cost_type` before passing it through.
    pub cost_type: &'a str,
    pub auto_approve_past: bool,
}

pub struct UpdateAbsenceCategory<'a> {
    pub name: Option<&'a str>,
    pub color: Option<&'a str>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
    pub cost_type: Option<&'a str>,
    pub auto_approve_past: Option<bool>,
}

/// Translate the database constraints we care about into client-facing errors.
/// The DB enforces invariants like "slug unique" and "vacation XOR flextime
/// cost" as hard constraints; without this mapping the user would see an
/// opaque 500. Anything we don't recognize falls through to the standard
/// `AppError::from(sqlx::Error)` mapping, which logs and returns Internal.
fn map_constraint_error(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database_error) = &e {
        let constraint = database_error.constraint().unwrap_or("");
        let code = database_error.code().unwrap_or_default();
        // 23505 = unique_violation
        if code == "23505" {
            return AppError::conflict("Absence category slug already exists.");
        }
        // 23514 = check_violation. After migration 019 the only
        // category-level CHECK that user input can violate is the
        // cost_type whitelist — and the service layer validates it up
        // front via `validate_cost_type`, so this branch only fires if a
        // future direct-SQL caller bypasses the service. Keep the mapping
        // anyway so the error stays user-facing instead of a 500.
        if code == "23514" && constraint == "abs_cat_cost_type" {
            return AppError::bad_request(
                "Invalid cost_type; expected 'none', 'vacation', or 'flextime'.",
            );
        }
    }
    AppError::from(e)
}

/// Translate a foreign-key violation on `user_absence_category_access.user_id`
/// (a stale/unknown employee id supplied by the caller) into a client-facing
/// `BadRequest` instead of the generic 500 the default mapping would produce.
fn map_user_access_error(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database_error) = &e {
        if database_error.code().as_deref() == Some("23503") {
            return AppError::bad_request("Unknown employee id.");
        }
    }
    AppError::from(e)
}
