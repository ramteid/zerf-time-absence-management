use crate::db::DatabasePool;
use crate::error::AppResult;
use crate::repository::users::User;
use chrono::NaiveDate;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashSet;

#[derive(Clone)]
pub struct ReportDb {
    pool: DatabasePool,
}

impl ReportDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Check whether `target_id` is a non-admin direct report of `requester_id`.
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

    /// Time entries joined with category metadata for a user in a date range.
    /// Returns: (entry_date, start_time, end_time, cat_name, cat_color, category_id, counts_as_work, status, comment)
    #[allow(clippy::type_complexity)]
    pub async fn time_entry_rows(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<
        Vec<(
            NaiveDate,
            String,
            String,
            String,
            String,
            i64,
            bool,
            String,
            Option<String>,
        )>,
    > {
        Ok(sqlx::query_as(
            "SELECT z.entry_date, z.start_time, z.end_time, c.name, c.color, \
             z.category_id, c.counts_as_work, z.status, z.comment \
             FROM time_entries z JOIN categories c ON c.id=z.category_id \
             WHERE z.user_id=$1 AND z.entry_date BETWEEN $2 AND $3 \
             ORDER BY z.entry_date, z.start_time",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Active absences in range: (start_date, end_date, slug, category_name).
    ///
    /// `cancellation_pending` still blocks time logging until an approver
    /// decides, so reporting/flextime must treat it like approved.
    /// `category_name` is returned alongside the slug so PDF rendering can
    /// localise admin-created custom categories (which have no static i18n key).
    pub async fn approved_absence_rows(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate, String, String)>> {
        Ok(sqlx::query_as(
            "SELECT a.start_date, a.end_date, c.slug, c.name \
             FROM absences a JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND a.status IN ('approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Holidays in range as (date, name, local_name) tuples.
    pub async fn holiday_rows(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, String, Option<String>)>> {
        Ok(sqlx::query_as(
            "SELECT holiday_date, name, local_name FROM holidays \
             WHERE holiday_date BETWEEN $1 AND $2",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn holiday_set(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT holiday_date FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// Submitted/approved dates (for all_weeks_submitted check).
    /// Includes ALL entries regardless of counts_as_work: non-crediting entries
    /// fully participate in the submission workflow, so a day covered only by
    /// submitted non-crediting entries still counts as submitted.
    pub async fn submitted_dates_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT DISTINCT entry_date FROM time_entries \
             WHERE user_id=$1 AND status IN ('submitted','approved') \
             AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// Dates that have at least one incomplete entry (for all_weeks_submitted check).
    /// Incomplete means any status outside submitted/approved (e.g. draft or rejected).
    /// Includes ALL entries regardless of counts_as_work.
    pub async fn incomplete_dates_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT DISTINCT entry_date FROM time_entries \
             WHERE user_id=$1 AND status NOT IN ('submitted','approved') \
             AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// Returns presence flags `(has_draft, has_submitted, has_approved, has_rejected)`
    /// for time entries in the given range. Used to derive the frontend
    /// `weekStatus` value on the backend without shipping every entry.
    pub async fn week_status_flags(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<(bool, bool, bool, bool)> {
        let row: (Option<bool>, Option<bool>, Option<bool>, Option<bool>) = sqlx::query_as(
            "SELECT \
                BOOL_OR(status = 'draft'), \
                BOOL_OR(status = 'submitted'), \
                BOOL_OR(status = 'approved'), \
                BOOL_OR(status = 'rejected') \
             FROM time_entries \
             WHERE user_id = $1 AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok((
            row.0.unwrap_or(false),
            row.1.unwrap_or(false),
            row.2.unwrap_or(false),
            row.3.unwrap_or(false),
        ))
    }

    /// Returns true when at least one entry with status='submitted' (pending approval)
    /// exists in the given date range.
    pub async fn has_pending_submitted_entries_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM time_entries \
             WHERE user_id=$1 AND status='submitted' \
             AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    /// Absence ranges in a period (for all_weeks_submitted check).
    pub async fn absence_ranges_in_period(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate, String)>> {
        Ok(sqlx::query_as(
            "SELECT a.start_date, a.end_date, c.slug \
             FROM absences a JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND a.status IN ('approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// All active users for team report. Admins see everyone; team leads see their team.
    pub async fn active_team_members(
        &self,
        requester_id: i64,
        is_admin: bool,
    ) -> AppResult<Vec<User>> {
        const SEL: &str =
            "SELECT id, email, password_hash, first_name, last_name, role, \
             weekly_hours, workdays_per_week, start_date, hire_date, active, must_change_password, created_at, \
             allow_reopen_without_approval, dark_mode, overtime_start_balance_min, tracks_time \
             FROM users";
        if is_admin {
            Ok(QueryBuilder::<Postgres>::new(format!(
                "{SEL} WHERE active=TRUE ORDER BY last_name, first_name, id"
            ))
            .build_query_as::<User>()
            .fetch_all(&self.pool)
            .await?)
        } else {
            // Non-admin leads see themselves plus direct reports, but admin
            // subjects are excluded from lead-scoped team views (user-guide).
            Ok(QueryBuilder::<Postgres>::new(format!(
                "{SEL} WHERE active=TRUE \
                 AND (id=$1 OR id IN (\
                     SELECT ua.user_id FROM user_approvers ua \
                     JOIN users u ON u.id = ua.user_id \
                     WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin'\
                 )) \
                 ORDER BY last_name, first_name, id"
            ))
            .build_query_as::<User>()
            .bind(requester_id)
            .fetch_all(&self.pool)
            .await?)
        }
    }

    /// User start date and overtime start balance (minutes).
    pub async fn user_start_and_overtime(&self, user_id: i64) -> AppResult<(NaiveDate, i64)> {
        Ok(
            sqlx::query_as("SELECT start_date, overtime_start_balance_min FROM users WHERE id=$1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// Time entry rows for flextime (raw: date, start, end, status, counts_as_work).
    pub async fn flextime_entries(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, String, String, String, bool)>> {
        Ok(sqlx::query_as(
            "SELECT z.entry_date, z.start_time, z.end_time, z.status, c.counts_as_work \
             FROM time_entries z \
             JOIN categories c ON c.id = z.category_id \
             WHERE z.user_id=$1 AND z.entry_date BETWEEN $2 AND $3 \
             ORDER BY entry_date, start_time",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Category entries for a user (for per-category report).
    /// Returns (date, start, end, cat_name, cat_color, minutes, counts_as_work, status, comment).
    #[allow(clippy::type_complexity)]
    pub async fn category_entries_for_user(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<
        Vec<(
            NaiveDate,
            String,
            String,
            String,
            String,
            i64,
            bool,
            String,
            Option<String>,
        )>,
    > {
        Ok(sqlx::query_as(
            "SELECT z.entry_date, z.start_time, z.end_time, c.name, c.color, \
             z.category_id, c.counts_as_work, z.status, z.comment \
             FROM time_entries z JOIN categories c ON c.id=z.category_id \
             WHERE z.user_id=$1 AND z.entry_date BETWEEN $2 AND $3 \
             ORDER BY z.entry_date, z.start_time",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// All active time-tracking users in the team scope for the category report.
    /// Pure-admin users (tracks_time=FALSE) are excluded to stay consistent with the
    /// team overview report, which filters them out via `.filter(|m| m.tracks_time)`.
    pub async fn team_category_members(
        &self,
        requester_id: i64,
        is_admin: bool,
    ) -> AppResult<Vec<(i64, String, String)>> {
        if is_admin {
            Ok(sqlx::query_as(
                "SELECT id, first_name, last_name FROM users \
                 WHERE active=TRUE AND tracks_time=TRUE ORDER BY last_name, first_name, id",
            )
            .fetch_all(&self.pool)
            .await?)
        } else {
            // Non-admin leads: exclude admin subjects from lead-scoped views.
            Ok(sqlx::query_as(
                "SELECT id, first_name, last_name FROM users \
                 WHERE active=TRUE AND tracks_time=TRUE \
                 AND (id=$1 OR id IN (\
                     SELECT ua.user_id FROM user_approvers ua \
                     JOIN users u ON u.id = ua.user_id \
                     WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin'\
                 )) \
                 ORDER BY last_name, first_name, id",
            )
            .bind(requester_id)
            .fetch_all(&self.pool)
            .await?)
        }
    }

    /// Category rows for either a specific user or the requester's team scope.
    /// Returns (category_name, color, start_time, end_time).
    pub async fn category_rows_for_scope(
        &self,
        requester_id: i64,
        is_admin: bool,
        target_user_id: Option<i64>,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(String, String, String, String)>> {
        if let Some(user_id) = target_user_id {
            return Ok(sqlx::query_as(
                "SELECT c.name, c.color, z.start_time, z.end_time \
                 FROM time_entries z \
                 JOIN users u ON u.id=z.user_id \
                 JOIN categories c ON c.id=z.category_id \
                 WHERE z.status != 'rejected' AND z.entry_date >= u.start_date \
                 AND z.entry_date BETWEEN $1 AND $2 AND z.user_id = $3",
            )
            .bind(from)
            .bind(to)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?);
        }

        if is_admin {
            Ok(sqlx::query_as(
                "SELECT c.name, c.color, z.start_time, z.end_time \
                 FROM time_entries z \
                 JOIN users u ON u.id=z.user_id \
                 JOIN categories c ON c.id=z.category_id \
                 WHERE z.status != 'rejected' AND u.active=TRUE AND u.tracks_time=TRUE \
                 AND z.entry_date >= u.start_date \
                 AND z.entry_date BETWEEN $1 AND $2",
            )
            .bind(from)
            .bind(to)
            .fetch_all(&self.pool)
            .await?)
        } else {
            Ok(sqlx::query_as(
                "SELECT c.name, c.color, z.start_time, z.end_time \
                 FROM time_entries z \
                 JOIN users u ON u.id=z.user_id \
                 JOIN categories c ON c.id=z.category_id \
                 WHERE z.status != 'rejected' AND u.active=TRUE AND u.tracks_time=TRUE \
                 AND z.entry_date >= u.start_date \
                 AND z.entry_date BETWEEN $1 AND $2 \
                 AND z.user_id IN (SELECT id FROM users WHERE id = $3 \
                     OR id IN (SELECT ua.user_id FROM user_approvers ua \
                               JOIN users u2 ON u2.id = ua.user_id \
                               WHERE ua.approver_id = $3 AND u2.active=TRUE AND u2.role != 'admin'))",
            )
            .bind(from)
            .bind(to)
            .bind(requester_id)
            .fetch_all(&self.pool)
            .await?)
        }
    }

    /// Team-scope category rows. Returns (user_id, category_name, color, start_time, end_time).
    pub async fn team_category_entry_rows(
        &self,
        requester_id: i64,
        is_admin: bool,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(i64, String, String, String, String)>> {
        if is_admin {
            Ok(sqlx::query_as(
                "SELECT z.user_id, c.name, c.color, z.start_time, z.end_time \
                 FROM time_entries z \
                 JOIN users u ON u.id=z.user_id \
                 JOIN categories c ON c.id=z.category_id \
                 WHERE z.status != 'rejected' AND u.active=TRUE AND u.tracks_time=TRUE \
                 AND z.entry_date >= u.start_date \
                 AND z.entry_date BETWEEN $1 AND $2",
            )
            .bind(from)
            .bind(to)
            .fetch_all(&self.pool)
            .await?)
        } else {
            Ok(sqlx::query_as(
                "SELECT z.user_id, c.name, c.color, z.start_time, z.end_time \
                 FROM time_entries z \
                 JOIN users u ON u.id=z.user_id \
                 JOIN categories c ON c.id=z.category_id \
                 WHERE z.status != 'rejected' AND u.active=TRUE AND u.tracks_time=TRUE \
                 AND z.entry_date >= u.start_date \
                 AND z.entry_date BETWEEN $1 AND $2 \
                 AND z.user_id IN (SELECT id FROM users WHERE id = $3 \
                     OR id IN (SELECT ua.user_id FROM user_approvers ua \
                               JOIN users u2 ON u2.id = ua.user_id \
                               WHERE ua.approver_id = $3 AND u2.active=TRUE AND u2.role != 'admin'))",
            )
            .bind(from)
            .bind(to)
            .bind(requester_id)
            .fetch_all(&self.pool)
            .await?)
        }
    }
}
