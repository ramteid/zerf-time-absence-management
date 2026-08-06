use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::roles::{can_approve_non_admin_subjects, is_admin_role, ROLE_ASSISTANT};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};

/// Lightweight archived-user row returned by GET /users/archived.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ArchivedUser {
    pub id: i64,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub role: String,
    pub archived_at: DateTime<Utc>,
}

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
    /// Set when the user is archived. Archived users cannot log in and are
    /// excluded from active user lists. Restore clears this field.
    pub archived_at: Option<DateTime<Utc>>,
    /// When TRUE (admin only), this user receives in-app + email notifications
    /// for technical system errors. Default FALSE; forced FALSE for non-admins.
    pub receives_error_notifications: bool,
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
     overtime_start_balance_min, tracks_time, archived_at, \
     receives_error_notifications \
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

/// Approval reminder row (approver_id, total_pending_count).
pub type PendingApproverReminderRow = (i64, i64);

/// Values submitted when creating or updating one user's leave account.
/// The service supplies the current application year; the repository writes
/// the two yearly values as explicit overrides in the caller's transaction.
#[derive(Clone, Debug)]
pub struct UserLeaveAccountInput {
    pub category_id: i64,
    pub base_days: i64,
    pub current_year_days: i64,
    pub next_year_days: i64,
}

/// Category-owned account metadata used when a user form needs account
/// definitions but not another user's individual entitlements.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct LeaveAccountDefinition {
    pub category_id: i64,
    pub category_name: String,
    pub color: String,
    pub active: bool,
    pub default_days: i64,
    pub carryover_expiry: String,
    pub start_year: i32,
}

