use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::roles::{can_approve_non_admin_subjects, is_admin_role, ROLE_ASSISTANT};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};

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
    /// Optional employment start date that anchors annual-leave proration.
    /// Falls back to `start_date` when `None`.
    pub hire_date: Option<NaiveDate>,
    pub active: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    pub allow_reopen_without_approval: bool,
    /// When TRUE, this user's submitted weeks are auto-approved (draft ->
    /// approved directly, skipping the 'submitted' stop). No one is notified
    /// and no emails are sent for the auto-approval.
    pub allow_submission_without_approval: bool,
    pub dark_mode: bool,
    pub overtime_start_balance_min: i64,
    /// When FALSE (admin only), this user has no time/absence tracking.
    /// All related endpoints are blocked; navigation items are hidden.
    pub tracks_time: bool,
    /// Base annual leave entitlement (days/year), used whenever no explicit
    /// `user_annual_leave` override exists for a given year.
    pub annual_leave_days: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        is_admin_role(&self.role)
    }
    pub fn is_lead(&self) -> bool {
        can_approve_non_admin_subjects(&self.role, self.active)
    }
}

const USER_SELECT: &str =
    "SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, workdays_per_week, \
     start_date, hire_date, active, must_change_password, created_at, \
     allow_reopen_without_approval, allow_submission_without_approval, dark_mode, \
     overtime_start_balance_min, tracks_time, annual_leave_days \
     FROM users";

/// Team settings row (id, email, first_name, last_name, role,
/// allow_reopen_without_approval, allow_submission_without_approval).
pub type TeamSettingsRow = (i64, String, String, String, String, bool, bool);

/// Lightweight user record returned by the submission-reminder query.
pub struct ActiveUserRow {
    pub id: i64,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub start_date: NaiveDate,
    pub workdays_per_week: i16,
}

