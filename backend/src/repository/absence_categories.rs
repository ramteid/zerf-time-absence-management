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
    /// Whether an approved absence in this category actually reduces the
    /// employee's pay. Independent of `cost_type`: a `cost_type == "none"`
    /// category can be paid (special leave, paid training, Bildungsurlaub) or
    /// unpaid — Zerf otherwise has no opinion on pay, only on balances (see
    /// `help_cost_type_none`). Only meaningful when `cost_type == "none"`
    /// (enforced by the `abs_cat_unpaid_requires_none_cost` CHECK): vacation
    /// and flextime categories are always paid through their own mechanics.
    pub unpaid: bool,
    /// Whether absences in this category count toward the "AU" (medical
    /// certificate) threshold calculation — see `services::medical_certificate`.
    /// Independent of `auto_approve_past`: an org may want sick-like behavior
    /// without the category counting toward the threshold, or vice versa.
    pub medical_certificate_relevant: bool,
    /// Default annual entitlement for this category's leave account. Present
    /// exactly when `cost_type == "vacation"` (enforced by migration 039).
    pub leave_account_default_days: Option<i64>,
    /// MM-DD date on which carryover from this account expires. Present
    /// exactly when this category owns a leave account.
    pub leave_account_carryover_expiry: Option<String>,
    /// Internal first entitlement year for the account. It is used only by
    /// balance calculations and is intentionally omitted from public DTOs.
    #[serde(skip_serializing)]
    pub leave_account_start_year: Option<i32>,
}

impl AbsenceCategory {
    /// True when this category owns an independent leave account.
    pub fn has_leave_account(&self) -> bool {
        self.cost_type == COST_TYPE_VACATION
    }

    /// True when an approved absence in this category keeps the day's work
    /// target — the absence "costs" the employee's flextime balance.
    pub fn is_flextime_cost(&self) -> bool {
        self.cost_type == COST_TYPE_FLEXTIME
    }

    /// True for categories the monthly payroll report includes automatically:
    /// sick-like categories (`auto_approve_past`) and categories explicitly
    /// marked `unpaid`. Vacation- and flextime-cost categories are excluded
    /// because their cost already shows up in the leave/flextime balances;
    /// paid `cost_type == "none"` categories (special leave, paid training,
    /// Bildungsurlaub) are excluded because they don't change what payroll
    /// has to pay out.
    pub fn is_payroll_relevant(&self) -> bool {
        self.auto_approve_past || self.unpaid
    }
}

