use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashSet;

/// `Absence` rows expose both `category_id` (the canonical reference) and
/// `kind` (the slug, populated by joining absence_categories). Frontend code
/// and business logic that previously branched on `kind == "vacation"` is
/// gradually migrating to the explicit behavior flags also returned here, but
/// the slug remains useful as a stable i18n key and for legacy callers.
/// `category_name` and `category_color` are projected directly from the joined
/// category row so that dialogs can display the correct label and color even
/// when the category has since been deactivated.
#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct Absence {
    pub id: i64,
    pub user_id: i64,
    pub category_id: i64,
    pub kind: String,
    pub category_name: String,
    pub category_color: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
    pub status: String,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub counts_as_vacation: bool,
    pub keeps_work_target: bool,
    pub auto_approve_past: bool,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct CalendarEntry {
    pub id: i64,
    pub user_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub kind: String,
    pub category_id: i64,
    /// Display name from the joined category row. Present even when the
    /// category has been deactivated, so the calendar tooltip can render the
    /// real name instead of falling back to the raw slug — the active-only
    /// `/absence-categories` list that powers the frontend store cannot
    /// resolve inactive slugs on its own.
    pub category_name: String,
    /// When true, non-lead teammates can see the real absence kind in the team
    /// calendar. Replaces the old `counts_as_vacation` privacy gate so each
    /// category can independently control its calendar visibility.
    pub team_visible: bool,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
    pub status: String,
}

/// SELECT prefix that joins absence_categories to expose the slug (`kind`) and
/// the three behavior booleans on every absence row. Use this everywhere
/// absences are fetched as `Absence` structs — keeping the projection in one
/// place ensures the row shape stays consistent across queries.
const ABS_SELECT: &str =
    "SELECT a.id, a.user_id, a.category_id, c.slug AS kind, c.name AS category_name, \
     c.color AS category_color, a.start_date, a.end_date, \
     a.comment, a.status, a.reviewed_by, a.reviewed_at, a.rejection_reason, a.created_at, \
     c.counts_as_vacation, c.keeps_work_target, c.auto_approve_past \
     FROM absences a JOIN absence_categories c ON c.id = a.category_id";

#[derive(Clone)]
pub struct AbsenceDb {
    pool: DatabasePool,
}

