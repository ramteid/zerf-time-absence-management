use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Configurable absence category. The legacy hardcoded kinds
/// (vacation/sick/training/special_leave/unpaid/general_absence/flextime_reduction)
/// are seeded as rows; admins can add/rename/recolor/deactivate freely. The
/// three behavior flags drive the application logic that used to be wired to
/// magic slug constants.
#[derive(FromRow, Serialize, Deserialize, Clone, Debug)]
pub struct AbsenceCategory {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub color: String,
    pub sort_order: i64,
    pub active: bool,
    /// Deducts from the user's annual vacation balance (carryover/expiry).
    pub counts_as_vacation: bool,
    /// Keeps the daily work target — the absence "costs" flextime instead of
    /// being a free day off. Cannot be combined with `counts_as_vacation`.
    pub keeps_work_target: bool,
    /// Sick-like behavior: auto-approve when start_date <= today, allow
    /// backdating up to 30 days, and coexist with logged time on the same day.
    pub auto_approve_past: bool,
}

const ABS_CAT_COLUMNS: &str =
    "id, slug, name, color, sort_order, active, counts_as_vacation, keeps_work_target, auto_approve_past";

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
        Ok(sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
            "SELECT {ABS_CAT_COLUMNS} FROM absence_categories \
             WHERE active=TRUE ORDER BY sort_order, name"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_all(&self) -> AppResult<Vec<AbsenceCategory>> {
        Ok(sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
            "SELECT {ABS_CAT_COLUMNS} FROM absence_categories \
             ORDER BY active DESC, sort_order, name"
        )))
        .fetch_all(&self.pool)
        .await?)
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

    /// Mapping from id to (slug, counts_as_vacation, keeps_work_target,
    /// auto_approve_past). Used by callers that need to evaluate behavior flags
    /// for a small set of category ids without re-querying each one.
    pub async fn behavior_map(&self) -> AppResult<Vec<AbsenceCategory>> {
        // Behavior decisions ignore the active flag: an existing absence row
        // whose category was deactivated must still be processed correctly.
        Ok(sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
            "SELECT {ABS_CAT_COLUMNS} FROM absence_categories"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn create(&self, input: NewAbsenceCategory<'_>) -> AppResult<i64> {
        sqlx::query_scalar(
            "INSERT INTO absence_categories \
             (slug, name, color, sort_order, active, counts_as_vacation, keeps_work_target, auto_approve_past) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
        )
        .bind(input.slug)
        .bind(input.name)
        .bind(input.color)
        .bind(input.sort_order)
        .bind(input.active)
        .bind(input.counts_as_vacation)
        .bind(input.keeps_work_target)
        .bind(input.auto_approve_past)
        .fetch_one(&self.pool)
        .await
        .map_err(map_constraint_error)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(&self, id: i64, input: UpdateAbsenceCategory<'_>) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE absence_categories SET \
                name=COALESCE($1,name), \
                color=COALESCE($2,color), \
                sort_order=COALESCE($3,sort_order), \
                active=COALESCE($4,active), \
                counts_as_vacation=COALESCE($5,counts_as_vacation), \
                keeps_work_target=COALESCE($6,keeps_work_target), \
                auto_approve_past=COALESCE($7,auto_approve_past) \
             WHERE id=$8",
        )
        .bind(input.name)
        .bind(input.color)
        .bind(input.sort_order)
        .bind(input.active)
        .bind(input.counts_as_vacation)
        .bind(input.keeps_work_target)
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
    pub counts_as_vacation: bool,
    pub keeps_work_target: bool,
    pub auto_approve_past: bool,
}

pub struct UpdateAbsenceCategory<'a> {
    pub name: Option<&'a str>,
    pub color: Option<&'a str>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
    pub counts_as_vacation: Option<bool>,
    pub keeps_work_target: Option<bool>,
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
        // 23514 = check_violation
        if code == "23514" && constraint == "abs_cat_only_one_cost" {
            return AppError::bad_request(
                "A category cannot both deduct vacation and reduce flextime.",
            );
        }
    }
    AppError::from(e)
}