/// Approval reminder row (approver_id, approver_email, first_name, last_name, total_pending_count).
pub type PendingApproverReminderRow = (i64, String, String, String, i64);

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
        Ok(
            QueryBuilder::<Postgres>::new(format!("{USER_SELECT} WHERE email = $1"))
                .build_query_as::<User>()
                .bind(email)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn find_by_id(&self, id: i64) -> AppResult<Option<User>> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{USER_SELECT} WHERE id=$1"))
                .build_query_as::<User>()
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn find_by_id_active(&self, id: i64) -> AppResult<Option<User>> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{USER_SELECT} WHERE id=$1 AND active=TRUE"))
                .build_query_as::<User>()
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn find_all_ordered(&self) -> AppResult<Vec<User>> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{USER_SELECT} ORDER BY last_name, first_name"))
                .build_query_as::<User>()
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn find_for_approver(&self, approver_id: i64) -> AppResult<Vec<User>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{USER_SELECT} WHERE active=TRUE AND (id=$1 \
             OR id IN (SELECT ua.user_id FROM user_approvers ua \
                       JOIN users u ON u.id=ua.user_id \
                       WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin')) \
             ORDER BY last_name, first_name"
        ))
        .build_query_as::<User>()
        .bind(approver_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Like [`find_for_approver`], but includes inactive direct reports too.
    /// Used by the scoped team-lead "assistant management" feature, where a
    /// lead must be able to see (and reactivate) an assistant they previously
    /// deactivated — unlike every other lead-facing view, which intentionally
    /// only shows active team members.
    pub async fn find_for_approver_including_inactive(
        &self,
        approver_id: i64,
    ) -> AppResult<Vec<User>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{USER_SELECT} WHERE id=$1 \
             OR id IN (SELECT ua.user_id FROM user_approvers ua \
                       JOIN users u ON u.id=ua.user_id \
                       WHERE ua.approver_id=$1 AND u.role != 'admin') \
             ORDER BY last_name, first_name"
        ))
        .build_query_as::<User>()
        .bind(approver_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn find_all_active_ordered(&self) -> AppResult<Vec<User>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{USER_SELECT} WHERE active=TRUE ORDER BY last_name"
        ))
        .build_query_as::<User>()
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn find_active_team_for_lead(&self, lead_id: i64) -> AppResult<Vec<User>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{USER_SELECT} WHERE active=TRUE \
             AND (id=$1 OR id IN (SELECT ua.user_id FROM user_approvers ua \
                                  JOIN users u ON u.id=ua.user_id \
                                  WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin')) \
             ORDER BY last_name"
        ))
        .build_query_as::<User>()
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
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE active=TRUE AND lower(trim(role))='admin'",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn count_admin_direct_reports(&self, user_id: i64) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_approvers \
             WHERE approver_id=$1 \
             AND user_id IN (SELECT id FROM users WHERE active=TRUE AND lower(trim(role))='admin')",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn count_direct_reports(&self, user_id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar("SELECT COUNT(*) FROM user_approvers WHERE approver_id=$1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn count_active_direct_reports(&self, user_id: i64) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_approvers \
                 WHERE approver_id=$1 \
                 AND user_id IN (SELECT id FROM users WHERE active=TRUE)",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn count_active_direct_reports_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_approvers \
             WHERE approver_id=$1 \
             AND user_id IN (SELECT id FROM users WHERE active=TRUE)",
        )
        .bind(user_id)
        .fetch_one(tx)
        .await?)
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
        Ok(sqlx::query_as::<_, (i64, String, bool)>(
            "SELECT id, role, active FROM users WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Check whether `target_id` is a non-admin direct report of `approver_id`.
    pub async fn is_direct_report(&self, target_id: i64, approver_id: i64) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, Option<bool>>(
            "SELECT TRUE FROM user_approvers ua \
             WHERE ua.user_id=$1 AND ua.approver_id=$2 \
             AND EXISTS (SELECT 1 FROM users u WHERE u.id=$1 AND u.active=TRUE AND u.role != 'admin')",
        )
        .bind(target_id)
        .bind(approver_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten()
        .is_some())
    }

    pub async fn earliest_active_start_date(&self) -> AppResult<Option<NaiveDate>> {
        Ok(
            sqlx::query_scalar("SELECT MIN(start_date) FROM users WHERE active = true")
                .fetch_one(&self.pool)
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
            return Err(AppError::conflict("Email already exists."));
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
            return Err(AppError::conflict(
                "First name and last name already exist.",
            ));
        }
        Ok(())
    }

    // ── Team settings ──────────────────────────────────────────────────────

    pub async fn team_settings_all(&self) -> AppResult<Vec<TeamSettingsRow>> {
        // Pure-admin users (tracks_time=false) have no time entries of their own
        // and so the reopen-policy flag never applies to them; exclude them so
        // the team settings page doesn't show meaningless rows.
        Ok(sqlx::query_as::<_, TeamSettingsRow>(
            "SELECT id, email, first_name, last_name, role, \
             allow_reopen_without_approval, allow_submission_without_approval FROM users \
             WHERE active=TRUE AND tracks_time=TRUE ORDER BY last_name, first_name",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn team_settings_for_lead(&self, lead_id: i64) -> AppResult<Vec<TeamSettingsRow>> {
        Ok(sqlx::query_as::<_, TeamSettingsRow>(
            "SELECT id, email, first_name, last_name, role, \
             allow_reopen_without_approval, allow_submission_without_approval FROM users \
             WHERE active=TRUE AND tracks_time=TRUE \
             AND (id=$1 OR id IN (SELECT ua.user_id FROM user_approvers ua \
                                  JOIN users u ON u.id=ua.user_id \
                                  WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin')) \
             ORDER BY last_name, first_name",
        )
        .bind(lead_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Query active approvers who currently have pending review items.
    pub async fn pending_approvers_for_reminders(
        &self,
    ) -> AppResult<Vec<PendingApproverReminderRow>> {
        Ok(sqlx::query_as::<_, PendingApproverReminderRow>(
            "WITH user_pending AS (
                 SELECT user_id, COUNT(*)::bigint AS pending_count
                 FROM (
                     SELECT user_id FROM time_entries
                     WHERE status = 'submitted'
                     UNION ALL
                     SELECT user_id FROM absences           WHERE status IN ('requested','cancellation_pending')
                     UNION ALL
                     SELECT user_id FROM reopen_requests    WHERE status = 'pending'
                 ) all_pending
                 GROUP BY user_id
             ),
             via_assignment AS (
                 SELECT ua.approver_id, SUM(up.pending_count)::bigint AS pending_count
                 FROM user_approvers ua
                 JOIN user_pending up ON up.user_id = ua.user_id
                 JOIN users subject   ON subject.id = ua.user_id
                 JOIN users approver  ON approver.id = ua.approver_id
                                     AND approver.active = TRUE
                 WHERE (
                     (subject.role = 'admin' AND approver.role = 'admin') OR
                     (subject.role != 'admin' AND approver.role IN ('team_lead', 'admin'))
                 )
                 GROUP BY ua.approver_id
             ),
             combined AS (
                 SELECT approver_id, pending_count FROM via_assignment
             )
             SELECT c.approver_id, u.email, u.first_name, u.last_name, SUM(c.pending_count)::bigint AS total_pending
             FROM combined c
             JOIN users u ON u.id = c.approver_id AND u.active = TRUE
             GROUP BY c.approver_id, u.email, u.first_name, u.last_name
             HAVING SUM(c.pending_count) > 0",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn is_active_direct_report(
        &self,
        target_id: i64,
        approver_id: i64,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, Option<bool>>(
            "SELECT TRUE FROM user_approvers ua \
                 JOIN users u ON u.id = ua.user_id \
                 WHERE ua.user_id=$1 AND ua.approver_id=$2 \
                 AND u.active=TRUE AND u.role != 'admin'",
        )
        .bind(target_id)
        .bind(approver_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten()
        .is_some())
    }

    pub async fn update_allow_reopen(&self, target_id: i64, allow: bool) -> AppResult<()> {
        sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
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

    pub async fn fetch_for_update(tx: &mut sqlx::PgConnection, id: i64) -> AppResult<User> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{USER_SELECT} WHERE id=$1 FOR UPDATE"))
                .build_query_as::<User>()
                .bind(id)
                .fetch_one(tx)
                .await?,
        )
    }

    pub async fn create_initial_admin(
        tx: &mut sqlx::PgConnection,
        email: &str,
        password_hash: &str,
        first_name: &str,
        last_name: &str,
        start_date: NaiveDate,
        tracks_time: bool,
    ) -> AppResult<i64> {
        sqlx::query(
            "INSERT INTO users(email, password_hash, first_name, last_name, role, \
               weekly_hours, workdays_per_week, start_date, hire_date, must_change_password, \
               overtime_start_balance_min, tracks_time) \
               VALUES ($1, $2, $3, $4, 'admin', 39.0, 5, $5, NULL, FALSE, 0, $6)",
        )
        .bind(email)
        .bind(password_hash)
        .bind(first_name)
        .bind(last_name)
        .bind(start_date)
        .bind(tracks_time)
        .execute(&mut *tx)
        .await?;
        let id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
            .bind(email)
            .fetch_one(&mut *tx)
            .await?;
        // The initial admin is created outside the regular `create()` path
        // (it bootstraps the system before any user exists), so it needs the
        // same default-enable grant for whatever categories were already
        // seeded at startup.
        sqlx::query(
            "INSERT INTO user_category_access (user_id, category_id) SELECT $1, id FROM categories",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO user_absence_category_access (user_id, category_id) SELECT $1, id FROM absence_categories",
        )
        .bind(id)
        .execute(&mut *tx)
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
    ///
    /// `category_ids`/`absence_category_ids` of `None` default to every
    /// existing category (mirroring how a newly created category defaults to
    /// enabled for every employee); `Some(ids)` grants exactly that list
    /// (which may be empty) instead. Callers are expected to have already
    /// validated that every id refers to a real category.
    #[allow(clippy::too_many_arguments)]
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
        hire_date: Option<NaiveDate>,
        must_change_password: bool,
        overtime_start_balance_min: i64,
        tracks_time: bool,
        category_ids: Option<&[i64]>,
        absence_category_ids: Option<&[i64]>,
        annual_leave_days: i64,
    ) -> Result<i64, sqlx::Error> {
        let new_user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users(email, password_hash, first_name, last_name, role, \
             weekly_hours, workdays_per_week, start_date, hire_date, must_change_password, \
             overtime_start_balance_min, tracks_time, annual_leave_days) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id",
        )
        .bind(email)
        .bind(password_hash)
        .bind(first_name)
        .bind(last_name)
        .bind(role)
        .bind(weekly_hours)
        .bind(workdays_per_week)
        .bind(start_date)
        .bind(hire_date)
        .bind(must_change_password)
        .bind(overtime_start_balance_min)
        .bind(tracks_time)
        .bind(annual_leave_days)
        .fetch_one(&mut *tx)
        .await?;
        match category_ids {
            None => {
                sqlx::query(
                    "INSERT INTO user_category_access (user_id, category_id) SELECT $1, id FROM categories",
                )
                .bind(new_user_id)
                .execute(&mut *tx)
                .await?;
            }
            Some(ids) => {
                let unique_ids: std::collections::HashSet<&i64> = ids.iter().collect();
                for category_id in unique_ids {
                    sqlx::query(
                        "INSERT INTO user_category_access (user_id, category_id) VALUES ($1, $2)",
                    )
                    .bind(new_user_id)
                    .bind(category_id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        match absence_category_ids {
            None => {
                sqlx::query(
                    "INSERT INTO user_absence_category_access (user_id, category_id) SELECT $1, id FROM absence_categories",
                )
                .bind(new_user_id)
                .execute(&mut *tx)
                .await?;
            }
            Some(ids) => {
                let unique_ids: std::collections::HashSet<&i64> = ids.iter().collect();
                for category_id in unique_ids {
                    sqlx::query(
                        "INSERT INTO user_absence_category_access (user_id, category_id) VALUES ($1, $2)",
                    )
                    .bind(new_user_id)
                    .bind(category_id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        Ok(new_user_id)
    }

    #[allow(clippy::too_many_arguments)]
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
        hire_date: Option<Option<NaiveDate>>,
        active: Option<bool>,
        allow_reopen_without_approval: Option<bool>,
        allow_submission_without_approval: Option<bool>,
        overtime_start_balance_min: Option<i64>,
        tracks_time: Option<bool>,
        annual_leave_days: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        // hire_date is nullable, so a plain COALESCE cannot express "clear it
        // back to NULL". Use an explicit flag + CASE, mirroring
        // CategoryDb::update's handling of the nullable `description` column.
        let update_hire_date = hire_date.is_some();
        let hire_date = hire_date.flatten();
        sqlx::query(
            "UPDATE users \
             SET email=COALESCE($1,email), \
                 first_name=COALESCE($2,first_name), \
                 last_name=COALESCE($3,last_name), \
                 role=COALESCE($4,role), \
                 weekly_hours=COALESCE($5,weekly_hours), \
                 workdays_per_week=COALESCE($6,workdays_per_week), \
                 start_date=COALESCE($7,start_date), \
                 hire_date=CASE WHEN $8 THEN $9 ELSE hire_date END, \
                 active=COALESCE($10,active), \
                 allow_reopen_without_approval=COALESCE($11,allow_reopen_without_approval), \
                 overtime_start_balance_min=COALESCE($12,overtime_start_balance_min), \
                 tracks_time=COALESCE($13,tracks_time), \
                 allow_submission_without_approval=COALESCE($15,allow_submission_without_approval), \
                 annual_leave_days=COALESCE($16,annual_leave_days) \
             WHERE id=$14",
        )
        .bind(email)
        .bind(first_name)
        .bind(last_name)
        .bind(role)
        .bind(weekly_hours)
        .bind(workdays_per_week)
        .bind(start_date)
        .bind(update_hire_date)
        .bind(hire_date)
        .bind(active)
        .bind(allow_reopen_without_approval)
        .bind(overtime_start_balance_min)
        .bind(tracks_time)
        .bind(id)
        .bind(allow_submission_without_approval)
        .bind(annual_leave_days)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Delete all time entries, absences, and reopen requests for a user within
    /// an existing transaction. Used when an admin disables their own time
    /// tracking — all historical data is purged atomically.
    pub async fn delete_time_data_for_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<()> {
        sqlx::query("DELETE FROM reopen_requests WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM absences WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM time_entries WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *tx)
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
            Self::insert_approver_tx(&mut *tx, user_id, aid).await?;
        }
        Ok(())
    }

    /// Insert a single approver relationship (within a tx).
    pub async fn insert_approver_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        approver_id: i64,
    ) -> AppResult<()> {
        let (subject_role, _) =
            sqlx::query_as::<_, (String, bool)>("SELECT role, active FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(AppError::NotFound)?;
        let requires_admin_approver = is_admin_role(&subject_role);
        let rows = sqlx::query(
            "INSERT INTO user_approvers(user_id, approver_id) \
             SELECT $1, $2 \
             WHERE EXISTS ( \
                SELECT 1 FROM users approver \
                WHERE approver.id = $2 \
                AND ( \
                    ($3::bool = TRUE AND approver.active = TRUE AND approver.role = 'admin') OR \
                    ($3::bool = FALSE AND approver.active = TRUE AND approver.role IN ('team_lead', 'admin')) \
                ) \
             )",
        )
        .bind(user_id)
        .bind(approver_id)
        .bind(requires_admin_approver)
        .execute(tx)
        .await?;
        if rows.rows_affected() == 0 {
            return Err(AppError::bad_request(
                "Approver must be an active Team lead or Admin (admins may only report to active admins).",
            ));
        }
        Ok(())
    }

    /// Fetch all active approver IDs for a user from the junction table.
    pub async fn get_approver_ids(&self, user_id: i64) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT ua.approver_id FROM user_approvers ua \
             JOIN users approver ON approver.id = ua.approver_id \
             JOIN users subject ON subject.id = ua.user_id \
             WHERE ua.user_id = $1 AND approver.active = TRUE \
             AND ( \
                 (lower(trim(subject.role)) = 'admin' AND approver.role = 'admin') OR \
                 (lower(trim(subject.role)) != 'admin' AND approver.role IN ('team_lead', 'admin')) \
             )",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Fetch active approver IDs for a user within an existing transaction.
    pub async fn get_approver_ids_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT approver_id FROM user_approvers WHERE user_id=$1 ORDER BY approver_id",
        )
        .bind(user_id)
        .fetch_all(tx)
        .await?)
    }

    /// Fetch approver details (id, first_name, last_name) for a user.
    pub async fn get_approver_details(
        &self,
        user_id: i64,
    ) -> AppResult<Vec<(i64, String, String)>> {
        Ok(sqlx::query_as::<_, (i64, String, String)>(
            "SELECT approver.id, approver.first_name, approver.last_name \
             FROM user_approvers ua \
             JOIN users approver ON approver.id = ua.approver_id \
             JOIN users subject ON subject.id = ua.user_id \
             WHERE ua.user_id = $1 AND approver.active = TRUE \
             AND ( \
                 (lower(trim(subject.role)) = 'admin' AND approver.role = 'admin') OR \
                 (lower(trim(subject.role)) != 'admin' AND approver.role IN ('team_lead', 'admin')) \
             ) \
             ORDER BY approver.last_name, approver.first_name",
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

    pub async fn delete_tx(tx: &mut sqlx::PgConnection, id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM users WHERE id=$1")
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
        let result = sqlx::query("UPDATE users SET allow_reopen_without_approval=$1 WHERE id=$2")
            .bind(allow_reopen_without_approval)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn update_password(
        tx: &mut sqlx::PgConnection,
        id: i64,
        hash: &str,
        must_change_password: bool,
    ) -> AppResult<()> {
        let result =
            sqlx::query("UPDATE users SET password_hash=$1, must_change_password=$2 WHERE id=$3")
                .bind(hash)
                .bind(must_change_password)
                .bind(id)
                .execute(tx)
                .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn update_password_self(&self, id: i64, hash: &str) -> AppResult<()> {
        sqlx::query("UPDATE users SET password_hash=$1, must_change_password=FALSE WHERE id=$2")
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
            "SELECT COUNT(*) FROM users WHERE active=TRUE AND lower(trim(role))='admin'",
        )
        .fetch_one(tx)
        .await?)
    }

    // ── Annual leave ───────────────────────────────────────────────────────

    /// Annual leave entitlement for `user_id` in `year`: an explicit
    /// `user_annual_leave` override for that year takes precedence; otherwise
    /// the user's own base `annual_leave_days` is used.
    pub async fn get_leave_days(&self, user_id: i64, year: i32) -> AppResult<i64> {
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT days FROM user_annual_leave WHERE user_id=$1 AND year=$2")
                .bind(user_id)
                .bind(year)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(days) = existing {
            return Ok(days);
        }
        Ok(
            sqlx::query_scalar("SELECT annual_leave_days FROM users WHERE id=$1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn set_leave_days(&self, user_id: i64, year: i32, days: i64) -> AppResult<()> {
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

    pub async fn annual_days_or_default(
        &self,
        user_id: i64,
        year: i32,
        default_days: i64,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT days FROM user_annual_leave WHERE user_id=$1 AND year=$2",
        )
        .bind(user_id)
        .bind(year)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(default_days))
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

    pub async fn get_active_non_assistant_users(&self) -> AppResult<Vec<ActiveUserRow>> {
        let rows = sqlx::query_as::<_, (i64, String, String, String, NaiveDate, i16)>(
            "SELECT id, email, first_name, last_name, start_date, workdays_per_week FROM users \
             WHERE active = TRUE AND lower(trim(role)) != $1 AND weekly_hours > 0 \
             AND tracks_time = TRUE",
        )
        .bind(ROLE_ASSISTANT)
        .fetch_all(&self.pool)
        .await?;
        tracing::debug!(
            target: "zerf::assistant_role",
            selected_user_count = rows.len(),
            "loaded active non-assistant users with weekly_hours > 0 for submission reminders"
        );
        Ok(rows
            .into_iter()
            .map(
                |(id, email, first_name, last_name, start_date, workdays_per_week)| ActiveUserRow {
                    id,
                    email,
                    first_name,
                    last_name,
                    start_date,
                    workdays_per_week,
                },
            )
            .collect())
    }

    /// Begin a transaction.
    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }
}