impl AbsenceDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    // ── Holidays helpers ───────────────────────────────────────────────────

    pub async fn holidays_set(
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

    /// Count contract workdays in a date range, excluding public holidays.
    ///
    /// Contract workdays are determined by `workdays_per_week`:
    ///   - workdays_per_week=5: Mon-Fri (ISO weekday 0-4)
    ///   - workdays_per_week=4: Mon-Thu (ISO weekday 0-3)
    ///   - workdays_per_week=6: Mon-Sat (ISO weekday 0-5)
    ///
    /// ISO weekday mapping: 0=Monday, 1=Tuesday, ..., 6=Sunday
    /// A day is a contract workday if: ISO_weekday < workdays_per_week
    fn workdays_in_window(
        from: NaiveDate,
        to: NaiveDate,
        holidays: &HashSet<NaiveDate>,
        workdays_per_week: i16,
    ) -> f64 {
        if to < from {
            return 0.0;
        }
        let mut count = 0.0;
        let mut d = from;
        while d <= to {
            // ISO weekday 0=Mon, 6=Sun; contract workdays are first N days of week
            if d.weekday().num_days_from_monday() < workdays_per_week as u32
                && !holidays.contains(&d)
            {
                count += 1.0;
            }
            d += Duration::days(1);
        }
        count
    }

    /// Fetch the user's configured workdays_per_week (contract hours per week).
    /// Returns 1-7; default is typically 5 (Mon-Fri).
    pub async fn user_workdays_per_week(&self, user_id: i64) -> AppResult<i16> {
        Ok(
            sqlx::query_scalar("SELECT workdays_per_week FROM users WHERE id=$1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// Count default contract workdays (Mon-Fri, hardcoded 5 days) between `from` and `to`
    /// (inclusive), excluding public holidays.
    /// NOTE: This function is for legacy compatibility. Prefer workdays_for_user() for
    /// per-user workday calculations.
    pub async fn workdays(&self, from: NaiveDate, to: NaiveDate) -> AppResult<f64> {
        if to < from {
            return Ok(0.0);
        }
        let holidays = self.holidays_set(from, to).await?;
        Ok(Self::workdays_in_window(from, to, &holidays, 5))
    }

    /// Count user-specific contract workdays between `from` and `to` (inclusive),
    /// excluding public holidays.
    ///
    /// This respects the user's workdays_per_week setting. For example:
    ///   - A 5-day worker: counts Mon-Fri
    ///   - A 4-day worker: counts Mon-Thu
    ///   - A 6-day worker: counts Mon-Sat
    pub async fn workdays_for_user(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<f64> {
        if to < from {
            return Ok(0.0);
        }
        let holidays = self.holidays_set(from, to).await?;
        let workdays_per_week = self.user_workdays_per_week(user_id).await?;
        // Count contract workdays for this specific user based on their workdays_per_week setting.
        // Contract days are determined by: ISO_weekday < workdays_per_week (0=Mon, 6=Sun)
        // Example: 5 days = Mon-Fri, 4 days = Mon-Thu, 6 days = Mon-Sat
        Ok(Self::workdays_in_window(
            from,
            to,
            &holidays,
            workdays_per_week,
        ))
    }

    /// Sum of workdays for approved (and cancellation_pending) absences whose
    /// category matches `category_id`, clamped to the [from, to] window. Used
    /// by team reports for kind-specific totals (e.g. "vacation taken" =
    /// total workdays in approved absences from the vacation category).
    pub async fn workdays_total_for_category(
        &self,
        user_id: i64,
        category_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<f64> {
        self.workdays_total_for_category_filtered(
            user_id,
            category_id,
            from,
            to,
            &["approved", "cancellation_pending"],
        )
        .await
    }

    pub async fn workdays_total_for_category_filtered(
        &self,
        user_id: i64,
        category_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        statuses: &[&str],
    ) -> AppResult<f64> {
        let ranges: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
            "SELECT start_date, end_date FROM absences \
             WHERE user_id=$1 AND category_id=$2 AND status = ANY($3) \
             AND end_date >= $4 AND start_date <= $5",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(statuses)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        let workdays_per_week = self.user_workdays_per_week(user_id).await?;
        let holidays = self.holidays_set(from, to).await?;
        let mut total = 0.0;
        for (s, e) in ranges {
            let cs = std::cmp::max(s, from);
            let ce = std::cmp::min(e, to);
            total += Self::workdays_in_window(cs, ce, &holidays, workdays_per_week);
        }
        Ok(total)
    }

    /// Sum of workdays for absences whose category has `auto_approve_past=TRUE`
    /// (sick-like behaviour) regardless of how many such categories exist.
    /// Used by the team report's "sick days" column.
    pub async fn auto_approve_workdays_total(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<f64> {
        let ranges: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
            "SELECT a.start_date, a.end_date FROM absences a \
             JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND c.auto_approve_past=TRUE \
             AND a.status IN ('approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        let workdays_per_week = self.user_workdays_per_week(user_id).await?;
        let holidays = self.holidays_set(from, to).await?;
        let mut total = 0.0;
        for (s, e) in ranges {
            let cs = std::cmp::max(s, from);
            let ce = std::cmp::min(e, to);
            total += Self::workdays_in_window(cs, ce, &holidays, workdays_per_week);
        }
        Ok(total)
    }

    /// Sum of workdays for absences whose category has `counts_as_vacation=TRUE`
    /// (regardless of slug) in the requested statuses, clamped to [from, to].
    /// Used by the team report's vacation columns.
    pub async fn vacation_workdays_total(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<f64> {
        self.vacation_workdays_total_filtered(
            user_id,
            from,
            to,
            &["approved", "cancellation_pending"],
        )
        .await
    }

    /// Sum of workdays for absences whose category has `counts_as_vacation=TRUE`
    /// and whose status is in `statuses`, clamped to [from, to]. Used by
    /// carryover/balance calculations that previously filtered on `kind='vacation'`.
    pub async fn vacation_workdays_total_filtered(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        statuses: &[&str],
    ) -> AppResult<f64> {
        let ranges: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
            "SELECT a.start_date, a.end_date FROM absences a \
             JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND c.counts_as_vacation=TRUE AND a.status = ANY($2) \
             AND a.end_date >= $3 AND a.start_date <= $4",
        )
        .bind(user_id)
        .bind(statuses)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        let workdays_per_week = self.user_workdays_per_week(user_id).await?;
        let holidays = self.holidays_set(from, to).await?;
        let mut total = 0.0;
        for (s, e) in ranges {
            let cs = std::cmp::max(s, from);
            let ce = std::cmp::min(e, to);
            total += Self::workdays_in_window(cs, ce, &holidays, workdays_per_week);
        }
        Ok(total)
    }

    // ── Queries ────────────────────────────────────────────────────────────

    pub async fn find_by_id(&self, id: i64) -> AppResult<Absence> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{ABS_SELECT} WHERE a.id=$1"))
                .build_query_as::<Absence>()
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn get_user_id(&self, id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM absences WHERE id=$1")
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn list_for_user(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<Absence>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{ABS_SELECT} WHERE a.user_id=$1 AND a.end_date >= $2 AND a.start_date <= $3 \
             ORDER BY a.start_date DESC"
        ))
        .build_query_as::<Absence>()
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn list_all(
        &self,
        is_admin: bool,
        requester_id: i64,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        status_filter: Option<&str>,
    ) -> AppResult<Vec<Absence>> {
        let mut builder = QueryBuilder::<Postgres>::new(format!("{ABS_SELECT} WHERE TRUE"));
        if !is_admin {
            // Non-admin leads: only show absences from active, non-admin direct
            // reports. Admin-subject absences are excluded from lead scope.
            builder
                .push(" AND a.user_id IN (SELECT ua.user_id FROM user_approvers ua JOIN users u ON u.id=ua.user_id WHERE ua.approver_id = ")
                .push_bind(requester_id)
                .push(" AND u.active=TRUE AND u.role != 'admin')");
        }
        if let Some(f) = from {
            builder.push(" AND a.end_date >= ").push_bind(f);
        }
        if let Some(t) = to {
            builder.push(" AND a.start_date <= ").push_bind(t);
        }
        if let Some(s) = status_filter {
            if s == "pending_review" {
                builder.push(" AND a.status IN ('requested','cancellation_pending')");
            } else {
                builder.push(" AND a.status = ").push_bind(s.to_owned());
            }
        }
        builder.push(" ORDER BY a.start_date DESC");
        Ok(builder
            .build_query_as::<Absence>()
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn calendar_scope_user_ids(
        &self,
        requester_id: i64,
        is_admin: bool,
        is_lead: bool,
    ) -> AppResult<Option<Vec<i64>>> {
        if is_admin {
            return Ok(None); // see all
        }
        let mut ids = vec![requester_id];
        if is_lead {
            // Non-admin leads: exclude admin subjects from lead-scoped calendar
            // view, consistent with the scope rule for all lead-scoped views.
            let mut reports: Vec<i64> = sqlx::query_scalar(
                "SELECT ua.user_id FROM user_approvers ua \
                 JOIN users u ON u.id = ua.user_id \
                 WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role != 'admin'",
            )
            .bind(requester_id)
            .fetch_all(&self.pool)
            .await?;
            ids.append(&mut reports);
        }
        // Regular employees and assistants only see their own absences in the
        // calendar. They have no business need to view team-mate or approver
        // data, and exposing sensitive absence kinds (e.g. sick leave under
        // GDPR Art. 9) across the team is a privacy violation.
        ids.sort_unstable();
        ids.dedup();
        Ok(Some(ids))
    }

    pub async fn calendar_entries(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        scope_ids: Option<&[i64]>,
    ) -> AppResult<Vec<CalendarEntry>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT a.id, a.user_id, u.first_name, u.last_name, c.slug AS kind, a.category_id, \
             c.name AS category_name, c.team_visible, \
             a.start_date, a.end_date, a.comment, a.status \
             FROM absences a \
             JOIN users u ON u.id=a.user_id \
             JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.status IN ('requested','approved','cancellation_pending') \
             AND a.end_date >=",
        );
        builder.push_bind(from);
        builder.push(" AND a.start_date <= ").push_bind(to);
        // Calendar scope rule:
        //   - admins: see every user's absences (scope_ids = None).
        //   - leads:  see their own + direct reports (scope_ids = [...]).
        //   - employees/assistants: see their own (scope_ids = [self]).
        // For ALL viewers we additionally expose absences from out-of-scope
        // users when the category has team_visible=TRUE — that is the whole
        // point of the flag and the user-guide promises non-leads can see
        // teammates' vacation, training, etc. in the team calendar. Sensitive
        // categories (team_visible=FALSE, e.g. sick leave) stay restricted to
        // the requester's normal scope. Handler then masks the displayed kind
        // for cross-user entries when needed.
        if let Some(ids) = scope_ids {
            builder.push(" AND (c.team_visible=TRUE OR a.user_id IN (");
            let mut sep = builder.separated(", ");
            for id in ids {
                sep.push_bind(*id);
            }
            sep.push_unseparated("))");
        }
        builder.push(" ORDER BY a.start_date");
        Ok(builder
            .build_query_as::<CalendarEntry>()
            .fetch_all(&self.pool)
            .await?)
    }

    /// Load vacation absences (categories with counts_as_vacation=TRUE) for
    /// balance calculation. Includes requested/approved/cancellation_pending.
    pub async fn vacation_absences_in_year(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<Absence>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{ABS_SELECT} WHERE a.user_id=$1 AND c.counts_as_vacation=TRUE \
             AND a.status IN ('requested','approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3"
        ))
        .build_query_as::<Absence>()
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Approved/cancellation_pending absences in the requested window, with the
    /// category slug as `kind`. Consumers iterate the slug for labelling.
    pub async fn approved_ranges_in_period(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate, String)>> {
        Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate, String)>(
            "SELECT a.start_date, a.end_date, c.slug FROM absences a \
             JOIN absence_categories c ON c.id = a.category_id \
             WHERE a.user_id=$1 AND a.status IN ('approved','cancellation_pending') \
             AND a.end_date >= $2 AND a.start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    // ── Mutations ──────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        user_id: i64,
        category_id: i64,
        // Kept for call-site compatibility; time-conflict is checked at approval, not creation.
        _category_auto_approve_past: bool,
        start_date: NaiveDate,
        end_date: NaiveDate,
        comment: Option<&str>,
        initial_status: &str,
    ) -> AppResult<Absence> {
        let mut tx = self.pool.begin().await?;
        Self::lock_user_scope_tx(&mut tx, user_id).await?;

        let overlap: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM absences WHERE user_id=$1 \
             AND status IN ('requested','approved','cancellation_pending') \
             AND end_date >= $2 AND start_date <= $3",
        )
        .bind(user_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_one(&mut *tx)
        .await?;
        if overlap > 0 {
            return Err(AppError::conflict("Overlap with existing absence."));
        }

        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO absences(user_id, category_id, start_date, end_date, comment, status) \
             VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(start_date)
        .bind(end_date)
        .bind(comment)
        .bind(initial_status)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        self.find_by_id(new_id).await
    }

    pub async fn cancel(&self, absence_id: i64, owner_id: i64) -> AppResult<Absence> {
        let mut tx = self.pool.begin().await?;
        Self::lock_user_scope_tx(&mut tx, owner_id).await?;
        let before: Absence = QueryBuilder::<Postgres>::new(format!(
            "{ABS_SELECT} WHERE a.id=$1 FOR UPDATE OF a"
        ))
        .build_query_as::<Absence>()
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("UPDATE absences SET status='cancelled' WHERE id=$1")
            .bind(absence_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(before)
    }

    pub async fn approve_tx(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
        reviewer_id: i64,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE absences SET status='approved', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='requested'",
        )
        .bind(reviewer_id)
        .bind(absence_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    pub async fn reject_tx(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
        reviewer_id: i64,
        reason: &str,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE absences SET status='rejected', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 \
             WHERE id=$3 AND status='requested'",
        )
        .bind(reviewer_id)
        .bind(reason)
        .bind(absence_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    pub async fn revoke_tx(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
        reviewer_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE absences SET status='cancelled', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP WHERE id=$2",
        )
        .bind(reviewer_id)
        .bind(absence_id)
        .execute(tx)
        .await?;
        Ok(())
    }

    pub async fn request_cancellation_tx(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE absences SET status='cancellation_pending' \
             WHERE id=$1 AND status='approved'",
        )
        .bind(absence_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    pub async fn approve_cancellation_tx(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
        reviewer_id: i64,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE absences SET status='cancelled', reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='cancellation_pending'",
        )
        .bind(reviewer_id)
        .bind(absence_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    pub async fn reject_cancellation_tx(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
        reviewer_id: i64,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE absences SET status='approved', reviewed_by=$2, reviewed_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND status='cancellation_pending'",
        )
        .bind(absence_id)
        .bind(reviewer_id)
        .execute(tx)
        .await?
        .rows_affected())
    }

    pub async fn find_for_update(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
    ) -> AppResult<Absence> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{ABS_SELECT} WHERE a.id=$1 FOR UPDATE OF a"
        ))
        .build_query_as::<Absence>()
        .bind(absence_id)
        .fetch_one(tx)
        .await?)
    }

    pub async fn is_direct_report_for_update(
        tx: &mut sqlx::PgConnection,
        subject_id: i64,
        approver_id: i64,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, Option<bool>>(
            "SELECT TRUE FROM user_approvers ua \
             WHERE ua.user_id=$1 AND ua.approver_id=$2 \
             AND EXISTS (SELECT 1 FROM users u WHERE u.id=$1 AND u.active=TRUE AND u.role != 'admin') \
             FOR UPDATE",
        )
        .bind(subject_id)
        .bind(approver_id)
        .fetch_optional(tx)
        .await?
        .flatten()
        .is_some())
    }

    // ── Vacation balance helpers ───────────────────────────────────────────

    /// Load ranges of absences whose category has counts_as_vacation=TRUE in
    /// statuses that reserve vacation budget (requested/approved/
    /// cancellation_pending), optionally excluding one absence id.
    pub async fn vacation_ranges_in_year_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        exclude_id: Option<i64>,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate)>> {
        if let Some(excl) = exclude_id {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT a.start_date, a.end_date FROM absences a \
                 JOIN absence_categories c ON c.id = a.category_id \
                 WHERE a.id != $1 AND a.user_id=$2 AND c.counts_as_vacation=TRUE \
                 AND a.status IN ('requested','approved','cancellation_pending') \
                 AND a.end_date >= $3 AND a.start_date <= $4",
            )
            .bind(excl)
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(tx)
            .await?)
        } else {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT a.start_date, a.end_date FROM absences a \
                 JOIN absence_categories c ON c.id = a.category_id \
                 WHERE a.user_id=$1 AND c.counts_as_vacation=TRUE \
                 AND a.status IN ('requested','approved','cancellation_pending') \
                 AND a.end_date >= $2 AND a.start_date <= $3",
            )
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(tx)
            .await?)
        }
    }

    /// Pending/approved/cancellation_pending ranges for categories with
    /// `keeps_work_target=TRUE` (flextime-cost categories) whose end_date is
    /// on or after `from`. Used by `validate_flextime_balance` to subtract
    /// committed-but-not-yet-realised flextime usage so multiple overlapping
    /// requests cannot each individually fit yet collectively breach the floor.
    pub async fn keeps_work_target_ranges_after_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        from: NaiveDate,
        exclude_id: Option<i64>,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate)>> {
        if let Some(excl) = exclude_id {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT a.start_date, a.end_date FROM absences a \
                 JOIN absence_categories c ON c.id = a.category_id \
                 WHERE a.id != $1 AND a.user_id=$2 AND c.keeps_work_target=TRUE \
                 AND a.status IN ('requested','approved','cancellation_pending') \
                 AND a.end_date >= $3",
            )
            .bind(excl)
            .bind(user_id)
            .bind(from)
            .fetch_all(tx)
            .await?)
        } else {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT a.start_date, a.end_date FROM absences a \
                 JOIN absence_categories c ON c.id = a.category_id \
                 WHERE a.user_id=$1 AND c.keeps_work_target=TRUE \
                 AND a.status IN ('requested','approved','cancellation_pending') \
                 AND a.end_date >= $2",
            )
            .bind(user_id)
            .bind(from)
            .fetch_all(tx)
            .await?)
        }
    }

    /// Approved-only vacation ranges, optionally excluding one absence id.
    pub async fn approved_vacation_ranges_in_year_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        exclude_id: Option<i64>,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate)>> {
        if let Some(excl) = exclude_id {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT a.start_date, a.end_date FROM absences a \
                 JOIN absence_categories c ON c.id = a.category_id \
                 WHERE a.id != $1 AND a.user_id=$2 AND c.counts_as_vacation=TRUE \
                 AND a.status='approved' \
                 AND a.end_date >= $3 AND a.start_date <= $4",
            )
            .bind(excl)
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(tx)
            .await?)
        } else {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT a.start_date, a.end_date FROM absences a \
                 JOIN absence_categories c ON c.id = a.category_id \
                 WHERE a.user_id=$1 AND c.counts_as_vacation=TRUE \
                 AND a.status='approved' \
                 AND a.end_date >= $2 AND a.start_date <= $3",
            )
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(tx)
            .await?)
        }
    }

    // ── Transaction helpers ────────────────────────────────────────────────

    pub async fn lock_user_scope_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
    ) -> AppResult<()> {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(user_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Block creating an absence on days that already carry logged time entries,
    /// except for "auto-approve past" categories (sick-like behavior) where
    /// partial-day overlap is intentional — someone may have worked the morning
    /// then called in sick at lunch.
    pub async fn ensure_no_time_conflict_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        auto_approve_past: bool,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> AppResult<()> {
        if auto_approve_past {
            return Ok(());
        }
        let conflict: Option<NaiveDate> = sqlx::query_scalar(
            "SELECT te.entry_date FROM time_entries te \
                             WHERE te.user_id=$1 AND te.status <> 'rejected' \
                         AND te.entry_date BETWEEN $2 AND $3 \
             ORDER BY te.entry_date LIMIT 1",
        )
        .bind(user_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_optional(&mut **tx)
        .await?;
        if conflict.is_some() {
            return Err(AppError::bad_request(
                "Non-sick absences cannot overlap days with logged time. \
                 Please remove or reject the time entries first.",
            ));
        }
        Ok(())
    }

    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }

    /// Return the error if any active absence for this user overlaps `[start, end]`.
    /// Pass `exclude_id` when editing an existing absence so it is not counted against itself.
    pub async fn assert_no_overlap_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        start_date: NaiveDate,
        end_date: NaiveDate,
        exclude_id: Option<i64>,
    ) -> AppResult<()> {
        let count: i64 = if let Some(excl) = exclude_id {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM absences WHERE id != $1 AND user_id=$2 \
                 AND status IN ('requested','approved','cancellation_pending') \
                 AND end_date >= $3 AND start_date <= $4",
            )
            .bind(excl)
            .bind(user_id)
            .bind(start_date)
            .bind(end_date)
            .fetch_one(&mut **tx)
            .await?
        } else {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM absences WHERE user_id=$1 \
                 AND status IN ('requested','approved','cancellation_pending') \
                 AND end_date >= $2 AND start_date <= $3",
            )
            .bind(user_id)
            .bind(start_date)
            .bind(end_date)
            .fetch_one(&mut **tx)
            .await?
        };
        if count > 0 {
            return Err(AppError::conflict("Overlap with existing absence."));
        }
        Ok(())
    }

    /// Insert a new absence row and return the generated ID.
    pub async fn insert_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        category_id: i64,
        start_date: NaiveDate,
        end_date: NaiveDate,
        comment: Option<&str>,
        initial_status: &str,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "INSERT INTO absences(user_id, category_id, start_date, end_date, comment, status) \
             VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
        )
        .bind(user_id)
        .bind(category_id)
        .bind(start_date)
        .bind(end_date)
        .bind(comment)
        .bind(initial_status)
        .fetch_one(&mut **tx)
        .await?)
    }

    /// Update mutable fields of a pending absence (resets review metadata).
    pub async fn update_fields_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        absence_id: i64,
        category_id: i64,
        start_date: NaiveDate,
        end_date: NaiveDate,
        comment: Option<&str>,
        new_status: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE absences SET category_id=$1, start_date=$2, end_date=$3, comment=$4, \
             status=$5, reviewed_by=NULL, reviewed_at=NULL, rejection_reason=NULL \
             WHERE id=$6",
        )
        .bind(category_id)
        .bind(start_date)
        .bind(end_date)
        .bind(comment)
        .bind(new_status)
        .bind(absence_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Cancel a still-requested absence (user-initiated withdrawal, no review needed).
    pub async fn cancel_requested_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        absence_id: i64,
    ) -> AppResult<()> {
        sqlx::query("UPDATE absences SET status='cancelled' WHERE id=$1")
            .bind(absence_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Return the `before_data` JSON from the most recent 'updated' audit log entry
    /// for this absence. Returns `None` when no such entry exists (e.g. first request).
    pub async fn latest_update_before_data(
        pool: &DatabasePool,
        absence_id: i64,
    ) -> AppResult<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT before_data FROM audit_log \
             WHERE table_name='absences' AND record_id=$1 AND action='updated' \
             ORDER BY occurred_at DESC LIMIT 1",
        )
        .bind(absence_id)
        .fetch_optional(pool)
        .await?
        .flatten())
    }

    /// Batch version of `latest_update_before_data` for multiple absence IDs.
    /// Returns a map from absence_id to the most recent 'updated' before_data JSON.
    pub async fn latest_update_before_data_batch(
        pool: &DatabasePool,
        absence_ids: &[i64],
    ) -> AppResult<std::collections::HashMap<i64, String>> {
        if absence_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT DISTINCT ON (record_id) record_id, before_data \
             FROM audit_log \
             WHERE table_name='absences' AND record_id = ANY($1) AND action='updated' \
             ORDER BY record_id, occurred_at DESC",
        )
        .bind(absence_ids)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, data)| data.map(|d| (id, d)))
            .collect())
    }

    /// Carryover expiry setting (used in vacation balance calculation).
    pub async fn carryover_expiry_setting(&self) -> AppResult<String> {
        Ok(
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key='carryover_expiry_date'")
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or_else(|| "03-31".to_string()),
        )
    }

    /// Effective annual leave entitlement from the `user_annual_leave` table.
    pub async fn effective_annual_days(&self, user_id: i64, year: i32) -> AppResult<i64> {
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT days FROM user_annual_leave WHERE user_id=$1 AND year=$2")
                .bind(user_id)
                .bind(year)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(d) = existing {
            return Ok(d);
        }
        let default: i64 = sqlx::query_scalar(
            "SELECT COALESCE(value::BIGINT, 30) FROM app_settings \
             WHERE key='default_annual_leave_days'",
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(30);
        Ok(default)
    }
}
