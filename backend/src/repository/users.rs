use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

const USER_GRAPH_LOCK_KEY: i64 = 0x7A_45_52_46_5F_53_54_55_i64;

/// Full user row returned from the database.
/// Note: approver relationships live in the `user_approvers` junction table,
/// not in this struct (the column was removed in migration 002).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub weekly_hours: f64,
    pub workdays_per_week: i16,
    pub start_date: NaiveDate,
    pub active: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    pub allow_reopen_without_approval: bool,
    pub dark_mode: bool,
    pub overtime_start_balance_min: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
    pub fn is_lead(&self) -> bool {
        self.role == "team_lead" || self.role == "admin"
    }
}

const USER_SELECT: &str =
    "SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, workdays_per_week, \
     start_date, active, must_change_password, created_at, \
     allow_reopen_without_approval, dark_mode, overtime_start_balance_min \
     FROM users";

/// Team settings row (id, email, first_name, last_name, role, allow_reopen_without_approval).
pub type TeamSettingsRow = (i64, String, String, String, String, bool);

#[derive(Serialize, sqlx::FromRow)]
pub struct AnnualLeaveRow {
    pub user_id: i64,
    pub year: i32,
    pub days: i64,
}

#[derive(Clone)]
pub struct UserDb {
    pool: DatabasePool,
}