const ABS_CAT_COLUMNS: &str =
    "id, slug, name, color, sort_order, active, cost_type, auto_approve_past, unpaid, \
     medical_certificate_relevant, \
     leave_account_default_days, leave_account_carryover_expiry, leave_account_start_year";

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

    /// Insert a category in the caller-owned transaction. Category access and
    /// leave-account seeding are intentionally separate repository operations:
    /// services own the complete transaction boundary and shared advisory lock.
    pub async fn create_tx(
        tx: &mut sqlx::PgConnection,
        input: NewAbsenceCategory<'_>,
    ) -> AppResult<i64> {
        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO absence_categories \
             (slug, name, color, sort_order, active, cost_type, auto_approve_past, unpaid, \
              medical_certificate_relevant, \
              leave_account_default_days, leave_account_carryover_expiry, leave_account_start_year) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id",
        )
        .bind(input.slug)
        .bind(input.name)
        .bind(input.color)
        .bind(input.sort_order)
        .bind(input.active)
        .bind(input.cost_type)
        .bind(input.auto_approve_past)
        .bind(input.unpaid)
        .bind(input.medical_certificate_relevant)
        .bind(input.leave_account_default_days)
        .bind(input.leave_account_carryover_expiry)
        .bind(input.leave_account_start_year)
        .fetch_one(tx)
        .await
        .map_err(map_constraint_error)?;
        Ok(new_id)
    }

    /// Grant a newly created category to every active, non-archived, time-tracking user.
    pub async fn grant_default_access_to_all_users_tx(
        tx: &mut sqlx::PgConnection,
        category_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_absence_category_access (user_id, category_id)
             SELECT id, $1 FROM users WHERE active=TRUE AND archived_at IS NULL AND tracks_time=TRUE
             ON CONFLICT (user_id, category_id) DO NOTHING",
        )
        .bind(category_id)
        .execute(tx)
        .await?;
        Ok(())
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

    /// Transaction-bound form of [`Self::enabled_user_ids`], for callers that
    /// must read the current access set under the user-graph advisory lock
    /// before diffing it against a new set (e.g. leave-account reconciliation
    /// in `services::absence_categories::set_category_users`).
    pub async fn enabled_user_ids_tx(
        tx: &mut sqlx::PgConnection,
        category_id: i64,
    ) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT user_id FROM user_absence_category_access WHERE category_id = $1",
        )
        .bind(category_id)
        .fetch_all(tx)
        .await?)
    }

    /// Replace the full set of employees enabled for an absence category, in
    /// the caller-owned transaction. Duplicate ids in `user_ids` are
    /// tolerated (deduplicated) rather than raising a primary-key conflict;
    /// an id that doesn't correspond to a real user raises a client-facing
    /// `BadRequest` instead of a generic 500.
    ///
    /// The caller owns the transaction (rather than this method opening its
    /// own, as it used to) because for leave-account categories the access
    /// diff must be reconciled with `user_leave_accounts` atomically — see
    /// `services::absence_categories::set_category_users`.
    pub async fn set_enabled_user_ids_tx(
        tx: &mut sqlx::PgConnection,
        category_id: i64,
        user_ids: &[i64],
    ) -> AppResult<()> {
        let unique_ids: std::collections::HashSet<i64> = user_ids.iter().copied().collect();
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
        Ok(())
    }

    pub async fn is_enabled_for_user(&self, category_id: i64, user_id: i64) -> AppResult<bool> {
        let exists: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM user_absence_category_access uaca JOIN absence_categories c ON c.id = uaca.category_id WHERE uaca.category_id = $1 AND uaca.user_id = $2 AND c.active=TRUE",
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
            "SELECT c.id, c.slug, c.name, c.color, c.sort_order, c.active, c.cost_type, c.auto_approve_past, c.unpaid, \
                    c.medical_certificate_relevant, \
                    c.leave_account_default_days, c.leave_account_carryover_expiry, c.leave_account_start_year \
             FROM absence_categories c \
             JOIN user_absence_category_access uaca ON uaca.category_id = c.id AND uaca.user_id = $1 \
             WHERE c.active = TRUE ORDER BY c.sort_order, c.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Absence categories the employee may currently request, plus categories
    /// still referenced by live absences. The projected `active` flag is
    /// user-specific: it is true only when the category is globally active and
    /// the employee still has access. This keeps access-revoked categories out
    /// of request dropdowns while preserving behavior metadata for existing
    /// requested/approved/cancellation-pending rows.
    pub async fn list_all_for_user(&self, user_id: i64) -> AppResult<Vec<AbsenceCategory>> {
        Ok(sqlx::query_as::<_, AbsenceCategory>(
            "SELECT c.id, c.slug, c.name, c.color, c.sort_order, \
                    (c.active AND uaca.user_id IS NOT NULL) AS active, \
                    c.cost_type, c.auto_approve_past, c.unpaid, \
                    c.medical_certificate_relevant, \
                    c.leave_account_default_days, c.leave_account_carryover_expiry, c.leave_account_start_year \
             FROM absence_categories c \
             LEFT JOIN user_absence_category_access uaca \
                    ON uaca.category_id = c.id AND uaca.user_id = $1 \
             WHERE uaca.user_id IS NOT NULL \
                OR EXISTS ( \
                    SELECT 1 FROM absences a \
                    WHERE a.user_id = $1 AND a.category_id = c.id \
                    AND a.status IN ('requested','approved','cancellation_pending') \
                ) \
             ORDER BY c.sort_order, c.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Update a category in the caller-owned transaction.
    pub async fn update_tx(
        tx: &mut sqlx::PgConnection,
        id: i64,
        input: UpdateAbsenceCategory<'_>,
    ) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE absence_categories SET \
                name=COALESCE($1,name), \
                color=COALESCE($2,color), \
                sort_order=COALESCE($3,sort_order), \
                active=COALESCE($4,active), \
                cost_type=COALESCE($5,cost_type), \
                auto_approve_past=COALESCE($6,auto_approve_past), \
                unpaid=COALESCE($7,unpaid), \
                medical_certificate_relevant=COALESCE($8,medical_certificate_relevant), \
                leave_account_default_days=COALESCE($9,leave_account_default_days), \
                leave_account_carryover_expiry=COALESCE($10,leave_account_carryover_expiry), \
                leave_account_start_year=COALESCE($11,leave_account_start_year) \
             WHERE id=$12",
        )
        .bind(input.name)
        .bind(input.color)
        .bind(input.sort_order)
        .bind(input.active)
        .bind(input.cost_type)
        .bind(input.auto_approve_past)
        .bind(input.unpaid)
        .bind(input.medical_certificate_relevant)
        .bind(input.leave_account_default_days)
        .bind(input.leave_account_carryover_expiry)
        .bind(input.leave_account_start_year)
        .bind(id)
        .execute(tx)
        .await
        .map_err(map_constraint_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// Lock and return a category as part of a broader category/user graph
    /// mutation. The service uses this to evaluate valid cost-type transitions
    /// against a stable current row before updating it.
    pub async fn find_by_id_tx(
        tx: &mut sqlx::PgConnection,
        id: i64,
    ) -> AppResult<Option<AbsenceCategory>> {
        Ok(
            sqlx::query_as::<_, AbsenceCategory>(sqlx::AssertSqlSafe(format!(
                "SELECT {ABS_CAT_COLUMNS} FROM absence_categories WHERE id=$1 FOR UPDATE"
            )))
            .bind(id)
            .fetch_optional(tx)
            .await?,
        )
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

    /// Transaction-bound variant for TOCTOU-safe checks inside a lock.
    pub async fn usage_count_tx(tx: &mut sqlx::PgConnection, id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM absences WHERE category_id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
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
    /// Only valid when `cost_type == "none"` (enforced by the
    /// `abs_cat_unpaid_requires_none_cost` CHECK); service-layer code
    /// validates this before passing it through.
    pub unpaid: bool,
    pub medical_certificate_relevant: bool,
    /// Required for `cost_type = 'vacation'`; otherwise it must be `None`.
    pub leave_account_default_days: Option<i64>,
    /// Required MM-DD carryover expiry for `cost_type = 'vacation'`.
    pub leave_account_carryover_expiry: Option<&'a str>,
    /// Internal first entitlement year. The service determines this from the
    /// application timezone and never accepts it from a public request.
    pub leave_account_start_year: Option<i32>,
}

pub struct UpdateAbsenceCategory<'a> {
    pub name: Option<&'a str>,
    pub color: Option<&'a str>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
    pub cost_type: Option<&'a str>,
    pub auto_approve_past: Option<bool>,
    pub unpaid: Option<bool>,
    pub medical_certificate_relevant: Option<bool>,
    pub leave_account_default_days: Option<i64>,
    pub leave_account_carryover_expiry: Option<&'a str>,
    pub leave_account_start_year: Option<i32>,
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
        // 23514 = check_violation. The service layer validates both of these
        // up front (`validate_cost_type`, the unpaid/cost_type pairing), so
        // these branches only fire if a future direct-SQL caller bypasses the
        // service. Keep the mapping anyway so the error stays user-facing
        // instead of a 500.
        if code == "23514" && constraint == "abs_cat_cost_type" {
            return AppError::bad_request(
                "Invalid cost_type; expected 'none', 'vacation', or 'flextime'.",
            );
        }
        if code == "23514" && constraint == "abs_cat_unpaid_requires_none_cost" {
            return AppError::bad_request("Unpaid can only be set when cost_type is 'none'.");
        }
        if code == "23514" && constraint == "abs_cat_leave_account_default_days_range" {
            return AppError::bad_request("Leave-account default days must be between 0 and 366.");
        }
        if code == "23514" && constraint == "abs_cat_leave_account_carryover_expiry_format" {
            return AppError::bad_request(
                "Leave-account carryover expiry must be a real MM-DD date.",
            );
        }
        if code == "23514" && constraint == "abs_cat_leave_account_fields_match_cost_type" {
            return AppError::bad_request(
                "Leave-account fields are required only for cost_type 'vacation'.",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn category(cost_type: &str, auto_approve_past: bool, unpaid: bool) -> AbsenceCategory {
        AbsenceCategory {
            id: 1,
            slug: "test".to_string(),
            name: "Test".to_string(),
            color: "#000000".to_string(),
            sort_order: 0,
            active: true,
            cost_type: cost_type.to_string(),
            auto_approve_past,
            unpaid,
            medical_certificate_relevant: false,
            leave_account_default_days: (cost_type == COST_TYPE_VACATION).then_some(30),
            leave_account_carryover_expiry: (cost_type == COST_TYPE_VACATION)
                .then_some("03-31".to_string()),
            leave_account_start_year: (cost_type == COST_TYPE_VACATION).then_some(2026),
        }
    }

    /// A `cost_type == "none"` category is not payroll-relevant unless it is
    /// also marked `unpaid`: paid special leave, paid training, and legally
    /// mandated paid educational leave (Bildungsurlaub) are all
    /// `cost_type == "none"` too, and none of them reduce salary.
    #[test]
    fn cost_type_none_alone_is_not_payroll_relevant() {
        let paid_special_leave = category(COST_TYPE_NONE, false, false);
        assert!(!paid_special_leave.is_payroll_relevant());
    }

    #[test]
    fn unpaid_flag_makes_a_none_cost_category_payroll_relevant() {
        let unpaid_leave = category(COST_TYPE_NONE, false, true);
        assert!(unpaid_leave.is_payroll_relevant());
    }

    #[test]
    fn auto_approve_past_makes_a_category_payroll_relevant_regardless_of_unpaid() {
        let sick = category(COST_TYPE_NONE, true, false);
        assert!(sick.is_payroll_relevant());
    }

    #[test]
    fn vacation_and_flextime_cost_categories_are_never_payroll_relevant() {
        assert!(!category(COST_TYPE_VACATION, false, false).is_payroll_relevant());
        assert!(!category(COST_TYPE_FLEXTIME, false, false).is_payroll_relevant());
    }
}
