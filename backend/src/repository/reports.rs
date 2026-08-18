use crate::db::DatabasePool;
use crate::error::AppResult;
use crate::repository::time_entries::{
    EFFECTIVE_REJECTED_TIME_ENTRY_CONDITION, INCOMPLETE_TIME_ENTRY_CONDITION,
};
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

    /// Active absences in range: (id, start_date, end_date, slug, category_name).
    ///
    /// `cancellation_pending` still blocks time logging until an approver
    /// decides, so reporting/flextime must treat it like approved.
    /// `category_name` is returned alongside the slug so PDF rendering can
    /// localise admin-created custom categories (which have no static i18n key).
    /// `id` lets the payroll report look up the "AU required" verdict computed
    /// by `services::medical_certificate` for this specific absence.
    pub async fn approved_absence_rows(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(i64, NaiveDate, NaiveDate, String, String)>> {
        Ok(sqlx::query_as(
            "SELECT a.id, a.start_date, a.end_date, c.slug, c.name \
             FROM absences a JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND a.status IN ('approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3 \
             ORDER BY a.start_date, a.id",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Holidays in range as (date, name, local_name) tuples. Delegates to
    /// HolidayDb, the single source of truth for holiday dates: it also
    /// accounts for recurring manual holidays, which a literal
    /// `holiday_date BETWEEN` query here would miss for any year after the
    /// one they were first added for.
    pub async fn holiday_rows(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, String, Option<String>)>> {
        crate::repository::HolidayDb::new(self.pool.clone())
            .get_rows_in_range(from, to)
            .await
    }

    pub async fn holiday_set(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        crate::repository::HolidayDb::new(self.pool.clone())
            .get_dates_in_range(from, to)
            .await
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
        let sql = format!(
            "SELECT DISTINCT te.entry_date FROM time_entries te \
             WHERE te.user_id=$1 AND ({INCOMPLETE_TIME_ENTRY_CONDITION}) \
             AND te.entry_date BETWEEN $2 AND $3"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// Dates with at least one `status='approved'` entry (for the flextime
    /// balance cutoff: weeks are approved as a whole, so a day only "counts"
    /// once its entries have cleared approval).
    pub async fn approved_dates_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT DISTINCT entry_date FROM time_entries \
             WHERE user_id=$1 AND status='approved' \
             AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// Dates with at least one entry that is not settled: draft, submitted, or
    /// a rejection nobody has corrected yet. Any such date blocks its week from
    /// counting as "fully approved" for the flextime balance cutoff, even if
    /// every required day already has an approved entry alongside it.
    ///
    /// Rejections that an approved correction already closed
    /// (`rejection_resolved_at IS NOT NULL`) are pure history — same rule as
    /// `INCOMPLETE_TIME_ENTRY_CONDITION`. Counting them would freeze the cutoff
    /// forever on the week of any rejection that was later fixed.
    pub async fn unapproved_entry_dates_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT DISTINCT entry_date FROM time_entries \
             WHERE user_id=$1 AND status <> 'approved' \
             AND (status <> 'rejected' OR rejection_resolved_at IS NULL) \
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
        let sql = format!(
            "SELECT \
                BOOL_OR(te.status = 'draft'), \
                BOOL_OR(te.status = 'submitted'), \
                BOOL_OR(te.status = 'approved'), \
                BOOL_OR({EFFECTIVE_REJECTED_TIME_ENTRY_CONDITION}) \
             FROM time_entries te \
             WHERE te.user_id = $1 AND te.entry_date BETWEEN $2 AND $3"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        let row: (Option<bool>, Option<bool>, Option<bool>, Option<bool>) =
            sqlx::query_as(sqlx::AssertSqlSafe(sql))
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

    /// Returns true when the period still contains time-entry workflow state
    /// that nobody has finalized. Historical-only users cannot resolve these
    /// rows themselves, so the archive export must wait instead of producing a
    /// PDF with draft, pending, or still-rejected data.
    pub async fn has_unresolved_time_entries_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<bool> {
        let sql = format!(
            "SELECT EXISTS ( \
                SELECT 1 FROM time_entries te \
                WHERE te.user_id=$1 \
                AND te.entry_date BETWEEN $2 AND $3 \
                AND (te.status IN ('draft','submitted') \
                     OR ({EFFECTIVE_REJECTED_TIME_ENTRY_CONDITION})) \
             )"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_one(&self.pool)
            .await?)
    }

    /// User IDs with at least one time entry in the period, regardless of its
    /// workflow status. Payroll uses this to ignore assistants who did not
    /// work at all in a given month while retaining every assistant who has
    /// started recording hours for it.
    pub async fn user_ids_with_time_entries_in_range(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<i64>> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT user_id FROM time_entries \
             WHERE entry_date BETWEEN $1 AND $2",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(user_id,)| user_id).collect())
    }

    /// User IDs with payroll-relevant absences (auto_approve_past OR unpaid) in period.
    pub async fn user_ids_with_payroll_absences_in_range(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<i64>> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT a.user_id FROM absences a JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.status IN ('approved','cancellation_pending') AND (c.auto_approve_past=TRUE OR c.unpaid=TRUE) \
             AND a.end_date >= $1 AND a.start_date <= $2",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Returns true when the current start date would cause the PDF renderer to
    /// hide existing report content in this period.
    pub async fn has_report_content_before_start_date(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        user_start_date: NaiveDate,
    ) -> AppResult<bool> {
        if user_start_date <= from {
            return Ok(false);
        }

        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
                SELECT 1 FROM time_entries te \
                WHERE te.user_id=$1 \
                AND te.entry_date BETWEEN $2 AND $3 \
                AND te.entry_date < $4 \
                AND te.status != 'rejected' \
                UNION ALL \
                SELECT 1 FROM absences a \
                WHERE a.user_id=$1 \
                AND a.status IN ('approved','cancellation_pending') \
                AND a.end_date >= $2 \
                AND a.start_date <= $3 \
                AND a.start_date < $4 \
             )",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .bind(user_start_date)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Absence ranges in a period for the submission-reminder and user-facing
    /// completeness check.
    ///
    /// Includes `requested`, `approved`, and `cancellation_pending` absences.
    /// A `requested` absence already blocks time-entry creation via `validate_entry`
    /// (the employee cannot log work on days covered by a pending non-auto-approve
    /// absence), so those days must also be treated as excused here.  Excluding them
    /// would make any week containing a backdated requested absence permanently
    /// unsubmittable and trigger endless submission reminders until the approver
    /// decides.
    pub async fn absence_ranges_in_period(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate, String)>> {
        Ok(sqlx::query_as(
            "SELECT a.start_date, a.end_date, c.slug \
             FROM absences a JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND a.status IN ('requested','approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Absence ranges in a period using only finalized statuses (`approved`,
    /// `cancellation_pending`).  Used by the PDF-export gate: a `requested`
    /// absence means the month is not yet decided, so it should not pass the gate
    /// because the PDF content side (`approved_absence_rows`) excludes requested
    /// absences, so those days would render as 0-hour rows with a full daily
    /// deficit in the archived timesheet.
    pub async fn finalized_absence_ranges_in_period(
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

    /// Returns true when an undecided absence request overlaps the period.
    /// The report-upload gate uses this as a hard stop: even if the affected
    /// days already have submitted entries, the month is not final until the
    /// request is approved, rejected, or withdrawn.
    pub async fn has_requested_absences_in_period(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
                SELECT 1 FROM absences a \
                WHERE a.user_id=$1 AND a.status='requested' \
                AND a.end_date >= $2 AND a.start_date <= $3 \
             )",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
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
             allow_reopen_without_approval, allow_submission_without_approval, dark_mode, \
             overtime_start_balance_min, tracks_time, archived_at, \
             receives_error_notifications \
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

    /// All users who should appear in a monthly timesheet export for a given period.
    ///
    /// Includes users with active tracking and users whose historical rows touch
    /// the period, even if tracking is now disabled or the account is archived.
    /// Users whose current start date is after the period are intentionally
    /// excluded because the renderer would suppress those pre-start rows.
    pub async fn timesheet_members_for_period(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<User>> {
        const SQL: &str =
            "SELECT id, email, password_hash, first_name, last_name, role, \
             weekly_hours, workdays_per_week, start_date, hire_date, active, must_change_password, created_at, \
             allow_reopen_without_approval, allow_submission_without_approval, dark_mode, \
             overtime_start_balance_min, tracks_time, archived_at, \
             receives_error_notifications \
             FROM users \
             WHERE start_date <= $2 \
             AND ((active=TRUE AND tracks_time=TRUE) \
                  OR EXISTS (SELECT 1 FROM time_entries te \
                             WHERE te.user_id = users.id \
                             AND te.entry_date BETWEEN $1 AND $2) \
                  OR EXISTS (SELECT 1 FROM absences ab \
                             WHERE ab.user_id = users.id \
                             AND ab.status IN ('approved','cancellation_pending') \
                             AND ab.start_date <= $2 AND ab.end_date >= $1) \
             ) \
             ORDER BY last_name, first_name, id";
        Ok(sqlx::query_as(SQL)
            .bind(from)
            .bind(to)
            .fetch_all(&self.pool)
            .await?)
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