impl UserDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    // ── Lookups ────────────────────────────────────────────────────────────

    pub async fn find_by_email(&self, email: &str) -> AppResult<Option<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE email = $1"
        ))
        .bind(email)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn find_by_id(&self, id: i64) -> AppResult<Option<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE id=$1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn find_by_id_active(&self, id: i64) -> AppResult<Option<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE id=$1 AND active=TRUE"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn find_all_ordered(&self) -> AppResult<Vec<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} ORDER BY last_name, first_name"
        ))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn find_for_approver(&self, approver_id: i64) -> AppResult<Vec<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE id=$1 \
             OR (id IN (SELECT user_id FROM user_approvers WHERE approver_id=$1) AND role!='admin') \
             ORDER BY last_name, first_name"
        ))
        .bind(approver_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn find_all_active_ordered(&self) -> AppResult<Vec<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE active=TRUE ORDER BY last_name"
        ))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn find_active_team_for_lead(&self, lead_id: i64) -> AppResult<Vec<User>> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE active=TRUE \
             AND (id=$1 OR id IN (SELECT user_id FROM user_approvers WHERE approver_id=$1)) \
             AND role!='admin' ORDER BY last_name"
        ))
        .bind(lead_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn count(&self) -> AppResult<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn count_active_admins(&self) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM users WHERE active=TRUE AND role='admin'",
            )
            .fetch_one(&self.pool)
            .await?,
        )
    }

    pub async fn count_admin_direct_reports(&self, user_id: i64) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_approvers \
             WHERE approver_id=$1 \
             AND user_id IN (SELECT id FROM users WHERE active=TRUE AND role='admin')",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn count_non_admin_direct_reports(&self, user_id: i64) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_approvers \
             WHERE approver_id=$1 \
             AND user_id IN (SELECT id FROM users WHERE active=TRUE AND role!='admin')",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn count_active_direct_reports(&self, user_id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM user_approvers \
                 WHERE approver_id=$1 \
                 AND user_id IN (SELECT id FROM users WHERE active=TRUE)",
            )
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?,
        )
    }

    pub async fn get_active_flag(&self, id: i64) -> AppResult<Option<bool>> {
        Ok(sqlx::query_scalar("SELECT active FROM users WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Returns (role, active) for the given user id.
    pub async fn get_approver_info(&self, id: i64) -> AppResult<Option<(String, bool)>> {
        Ok(
            sqlx::query_as::<_, (String, bool)>("SELECT role, active FROM users WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Returns (id, role, active) for the given user id.
    pub async fn get_id_role_active(&self, id: i64) -> AppResult<Option<(i64, String, bool)>> {
        Ok(
            sqlx::query_as::<_, (i64, String, bool)>(
                "SELECT id, role, active FROM users WHERE id=$1",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?,
        )
    }

    /// Check whether `target_id` is a non-admin direct report of `approver_id`.
    pub async fn is_direct_report(
        &self,
        target_id: i64,
        approver_id: i64,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, Option<bool>>(
            "SELECT TRUE FROM user_approvers ua \
             JOIN users u ON u.id = ua.user_id \
             WHERE ua.user_id=$1 AND ua.approver_id=$2 AND u.role!='admin'",
        )
        .bind(target_id)
        .bind(approver_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten()
        .is_some())
    }

    pub async fn get_primary_admin_id(&self) -> AppResult<Option<i64>> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT id FROM users WHERE active=TRUE AND role='admin' ORDER BY id LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn get_all_admin_ids(&self) -> AppResult<Vec<i64>> {
        Ok(
            sqlx::query_scalar::<_, i64>(
                "SELECT id FROM users WHERE active=TRUE AND role='admin'",
            )
            .fetch_all(&self.pool)
            .await?,
        )
    }

    pub async fn get_start_date(&self, user_id: i64) -> AppResult<NaiveDate> {
        Ok(
            sqlx::query_scalar("SELECT start_date FROM users WHERE id=$1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn get_start_date_and_overtime_balance(
        &self,
        user_id: i64,
    ) -> AppResult<(NaiveDate, i64)> {
        Ok(sqlx::query_as::<_, (NaiveDate, i64)>(
            "SELECT start_date, overtime_start_balance_min FROM users WHERE id=$1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn check_email_available(
        &self,
        email: &str,
        exclude_id: Option<i64>,
    ) -> AppResult<()> {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM users \
             WHERE email=$1 AND ($2::BIGINT IS NULL OR id<>$2) LIMIT 1",
        )
        .bind(email)
        .bind(exclude_id)
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_some() {
            return Err(AppError::Conflict("Email already exists.".into()));
        }
        Ok(())
    }

    pub async fn check_name_available(
        &self,
        first_name: &str,
        last_name: &str,
        exclude_id: Option<i64>,
    ) -> AppResult<()> {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM users \
             WHERE first_name=$1 AND last_name=$2 \
             AND ($3::BIGINT IS NULL OR id<>$3) LIMIT 1",
        )
        .bind(first_name)
        .bind(last_name)
        .bind(exclude_id)
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_some() {
            return Err(AppError::Conflict(
                "First name and last name already exist.".into(),
            ));
        }
        Ok(())
    }

    // ── Team settings ──────────────────────────────────────────────────────

    pub async fn team_settings_all(&self) -> AppResult<Vec<TeamSettingsRow>> {
        Ok(sqlx::query_as::<_, TeamSettingsRow>(
            "SELECT id, email, first_name, last_name, role, \
             allow_reopen_without_approval FROM users \
             WHERE active=TRUE ORDER BY last_name, first_name",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn team_settings_for_lead(&self, lead_id: i64) -> AppResult<Vec<TeamSettingsRow>> {
        Ok(sqlx::query_as::<_, TeamSettingsRow>(
            "SELECT id, email, first_name, last_name, role, \
             allow_reopen_without_approval FROM users \
             WHERE active=TRUE \
             AND (id=$1 OR (id IN (SELECT user_id FROM user_approvers WHERE approver_id=$1) AND role!='admin')) \
             ORDER BY last_name, first_name",
        )
        .bind(lead_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn is_active_direct_report(
        &self,
        target_id: i64,
        approver_id: i64,
    ) -> AppResult<bool> {
        Ok(
            sqlx::query_scalar::<_, Option<bool>>(
                "SELECT TRUE FROM user_approvers ua \
                 JOIN users u ON u.id = ua.user_id \
                 WHERE ua.user_id=$1 AND ua.approver_id=$2 \
                 AND u.active=TRUE AND u.role!='admin'",
            )
            .bind(target_id)
            .bind(approver_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten()
            .is_some(),
        )
    }

    pub async fn update_allow_reopen(
        &self,
        target_id: i64,
        allow: bool,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2",
        )
        .bind(allow)
        .bind(target_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Mutations ──────────────────────────────────────────────────────────

    pub async fn lock_user_graph_tx(tx: &mut sqlx::PgConnection) -> AppResult<()> {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(USER_GRAPH_LOCK_KEY)
            .execute(tx)
            .await?;
        Ok(())
    }

    pub async fn fetch_for_update(
        tx: &mut sqlx::PgConnection,
        id: i64,
    ) -> AppResult<User> {
        Ok(sqlx::query_as::<_, User>(&format!(
            "{USER_SELECT} WHERE id=$1 FOR UPDATE"
        ))
        .bind(id)
        .fetch_one(tx)
        .await?)
    }

    pub async fn create_initial_admin(
        tx: &mut sqlx::PgConnection,
        email: &str,
        password_hash: &str,
        first_name: &str,
        last_name: &str,
        start_date: NaiveDate,
    ) -> AppResult<i64> {
        sqlx::query(
            "INSERT INTO users(email, password_hash, first_name, last_name, role, \
               weekly_hours, workdays_per_week, start_date, must_change_password, overtime_start_balance_min) \
               VALUES ($1, $2, $3, $4, 'admin', 39.0, 5, $5, FALSE, 0)",
        )
        .bind(email)
        .bind(password_hash)
        .bind(first_name)
        .bind(last_name)
        .bind(start_date)
        .execute(&mut *tx)
        .await?;
        let id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
            .bind(email)
            .fetch_one(&mut *tx)
            .await?;
        Ok(id)
    }

    pub async fn count_tx(tx: &mut sqlx::PgConnection) -> AppResult<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(tx)
            .await?)
    }

    /// Insert a new non-admin user row. Approver relationships must be inserted
    /// separately via `insert_approver_tx`.
    pub async fn create(
        tx: &mut sqlx::PgConnection,
        email: &str,
        password_hash: &str,
        first_name: &str,
        last_name: &str,
        role: &str,
        weekly_hours: f64,
        workdays_per_week: i16,
        start_date: NaiveDate,
        must_change_password: bool,
        overtime_start_balance_min: i64,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO users(email, password_hash, first_name, last_name, role, \
             weekly_hours, workdays_per_week, start_date, must_change_password, overtime_start_balance_min) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
        )
        .bind(email)
        .bind(password_hash)
        .bind(first_name)
        .bind(last_name)
        .bind(role)
        .bind(weekly_hours)
        .bind(workdays_per_week)
        .bind(start_date)
        .bind(must_change_password)
        .bind(overtime_start_balance_min)
        .fetch_one(tx)
        .await
    }

    pub async fn update_basic(
        tx: &mut sqlx::PgConnection,
        id: i64,
        email: Option<String>,
        first_name: Option<String>,
        last_name: Option<String>,
        role: Option<String>,
        weekly_hours: Option<f64>,
        workdays_per_week: Option<i16>,
        start_date: Option<NaiveDate>,
        active: Option<bool>,
        allow_reopen_without_approval: Option<bool>,
        overtime_start_balance_min: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE users \
             SET email=COALESCE($1,email), \
                 first_name=COALESCE($2,first_name), \
                 last_name=COALESCE($3,last_name), \
                 role=COALESCE($4,role), \
                 weekly_hours=COALESCE($5,weekly_hours), \
                 workdays_per_week=COALESCE($6,workdays_per_week), \
                 start_date=COALESCE($7,start_date), \
                 active=COALESCE($8,active), \
                 allow_reopen_without_approval=COALESCE($9,allow_reopen_without_approval), \
                 overtime_start_balance_min=COALESCE($10,overtime_start_balance_min) \
             WHERE id=$11",
        )
        .bind(email)
        .bind(first_name)
        .bind(last_name)
        .bind(role)
        .bind(weekly_hours)
        .bind(workdays_per_week)
        .bind(start_date)
        .bind(active)
        .bind(allow_reopen_without_approval)
        .bind(overtime_start_balance_min)
        .bind(id)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Replace all approvers for `user_id` with the provided list (within a tx).
    pub async fn set_approvers_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        approver_ids: &[i64],
    ) -> AppResult<()> {
        sqlx::query("DELETE FROM user_approvers WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        for &aid in approver_ids {
            sqlx::query(
                "INSERT INTO user_approvers(user_id, approver_id) VALUES ($1, $2)",
            )
            .bind(user_id)
            .bind(aid)
            .execute(&mut *tx)
            .await?;
        }
        Ok(())
    }

    /// Insert a single approver relationship (within a tx).
    pub async fn insert_approver_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        approver_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_approvers(user_id, approver_id) VALUES ($1, $2)",
        )
        .bind(user_id)
        .bind(approver_id)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Fetch all active approver IDs for a user from the junction table.
    pub async fn get_approver_ids(&self, user_id: i64) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT ua.approver_id FROM user_approvers ua \
             JOIN users u ON u.id = ua.approver_id \
             WHERE ua.user_id = $1 AND u.active = TRUE \
             AND (u.role = 'team_lead' OR u.role = 'admin')",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Fetch approver details (id, first_name, last_name) for a user.
    pub async fn get_approver_details(
        &self,
        user_id: i64,
    ) -> AppResult<Vec<(i64, String, String)>> {
        Ok(sqlx::query_as::<_, (i64, String, String)>(
            "SELECT u.id, u.first_name, u.last_name FROM user_approvers ua \
             JOIN users u ON u.id = ua.approver_id \
             WHERE ua.user_id = $1 AND u.active = TRUE \
             AND (u.role = 'team_lead' OR u.role = 'admin') \
             ORDER BY u.last_name, u.first_name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn deactivate_tx(tx: &mut sqlx::PgConnection, id: i64) -> AppResult<()> {
        sqlx::query("UPDATE users SET active=FALSE WHERE id=$1")
            .bind(id)
            .execute(tx)
            .await?;
        Ok(())
    }

    pub async fn update_dark_mode(&self, id: i64, dark_mode: bool) -> AppResult<()> {
        sqlx::query("UPDATE users SET dark_mode=$1 WHERE id=$2")
            .bind(dark_mode)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_reopen_policy(
        &self,
        id: i64,
        allow_reopen_without_approval: bool,
    ) -> AppResult<()> {
        sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
            .bind(allow_reopen_without_approval)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_password(
        tx: &mut sqlx::PgConnection,
        id: i64,
        hash: &str,
        must_change_password: bool,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE users SET password_hash=$1, must_change_password=$2 WHERE id=$3",
        )
        .bind(hash)
        .bind(must_change_password)
        .bind(id)
        .execute(tx)
        .await?;
        Ok(())
    }

    pub async fn update_password_self(
        &self,
        id: i64,
        hash: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE users SET password_hash=$1, must_change_password=FALSE WHERE id=$2",
        )
        .bind(hash)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_password_hash(&self, id: i64) -> AppResult<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn count_active_admins_tx(tx: &mut sqlx::PgConnection) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE active=TRUE AND role='admin'",
        )
        .fetch_one(tx)
        .await?)
    }

    // ── Annual leave ───────────────────────────────────────────────────────

    pub async fn get_leave_days(&self, user_id: i64, year: i32) -> AppResult<i64> {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT days FROM user_annual_leave WHERE user_id=$1 AND year=$2",
        )
        .bind(user_id)
        .bind(year)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(days) = existing {
            return Ok(days);
        }
        let default_days: i64 = self.get_default_leave_days().await?;
        sqlx::query(
            "INSERT INTO user_annual_leave(user_id, year, days) \
             VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(year)
        .bind(default_days)
        .execute(&self.pool)
        .await?;
        Ok(default_days)
    }

    pub async fn set_leave_days(
        &self,
        user_id: i64,
        year: i32,
        days: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_annual_leave(user_id, year, days) VALUES ($1,$2,$3) \
             ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days",
        )
        .bind(user_id)
        .bind(year)
        .bind(days)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_leave_days_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        year: i32,
        days: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_annual_leave(user_id, year, days) VALUES ($1,$2,$3) \
             ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days",
        )
        .bind(user_id)
        .bind(year)
        .bind(days)
        .execute(tx)
        .await?;
        Ok(())
    }

    pub async fn get_default_leave_days(&self) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COALESCE(value::BIGINT, 30) \
             FROM app_settings WHERE key='default_annual_leave_days'",
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(30))
    }

    pub async fn get_default_leave_days_tx(tx: &mut sqlx::PgConnection) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COALESCE(value::BIGINT, 30) \
             FROM app_settings WHERE key='default_annual_leave_days'",
        )
        .fetch_optional(tx)
        .await?
        .unwrap_or(30))
    }

    // ── Submission reminder helper ─────────────────────────────────────────

    pub async fn get_active_users_with_hours(&self) -> AppResult<Vec<(i64, String, NaiveDate)>> {
        Ok(sqlx::query_as::<_, (i64, String, NaiveDate)>(
            "SELECT id, email, start_date FROM users \
             WHERE active = TRUE AND weekly_hours > 0",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Begin a transaction.
    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }
}