/// One user's values for a leave-account category, including the effective
/// current and next-year entitlement after yearly overrides are applied.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct UserLeaveAccountDetails {
    pub category_id: i64,
    pub category_name: String,
    pub color: String,
    pub active: bool,
    pub base_days: i64,
    pub current_year: i32,
    pub current_year_days: i64,
    pub next_year: i32,
    pub next_year_days: i64,
    pub carryover_expiry: String,
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

    /// Returns all non-archived users (including active and inactive) ordered by name.
    /// This is the default admin user list view. Archived users are excluded —
    /// use `find_all_including_archived` or `find_archived_ordered` for those.
    pub async fn find_all_ordered(&self) -> AppResult<Vec<User>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{USER_SELECT} WHERE archived_at IS NULL ORDER BY last_name, first_name"
        ))
        .build_query_as::<User>()
        .fetch_all(&self.pool)
        .await?)
    }

    /// Returns all users including archived ones, ordered by name.
    /// Used only where the admin explicitly requests the full user list.
    pub async fn find_all_including_archived(&self) -> AppResult<Vec<User>> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{USER_SELECT} ORDER BY last_name, first_name"))
                .build_query_as::<User>()
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Returns archived users ordered by archived_at DESC.
    pub async fn find_archived_ordered(&self) -> AppResult<Vec<ArchivedUser>> {
        Ok(sqlx::query_as::<_, ArchivedUser>(
            "SELECT id, email, first_name, last_name, role, archived_at \
             FROM users WHERE archived_at IS NOT NULL ORDER BY archived_at DESC",
        )
        .fetch_all(&self.pool)
        .await?)
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

    /// Like [`find_for_approver`], but includes inactive (and archived) direct
    /// reports too. Used by the scoped team-lead "assistant management" feature
    /// so a lead can see all their assistants — including archived ones — and
    /// restore them via the archive/restore endpoints. The lead's own row is
    /// also included (unarchived, since an archived user cannot log in and would
    /// never reach this endpoint).
    pub async fn find_for_approver_including_inactive(
        &self,
        approver_id: i64,
    ) -> AppResult<Vec<User>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{USER_SELECT} WHERE (id=$1 AND archived_at IS NULL) \
             OR id IN (SELECT ua.user_id FROM user_approvers ua \
                       JOIN users u ON u.id = ua.user_id \
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
             AND EXISTS (SELECT 1 FROM users u WHERE u.id=$1 AND u.active=TRUE AND lower(trim(u.role)) != 'admin')",
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
             AND id<>$1 \
             AND lower(trim(role)) != 'admin' \
             AND id IN (SELECT ua.user_id FROM user_approvers ua \
                        WHERE ua.approver_id=$1) \
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
                     -- One submitted week (however many daily rows it has) counts
                     -- as a single pending item, matching how approvers actually
                     -- review and how the submission notification groups them.
                     SELECT te.user_id FROM time_entries te
                     JOIN users u ON u.id = te.user_id AND u.tracks_time = TRUE AND u.active = TRUE
                     WHERE te.status = 'submitted'
                     GROUP BY te.user_id, date_trunc('week', te.entry_date)
                     UNION ALL
                     SELECT a.user_id FROM absences a
                     JOIN users u ON u.id = a.user_id AND u.tracks_time = TRUE AND u.active = TRUE
                     WHERE a.status IN ('requested','cancellation_pending')
                     UNION ALL
                     SELECT rr.user_id FROM reopen_requests rr
                     JOIN users u ON u.id = rr.user_id AND u.tracks_time = TRUE AND u.active = TRUE
                     WHERE rr.status = 'pending'
                     AND NOT EXISTS (
                         SELECT 1 FROM time_entries te
                         WHERE te.user_id = rr.user_id
                         AND te.entry_date BETWEEN rr.week_start AND rr.week_start + 6
                         AND te.status = 'submitted'
                     )
                 ) all_pending
                 GROUP BY user_id
             ),
             via_assignment AS (
                 SELECT ua.approver_id, SUM(up.pending_count)::bigint AS pending_count
                 FROM user_approvers ua
                 JOIN user_pending up ON up.user_id = ua.user_id
                 JOIN users subject   ON subject.id = ua.user_id
                                     AND subject.active = TRUE
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
             SELECT c.approver_id, SUM(c.pending_count)::bigint AS total_pending
             FROM combined c
             JOIN users u ON u.id = c.approver_id AND u.active = TRUE
             GROUP BY c.approver_id
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
    ) -> Result<i64, sqlx::Error> {
        let new_user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users(email, password_hash, first_name, last_name, role, \
             weekly_hours, workdays_per_week, start_date, hire_date, must_change_password, \
             overtime_start_balance_min, tracks_time) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id",
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
        Self::seed_leave_accounts_for_user_sql_tx(tx, new_user_id, role, absence_category_ids)
            .await?;
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
        allow_reopen_without_approval: Option<bool>,
        allow_submission_without_approval: Option<bool>,
        overtime_start_balance_min: Option<i64>,
        tracks_time: Option<bool>,
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
                 allow_reopen_without_approval=COALESCE($10,allow_reopen_without_approval), \
                 overtime_start_balance_min=COALESCE($11,overtime_start_balance_min), \
                 tracks_time=COALESCE($12,tracks_time), \
                 allow_submission_without_approval=COALESCE($14,allow_submission_without_approval) \
             WHERE id=$13",
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
        .bind(allow_reopen_without_approval)
        .bind(overtime_start_balance_min)
        .bind(tracks_time)
        .bind(id)
        .bind(allow_submission_without_approval)
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
    /// Fetch all approver relationships in a single query, returning a map of
    /// `user_id -> [approver_id, ...]`. Used by the admin user list to avoid N+1
    /// queries when building the full user list with approver_ids.
    pub async fn get_all_approver_ids(
        &self,
    ) -> AppResult<std::collections::HashMap<i64, Vec<i64>>> {
        let rows = sqlx::query_as::<_, (i64, i64)>(
            "SELECT ua.user_id, ua.approver_id FROM user_approvers ua",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut map: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
        for (user_id, approver_id) in rows {
            map.entry(user_id).or_default().push(approver_id);
        }
        Ok(map)
    }

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

    /// Find active dependents (users for whom `approver_id` is an approver)
    /// within a transaction, for use in the archive flow.
    /// Returns (user_id, first_name, last_name, role) tuples.
    pub async fn find_active_dependents_tx(
        tx: &mut sqlx::PgConnection,
        approver_id: i64,
    ) -> AppResult<Vec<(i64, String, String, String)>> {
        Ok(sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT u.id, u.first_name, u.last_name, u.role \
             FROM user_approvers ua \
             JOIN users u ON u.id = ua.user_id \
             WHERE ua.approver_id=$1 AND u.active=TRUE AND u.archived_at IS NULL",
        )
        .bind(approver_id)
        .fetch_all(tx)
        .await?)
    }

    /// Check if a given user is a valid active approver for a subject whose
    /// `requires_admin_approver` flag is known. Returns true if valid.
    pub async fn is_valid_replacement_approver_tx(
        tx: &mut sqlx::PgConnection,
        approver_id: i64,
        requires_admin_approver: bool,
    ) -> AppResult<bool> {
        let valid: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id=$1 AND active=TRUE AND archived_at IS NULL \
             AND (($2::bool = TRUE AND role='admin') OR \
                  ($2::bool = FALSE AND role IN ('team_lead','admin')))",
        )
        .bind(approver_id)
        .bind(requires_admin_approver)
        .fetch_optional(tx)
        .await?;
        Ok(valid.is_some())
    }

    /// Reassign a dependent user from `old_approver_id` to `new_approver_id`
    /// within a transaction. Removes the old relationship and adds the new one
    /// (ignoring duplicates via ON CONFLICT DO NOTHING).
    pub async fn reassign_approver_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        old_approver_id: i64,
        new_approver_id: i64,
    ) -> AppResult<()> {
        sqlx::query("DELETE FROM user_approvers WHERE user_id=$1 AND approver_id=$2")
            .bind(user_id)
            .bind(old_approver_id)
            .execute(&mut *tx) // explicit deref needed to reborrow before second execute
            .await?;
        sqlx::query(
            "INSERT INTO user_approvers(user_id, approver_id) VALUES ($1,$2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(new_approver_id)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Archive a user: set active=FALSE and archived_at=NOW().
    /// Archived users cannot log in and are excluded from active user lists.
    pub async fn archive_tx(tx: &mut sqlx::PgConnection, id: i64) -> AppResult<()> {
        sqlx::query("UPDATE users SET active=FALSE, archived_at=NOW() WHERE id=$1")
            .bind(id)
            .execute(tx)
            .await?;
        Ok(())
    }

    /// Restore an archived user: set active=TRUE, archived_at=NULL.
    /// Optionally reset start_date when provided (avoids flextime gap accumulation).
    pub async fn restore_tx(
        tx: &mut sqlx::PgConnection,
        id: i64,
        new_start_date: Option<NaiveDate>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE users SET active=TRUE, archived_at=NULL, \
             start_date=COALESCE($2, start_date), must_change_password=TRUE \
             WHERE id=$1",
        )
        .bind(id)
        .bind(new_start_date)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Check whether a user has any time entries or absences.
    /// Used to guard hard delete: users with historical data must be archived,
    /// not hard-deleted.
    pub async fn has_time_data_tx(tx: &mut sqlx::PgConnection, user_id: i64) -> AppResult<bool> {
        let has_entries: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM time_entries WHERE user_id=$1) \
             OR EXISTS(SELECT 1 FROM absences WHERE user_id=$1)",
        )
        .bind(user_id)
        .fetch_one(tx)
        .await?;
        Ok(has_entries)
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

    /// Set the admin-only "receives technical error notifications" flag. Callers
    /// are responsible for forcing `false` when the user is not an admin.
    pub async fn set_receives_error_notifications_tx(
        tx: &mut sqlx::PgConnection,
        id: i64,
        enabled: bool,
    ) -> AppResult<()> {
        sqlx::query("UPDATE users SET receives_error_notifications=$1 WHERE id=$2")
            .bind(enabled)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        Ok(())
    }

    /// Ids of active admins who opted in to technical error notifications.
    /// Used by the error-notification worker to fan a queued error out to the
    /// right recipients (the facade resolves each address itself).
    pub async fn error_notification_recipient_ids(&self) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT id FROM users \
             WHERE active=TRUE AND lower(trim(role))='admin' \
             AND receives_error_notifications=TRUE \
             ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?)
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

    // Leave accounts.

    /// Return every category that owns a leave account. This is the account
    /// definition source for administrators and for balance calculations; it
    /// intentionally includes inactive categories so historical reports keep
    /// their account metadata.
    pub async fn list_leave_account_definitions(&self) -> AppResult<Vec<LeaveAccountDefinition>> {
        Ok(sqlx::query_as::<_, LeaveAccountDefinition>(
            "SELECT
                category.id AS category_id,
                category.name AS category_name,
                category.color,
                category.active,
                category.leave_account_default_days AS default_days,
                category.leave_account_carryover_expiry AS carryover_expiry,
                category.leave_account_start_year AS start_year
             FROM absence_categories AS category
             WHERE category.cost_type = 'vacation'
             ORDER BY category.active DESC, category.sort_order, category.name, category.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Return account definitions for the accounts that already belong to one
    /// user. Inactive or access-revoked categories remain present because their
    /// accounts and historical balances must stay visible.
    pub async fn leave_account_definitions_for_user(
        &self,
        user_id: i64,
    ) -> AppResult<Vec<LeaveAccountDefinition>> {
        Ok(sqlx::query_as::<_, LeaveAccountDefinition>(
            "SELECT
                category.id AS category_id,
                category.name AS category_name,
                category.color,
                category.active,
                category.leave_account_default_days AS default_days,
                category.leave_account_carryover_expiry AS carryover_expiry,
                category.leave_account_start_year AS start_year
             FROM user_leave_accounts AS account
             JOIN absence_categories AS category ON category.id = account.category_id
             WHERE account.user_id = $1
             ORDER BY category.active DESC, category.sort_order, category.name, category.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Return a user's base and current/next effective leave-account values.
    /// The only yearly override lookup is folded into this query, avoiding one
    /// database round trip per category.
    pub async fn user_leave_accounts_for_years(
        &self,
        user_id: i64,
        current_year: i32,
        next_year: i32,
    ) -> AppResult<Vec<UserLeaveAccountDetails>> {
        Ok(sqlx::query_as::<_, UserLeaveAccountDetails>(
            "SELECT
                account.category_id,
                category.name AS category_name,
                category.color,
                category.active,
                account.base_days,
                $2::INTEGER AS current_year,
                COALESCE(current_override.days, account.base_days) AS current_year_days,
                $3::INTEGER AS next_year,
                COALESCE(next_override.days, account.base_days) AS next_year_days,
                category.leave_account_carryover_expiry AS carryover_expiry
             FROM user_leave_accounts AS account
             JOIN absence_categories AS category ON category.id = account.category_id
             LEFT JOIN user_leave_account_year_overrides AS current_override
               ON current_override.user_id = account.user_id
              AND current_override.category_id = account.category_id
              AND current_override.year = $2
             LEFT JOIN user_leave_account_year_overrides AS next_override
               ON next_override.user_id = account.user_id
              AND next_override.category_id = account.category_id
              AND next_override.year = $3
             WHERE account.user_id = $1
             ORDER BY category.active DESC, category.sort_order, category.name, category.id",
        )
        .bind(user_id)
        .bind(current_year)
        .bind(next_year)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Return the effective entitlement for one user, account and year. A
    /// per-year override wins over the user's stored base value.
    pub async fn effective_leave_account_days(
        &self,
        user_id: i64,
        category_id: i64,
        year: i32,
    ) -> AppResult<i64> {
        let days = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(year_override.days, account.base_days)
             FROM user_leave_accounts AS account
             LEFT JOIN user_leave_account_year_overrides AS year_override
               ON year_override.user_id = account.user_id
              AND year_override.category_id = account.category_id
              AND year_override.year = $3
             WHERE account.user_id = $1 AND account.category_id = $2",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(year)
        .fetch_optional(&self.pool)
        .await?;
        days.ok_or(AppError::NotFound)
    }

    /// Transaction-bound form of [`Self::effective_leave_account_days`].
    pub async fn effective_leave_account_days_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        category_id: i64,
        year: i32,
    ) -> AppResult<i64> {
        let days = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(year_override.days, account.base_days)
             FROM user_leave_accounts AS account
             LEFT JOIN user_leave_account_year_overrides AS year_override
               ON year_override.user_id = account.user_id
              AND year_override.category_id = account.category_id
              AND year_override.year = $3
             WHERE account.user_id = $1 AND account.category_id = $2",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(year)
        .fetch_optional(tx)
        .await?;
        days.ok_or(AppError::NotFound)
    }

    /// Seed all current leave-account categories for a just-created user. The
    /// caller owns the surrounding transaction and user-graph advisory lock.
    /// Existing rows are never overwritten, making the operation safe during
    /// a retry or after a concurrent category was created before the lock.
    pub async fn seed_leave_accounts_for_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        role: &str,
    ) -> AppResult<()> {
        Self::seed_leave_accounts_for_user_sql_tx(tx, user_id, role, None).await?;
        Ok(())
    }

    /// Seed a newly created leave-account category for every existing user.
    /// Assistants receive zero days; all other users receive the category's
    /// current default. The service must call this under the shared user-graph
    /// advisory lock together with category creation and access seeding.
    pub async fn seed_leave_accounts_for_category_tx(
        tx: &mut sqlx::PgConnection,
        category_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_leave_accounts (user_id, category_id, base_days)
             SELECT
                users.id,
                category.id,
                CASE
                    WHEN lower(trim(users.role)) = $2 THEN 0
                    ELSE category.leave_account_default_days
                END
             FROM users
             JOIN absence_categories AS category ON category.id = $1
             WHERE category.cost_type = 'vacation'
             ON CONFLICT (user_id, category_id) DO NOTHING",
        )
        .bind(category_id)
        .bind(ROLE_ASSISTANT)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Zero one user's leave account and delete its yearly overrides. Used
    /// when access to a leave-account category is revoked so the balance
    /// genuinely reflects "no entitlement" rather than a stale base value
    /// kept alive by a leftover override. The account row itself is kept
    /// (never deleted) so historical balances stay computable.
    pub async fn revoke_leave_account_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        category_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE user_leave_accounts SET base_days = 0
             WHERE user_id = $1 AND category_id = $2",
        )
        .bind(user_id)
        .bind(category_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM user_leave_account_year_overrides
             WHERE user_id = $1 AND category_id = $2",
        )
        .bind(user_id)
        .bind(category_id)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// Restore one user's leave account to the category default after access
    /// is granted (assistants still get zero). The current- and next-year
    /// overrides are set to that same default so the account starts from an
    /// unambiguous, fully specified state rather than depending on whatever
    /// override rows happen to exist.
    pub async fn grant_leave_account_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        category_id: i64,
        current_year: i32,
        next_year: i32,
    ) -> AppResult<()> {
        let default_days: i64 = sqlx::query_scalar(
            "INSERT INTO user_leave_accounts (user_id, category_id, base_days)
             SELECT
                users.id,
                category.id,
                CASE
                    WHEN lower(trim(users.role)) = $3 THEN 0
                    ELSE category.leave_account_default_days
                END
             FROM users
             JOIN absence_categories AS category ON category.id = $2
             WHERE users.id = $1
             ON CONFLICT (user_id, category_id) DO UPDATE SET base_days = EXCLUDED.base_days
             RETURNING base_days",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(ROLE_ASSISTANT)
        .fetch_one(&mut *tx)
        .await?;
        Self::set_leave_account_year_days_tx(tx, user_id, category_id, current_year, default_days)
            .await?;
        Self::set_leave_account_year_days_tx(tx, user_id, category_id, next_year, default_days)
            .await?;
        Ok(())
    }

    /// Apply submitted base/current/next-year values atomically. Before
    /// writing explicit values, ensure categories created between form load and
    /// save receive their default account row as well.
    pub async fn apply_leave_account_values_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        current_year: i32,
        values: &[UserLeaveAccountInput],
    ) -> AppResult<()> {
        let next_year = current_year.checked_add(1).ok_or_else(|| {
            AppError::bad_request("Current year cannot be incremented for leave accounts.")
        })?;
        if !(2000..=2100).contains(&current_year) || !(2000..=2100).contains(&next_year) {
            return Err(AppError::bad_request(
                "Leave-account overrides support years from 2000 to 2100.",
            ));
        }

        let user_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await?;
        if !user_exists {
            return Err(AppError::NotFound);
        }

        let mut category_ids = std::collections::HashSet::with_capacity(values.len());
        for value in values {
            if !category_ids.insert(value.category_id) {
                return Err(AppError::bad_request(
                    "Each leave-account category may only be supplied once.",
                ));
            }
            if !(0..=366).contains(&value.base_days)
                || !(0..=366).contains(&value.current_year_days)
                || !(0..=366).contains(&value.next_year_days)
            {
                return Err(AppError::bad_request(
                    "Leave-account days must be between 0 and 366.",
                ));
            }
        }

        if !category_ids.is_empty() {
            let supplied_ids: Vec<i64> = category_ids.iter().copied().collect();
            let matching_ids: Vec<i64> = sqlx::query_scalar(
                "SELECT id FROM absence_categories
                 WHERE id = ANY($1) AND cost_type = 'vacation'",
            )
            .bind(&supplied_ids)
            .fetch_all(&mut *tx)
            .await?;
            if matching_ids.len() != supplied_ids.len() {
                return Err(AppError::bad_request(
                    "Every leave-account value must reference an existing leave-account category.",
                ));
            }
        }

        Self::seed_leave_accounts_for_existing_user_tx(tx, user_id).await?;

        for value in values {
            sqlx::query(
                "INSERT INTO user_leave_accounts (user_id, category_id, base_days)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (user_id, category_id)
                 DO UPDATE SET base_days = EXCLUDED.base_days",
            )
            .bind(user_id)
            .bind(value.category_id)
            .bind(value.base_days)
            .execute(&mut *tx)
            .await?;

            Self::set_leave_account_year_days_tx(
                tx,
                user_id,
                value.category_id,
                current_year,
                value.current_year_days,
            )
            .await?;
            Self::set_leave_account_year_days_tx(
                tx,
                user_id,
                value.category_id,
                next_year,
                value.next_year_days,
            )
            .await?;
        }
        Ok(())
    }

    /// Set one explicit yearly override. This low-level helper is public for
    /// service flows that need to update a single year under their existing
    /// transaction; normal user-form writes should use
    /// [`Self::apply_leave_account_values_tx`] instead.
    pub async fn set_leave_account_year_days_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        category_id: i64,
        year: i32,
        days: i64,
    ) -> AppResult<()> {
        if !(2000..=2100).contains(&year) || !(0..=366).contains(&days) {
            return Err(AppError::bad_request(
                "Leave-account override values are outside their allowed range.",
            ));
        }
        sqlx::query(
            "INSERT INTO user_leave_account_year_overrides (user_id, category_id, year, days)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, category_id, year)
             DO UPDATE SET days = EXCLUDED.days",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(year)
        .bind(days)
        .execute(tx)
        .await?;
        Ok(())
    }

    /// `granted_category_ids`: `None` means every leave-account category is
    /// granted (the pre-existing behavior, used by every caller except
    /// initial user creation); `Some(ids)` restricts the non-zero default to
    /// categories in `ids` — a leave-account category the new user was not
    /// granted access to seeds at zero, the same as an assistant, so their
    /// entitlement never outlives the access decision that grants it.
    async fn seed_leave_accounts_for_user_sql_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        role: &str,
        granted_category_ids: Option<&[i64]>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO user_leave_accounts (user_id, category_id, base_days)
             SELECT
                $1,
                category.id,
                CASE
                    WHEN lower(trim($2)) = $3 THEN 0
                    WHEN $4::BIGINT[] IS NOT NULL AND NOT (category.id = ANY($4)) THEN 0
                    ELSE category.leave_account_default_days
                END
             FROM absence_categories AS category
             WHERE category.cost_type = 'vacation'
             ON CONFLICT (user_id, category_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(role)
        .bind(ROLE_ASSISTANT)
        .bind(granted_category_ids)
        .execute(tx)
        .await?;
        Ok(())
    }

    async fn seed_leave_accounts_for_existing_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_leave_accounts (user_id, category_id, base_days)
             SELECT
                users.id,
                category.id,
                CASE
                    WHEN lower(trim(users.role)) = $2 THEN 0
                    ELSE category.leave_account_default_days
                END
             FROM users
             CROSS JOIN absence_categories AS category
             WHERE users.id = $1
               AND category.cost_type = 'vacation'
             ON CONFLICT (user_id, category_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(ROLE_ASSISTANT)
        .execute(tx)
        .await?;
        Ok(())
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
