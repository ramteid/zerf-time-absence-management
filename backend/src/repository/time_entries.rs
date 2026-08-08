use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::time_calc;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder};
use std::collections::{BTreeMap, HashSet};

async fn app_now(conn: &mut sqlx::PgConnection) -> AppResult<chrono::DateTime<chrono_tz::Tz>> {
    let timezone: Option<String> =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'timezone'")
            .fetch_optional(&mut *conn)
            .await?;
    let tz_name =
        timezone.unwrap_or_else(|| crate::services::settings::DEFAULT_TIMEZONE.to_string());
    let tz = tz_name
        .parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::Europe::Berlin);
    if let Some(d) = crate::services::settings::pinned_test_date() {
        // Pin to end-of-day on the reference date so entries for that date
        // are never rejected for having an end_time in the "future".
        // Use earliest()/latest() to handle DST gaps/ambiguous times
        // deterministically instead of falling back to Utc::now().
        use chrono::TimeZone;
        let naive = d.and_hms_opt(23, 59, 59).unwrap();
        let resolved = tz
            .from_local_datetime(&naive)
            .earliest()
            .or_else(|| tz.from_local_datetime(&naive).latest())
            .unwrap_or_else(|| Utc::now().with_timezone(&tz));
        return Ok(resolved);
    }
    Ok(Utc::now().with_timezone(&tz))
}

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct TimeEntry {
    pub id: i64,
    pub user_id: i64,
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub rejection_resolved_at: Option<DateTime<Utc>>,
    pub rejection_resolved_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating or updating a time entry.
#[derive(Deserialize, Clone)]
pub struct NewEntryData {
    pub entry_date: NaiveDate,
    pub start_time: String,
    pub end_time: String,
    pub category_id: i64,
    pub comment: Option<String>,
}

fn parse_time(s: &str) -> AppResult<NaiveTime> {
    time_calc::parse_input_time(s)
}

fn duration_min(start: &str, end: &str) -> AppResult<i64> {
    let s = parse_time(start)?;
    let e = parse_time(end)?;
    if e <= s {
        return Err(AppError::bad_request("End time must be after start time."));
    }
    Ok((e - s).num_minutes())
}

const TE_SELECT: &str =
    "SELECT id, user_id, entry_date, start_time, end_time, category_id, comment, status, \
     submitted_at, reviewed_by, reviewed_at, rejection_reason, \
     rejection_resolved_at, rejection_resolved_by, created_at, updated_at \
     FROM time_entries";

// Rejected entries are explicit workflow items. They stay active until an
// approved overlapping correction closes them by setting rejection_resolved_at.
// A closed rejected row remains visible as history, but no longer poisons
// completeness checks or week reopen selection.
pub(crate) const EFFECTIVE_REJECTED_TIME_ENTRY_CONDITION: &str = "\
    te.status = 'rejected' \
    AND te.rejection_resolved_at IS NULL";

pub(crate) const INCOMPLETE_TIME_ENTRY_CONDITION: &str = "\
    te.status NOT IN ('submitted','approved') \
    AND (\
        te.status <> 'rejected' \
        OR te.rejection_resolved_at IS NULL\
    )";

pub(crate) const REOPENABLE_TIME_ENTRY_CONDITION: &str = "\
    te.status IN ('submitted','approved') \
    OR (\
        te.status = 'rejected' \
        AND te.rejection_resolved_at IS NULL\
    )";

/// Validate that a new/updated time entry is acceptable.
/// Called within a transaction; `exclude_id` skips the entry being edited.
pub(crate) async fn validate_entry(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    te: &NewEntryData,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    if let Some(c) = &te.comment {
        if c.chars().count() > 2000 {
            return Err(AppError::bad_request("Comment too long (max 2000)."));
        }
    }
    let user_start: NaiveDate = sqlx::query_scalar("SELECT start_date FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await?;
    if te.entry_date < user_start {
        return Err(AppError::bad_request(
            "Entry date is before user start date.",
        ));
    }
    // Only the category's active flag matters here. Whether it counts as work is
    // irrelevant now that there is no per-day hour cap to compute.
    let category_active: Option<bool> =
        sqlx::query_scalar("SELECT active FROM categories WHERE id = $1")
            .bind(te.category_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(category_active) = category_active else {
        return Err(AppError::bad_request("Category not found."));
    };
    if !category_active {
        return Err(AppError::bad_request("Category is inactive."));
    }
    let app_now = app_now(conn).await?;
    let today = app_now.date_naive();
    if te.entry_date > today {
        return Err(AppError::bad_request(
            "Entries in the future are not allowed.",
        ));
    }
    // Public holidays are intentionally NOT blocked here: like the documented
    // sick-day exception, someone may still work (or be on call) on a public
    // holiday. The day's target is 0 regardless (see reports::is_contract_workday
    // / holiday_map), so any logged hours become a pure flextime gain with the
    // same "day off, but you worked anyway" mechanics as the sick-leave case.
    // Validate that end is strictly after start.
    let _ = duration_min(&te.start_time, &te.end_time)?;
    let start_n = parse_time(&te.start_time)?;
    let end_n = parse_time(&te.end_time)?;
    if te.entry_date == today {
        let now_time = app_now.time();
        if start_n > now_time {
            return Err(AppError::bad_request("Start time cannot be in the future."));
        }
        if end_n > now_time {
            return Err(AppError::bad_request("End time cannot be in the future."));
        }
    }

    // Overlap check: an entry may not share wall-clock minutes with any other
    // entry on the same day. Two entries claiming the same minutes is always a
    // data error, so this holds regardless of whether either category counts as
    // work (rejected entries are ignored; the entry being edited is excluded via
    // exclude_id).
    //
    // Zerf deliberately does NOT validate the length of an entry or cap the daily
    // total. There is no maximum-hours-per-day rule: assistants ("Aushilfe") have
    // no target and no flextime account and are simply paid for the hours they
    // work, while regular employees may legitimately log long or on-call days.
    // Keeping it simple, the only time-range rules are "end after start", "not in
    // the future", and "no overlap".
    let existing: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT te.id, te.start_time, te.end_time, te.status \
         FROM time_entries te \
         WHERE te.user_id=$1 AND te.entry_date=$2",
    )
    .bind(user_id)
    .bind(te.entry_date)
    .fetch_all(&mut *conn)
    .await?;

    for (eid, start_str, end_str, status) in &existing {
        if Some(*eid) == exclude_id || status == "rejected" {
            continue;
        }
        let es = parse_time(start_str)?;
        let ee = parse_time(end_str)?;
        if start_n < ee && es < end_n {
            return Err(AppError::bad_request("Overlap with an existing entry."));
        }
    }
    // Block entry creation on any day covered by a non-auto-approve-past absence
    // that is requested, approved, or pending cancellation. Including 'requested'
    // prevents a deadlock where entries added after requesting an absence make the
    // absence impossible to approve (ensure_no_time_conflict_tx blocks approval
    // when entries exist). Auto-approve-past (sick-like) categories are excluded
    // so partial-day overlaps remain possible.
    let absence_on_day: Option<String> = sqlx::query_scalar(
        "SELECT c.slug FROM absences a \
         JOIN absence_categories c ON c.id = a.category_id \
         WHERE a.user_id=$1 AND a.status IN ('approved','cancellation_pending','requested') \
         AND a.start_date <= $2 AND a.end_date >= $2 \
         AND c.auto_approve_past = FALSE LIMIT 1",
    )
    .bind(user_id)
    .bind(te.entry_date)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(kind) = absence_on_day {
        return Err(AppError::bad_request(format!(
            "Cannot log time on a day with an approved absence ({kind}). \
             Please cancel or adjust the absence first."
        )));
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ReopenValidationEntry {
    id: i64,
    entry_date: NaiveDate,
    start_time: String,
    end_time: String,
    status: String,
    rejection_resolved_at: Option<DateTime<Utc>>,
}

pub(crate) async fn validate_entries_after_reopen(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    affected_entry_ids: &[i64],
) -> AppResult<()> {
    if affected_entry_ids.is_empty() {
        return Ok(());
    }

    let affected_id_set: HashSet<i64> = affected_entry_ids.iter().copied().collect();
    let affected_entries: Vec<ReopenValidationEntry> = sqlx::query_as(
        "SELECT te.id, te.entry_date, te.start_time, te.end_time, te.status, te.rejection_resolved_at \
         FROM time_entries te \
         WHERE te.user_id=$1 AND te.id = ANY($2) \
         FOR UPDATE OF te",
    )
    .bind(user_id)
    .bind(affected_entry_ids)
    .fetch_all(&mut *conn)
    .await?;

    if affected_entries.len() != affected_id_set.len() {
        return Err(AppError::conflict(
            "Reopen target entries changed concurrently.",
        ));
    }

    // Reopened entries already exist and are only having their status reset to
    // draft — their date/time/category values are unchanged. Re-running the
    // full `validate_entry` creation checks here (absence-on-day conflict,
    // category-active, entry-date >= user start_date, entry-date <= today)
    // would reject historical rows for conditions that are only meaningful
    // when *creating* new data: a rejected entry whose category was later
    // deactivated, or one that now overlaps an approved absence, must still
    // be reopenable so it can be edited or deleted. Only the one invariant that
    // is still meaningful for a bulk status change is re-checked below: same-day
    // overlap, since resurrecting rejected entries can newly collide with drafts
    // created in the meantime. (There is no per-day hour cap to re-check — Zerf
    // never limits how many hours a day may hold.)
    let mut affected_dates: Vec<NaiveDate> = affected_entries
        .iter()
        .map(|entry| entry.entry_date)
        .collect();
    affected_dates.sort_unstable();
    affected_dates.dedup();
    if affected_dates.is_empty() {
        return Ok(());
    }

    let date_entries: Vec<ReopenValidationEntry> = sqlx::query_as(
        "SELECT te.id, te.entry_date, te.start_time, te.end_time, te.status, te.rejection_resolved_at \
         FROM time_entries te \
         WHERE te.user_id=$1 AND te.entry_date = ANY($2) \
         ORDER BY te.entry_date, te.start_time, te.id",
    )
    .bind(user_id)
    .bind(&affected_dates)
    .fetch_all(&mut *conn)
    .await?;

    let mut entries_by_date: BTreeMap<NaiveDate, Vec<(NaiveTime, NaiveTime)>> = BTreeMap::new();
    for entry in date_entries {
        if entry.status == "rejected" {
            // Resolved rejected are pure history and never block.
            if entry.rejection_resolved_at.is_some() {
                continue;
            }
            // Unresolved rejected that are not being reopened remain rejected and are allowed to overlap drafts
            // (validate_entry skips rejected). They must not poison a reopen of other weeks.
            if !affected_id_set.contains(&entry.id) {
                continue;
            }
        }
        entries_by_date
            .entry(entry.entry_date)
            .or_default()
            .push((parse_time(&entry.start_time)?, parse_time(&entry.end_time)?));
    }

    for entries in entries_by_date.values_mut() {
        entries.sort_by_key(|(start, end)| (*start, *end));
        for window in entries.windows(2) {
            let (_, previous_end) = window[0];
            let (next_start, _) = window[1];
            if next_start < previous_end {
                return Err(AppError::bad_request(
                    "Editing would create overlapping draft entries.",
                ));
            }
        }
    }

    Ok(())
}

async fn validate_entries_do_not_overlap_blocking_absences(
    conn: &mut sqlx::PgConnection,
    entry_ids: &[i64],
    action: &str,
) -> AppResult<()> {
    if entry_ids.is_empty() {
        return Ok(());
    }

    // Only `approved` and `cancellation_pending` absences block submission and
    // approval.  `requested` absences are deliberately excluded: the conflict
    // resolution flow allows employees to submit entries and lets the approver
    // handle any overlap when deciding the absence.  Blocking on `requested`
    // would let any unresolved absence request jam the employee's ability to
    // submit a week, and would cause the approver's batch-approve to error on
    // a conflict that was created after submission - handing employees a lever
    // to block approvals indefinitely.
    let conflict: Option<(i64, NaiveDate, String)> = sqlx::query_as(
        "SELECT te.id, te.entry_date, c.slug \
         FROM time_entries te \
         JOIN absences a ON a.user_id = te.user_id \
             AND a.status IN ('approved','cancellation_pending') \
             AND a.start_date <= te.entry_date \
             AND a.end_date >= te.entry_date \
         JOIN absence_categories c ON c.id = a.category_id \
         WHERE te.id = ANY($1) \
         AND c.auto_approve_past = FALSE \
         ORDER BY te.entry_date, te.id \
         LIMIT 1",
    )
    .bind(entry_ids)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some((entry_id, entry_date, kind)) = conflict {
        return Err(AppError::bad_request(format!(
            "Cannot {action} time entry {entry_id} on {entry_date}: the day has a blocking absence ({kind}). Delete or adjust the entry first."
        )));
    }

    Ok(())
}

#[derive(Clone)]
pub struct TimeEntryDb {
    pool: DatabasePool,
}

impl TimeEntryDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    async fn resolve_overlapping_rejected_entries_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        resolver_id: i64,
        replacement_entry_ids: &[i64],
    ) -> AppResult<()> {
        if replacement_entry_ids.is_empty() {
            return Ok(());
        }

        sqlx::query(
            "UPDATE time_entries rejected \
             SET rejection_resolved_at=CURRENT_TIMESTAMP, \
                 rejection_resolved_by=$2, \
                 updated_at=CURRENT_TIMESTAMP \
             FROM time_entries replacement \
             WHERE replacement.id = ANY($3) \
             AND replacement.user_id=$1 \
             AND replacement.status='approved' \
             AND rejected.user_id=$1 \
             AND rejected.status='rejected' \
             AND rejected.rejection_resolved_at IS NULL \
             AND rejected.id <> replacement.id \
             AND rejected.entry_date = replacement.entry_date \
             AND replacement.start_time::time < rejected.end_time::time \
             AND replacement.end_time::time > rejected.start_time::time",
        )
        .bind(user_id)
        .bind(resolver_id)
        .bind(replacement_entry_ids)
        .execute(tx)
        .await?;

        Ok(())
    }

    // ── Queries ────────────────────────────────────────────────────────────

    pub async fn list_for_user(
        &self,
        user_id: i64,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> AppResult<Vec<TimeEntry>> {
        let mut builder = QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE user_id = "));
        builder.push_bind(user_id);
        // Never show entries before the user's start_date. This is the natural lower
        // bound for any user, and critically ensures that when a pure-admin re-enables
        // time tracking (resetting start_date to today), historical entries from the
        // prior tracking period are silently excluded rather than deleted.
        builder
            .push(" AND entry_date >= (SELECT start_date FROM users WHERE id=")
            .push_bind(user_id)
            .push(")");
        if let Some(f) = from {
            builder.push(" AND entry_date >= ").push_bind(f);
        }
        if let Some(t) = to {
            builder.push(" AND entry_date <= ").push_bind(t);
        }
        builder.push(" ORDER BY entry_date, start_time");
        Ok(builder
            .build_query_as::<TimeEntry>()
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn list_all(
        &self,
        is_admin: bool,
        requester_id: i64,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        user_id_filter: Option<i64>,
        status_filter: Option<String>,
    ) -> AppResult<Vec<TimeEntry>> {
        // Always exclude entries from users who have time tracking disabled or
        // are archived. Their historical rows are kept immutably but must not
        // surface in any team or approval view (the handler already rejects
        // explicit user_id filters targeting inactive users; this keeps the
        // unfiltered listing consistent with that rule).
        // Also hide entries before the owner's start_date (same as list_for_user) so re-enabled tracking doesn't leak history.
        let mut builder = QueryBuilder::<Postgres>::new(format!(
            "{TE_SELECT} WHERE user_id IN (SELECT id FROM users WHERE tracks_time=TRUE AND active=TRUE) AND entry_date >= (SELECT start_date FROM users WHERE id = time_entries.user_id)"
        ));
        if !is_admin {
            // Non-admin leads: team members only (active, non-admin direct reports).
            // Own entries are unconditionally excluded — leads cannot act on their own
            // submissions and the endpoint is a team-management view, not self+team.
            // Callers that need the lead's own entries should use /time-entries instead.
            builder
                .push(" AND user_id IN (SELECT ua.user_id FROM user_approvers ua JOIN users u ON u.id=ua.user_id WHERE ua.approver_id = ")
                .push_bind(requester_id)
                .push(" AND u.active=TRUE AND u.role != 'admin')");
        }
        if let Some(f) = from {
            builder.push(" AND entry_date >= ").push_bind(f);
        }
        if let Some(t) = to {
            builder.push(" AND entry_date <= ").push_bind(t);
        }
        if let Some(uid) = user_id_filter {
            builder.push(" AND user_id = ").push_bind(uid);
        }
        if let Some(s) = status_filter {
            // Non-crediting entries fully participate in the approval workflow, so no
            // counts_as_work filter here — the approval queue must show all submitted
            // entries regardless of category.
            builder.push(" AND status = ").push_bind(s);
        }
        builder.push(" ORDER BY entry_date DESC, start_time");
        Ok(builder
            .build_query_as::<TimeEntry>()
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_by_id(&self, id: i64) -> AppResult<TimeEntry> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1"))
                .build_query_as::<TimeEntry>()
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn find_by_id_opt(&self, id: i64) -> AppResult<Option<TimeEntry>> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1"))
                .build_query_as::<TimeEntry>()
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Revert all submitted entries for a user back to draft and return the
    /// affected week starts so callers can clear pending review notifications
    /// after their transaction commits.
    pub async fn revert_submitted_to_draft_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<Vec<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "UPDATE time_entries \
             SET status='draft', submitted_at=NULL, reviewed_by=NULL, \
                 reviewed_at=NULL, rejection_reason=NULL, \
                 rejection_resolved_at=NULL, rejection_resolved_by=NULL, \
                 updated_at=CURRENT_TIMESTAMP \
             WHERE user_id=$1 AND status='submitted' \
             RETURNING entry_date",
        )
        .bind(user_id)
        .fetch_all(tx)
        .await?;
        let mut weeks: Vec<NaiveDate> = rows
            .into_iter()
            .map(|(entry_date,)| crate::time_calc::week_monday(entry_date))
            .collect();
        weeks.sort();
        weeks.dedup();
        Ok(weeks)
    }

    pub async fn find_by_id_for_update(
        tx: &mut sqlx::PgConnection,
        id: i64,
    ) -> AppResult<TimeEntry> {
        Ok(
            QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1 FOR UPDATE"))
                .build_query_as::<TimeEntry>()
                .bind(id)
                .fetch_one(tx)
                .await?,
        )
    }

    pub async fn get_user_id(&self, id: i64) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM time_entries WHERE id=$1")
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// Check whether `user_id` is a non-admin direct report of `approver_id`
    /// (with row lock for use inside transactions).
    pub async fn check_direct_report_for_update(
        tx: &mut sqlx::PgConnection,
        subject_user_id: i64,
        approver_id: i64,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, Option<bool>>(
            "SELECT TRUE FROM user_approvers ua \
             WHERE ua.user_id=$1 AND ua.approver_id=$2 \
             AND EXISTS (SELECT 1 FROM users u WHERE u.id=$1 AND u.active=TRUE AND u.role != 'admin') \
             FOR UPDATE",
        )
        .bind(subject_user_id)
        .bind(approver_id)
        .fetch_optional(tx)
        .await?
        .flatten()
        .is_some())
    }

    pub async fn get_date_for_entry(&self, id: i64) -> AppResult<Option<NaiveDate>> {
        Ok(
            sqlx::query_scalar("SELECT entry_date FROM time_entries WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Returns true if every entry in `ids` is owned by `user_id`.
    /// A single query replaces the previous N+1 per-entry ownership loop.
    pub async fn all_entries_owned_by_user(&self, ids: &[i64], user_id: i64) -> AppResult<bool> {
        if ids.is_empty() {
            return Ok(true);
        }
        let unowned: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries WHERE id = ANY($1) AND user_id != $2",
        )
        .bind(ids)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(unowned == 0)
    }

    pub async fn get_credited_submitted_dates_for_entries(
        &self,
        user_id: i64,
        ids: &[i64],
    ) -> AppResult<Vec<NaiveDate>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        Ok(sqlx::query_scalar(
            "SELECT te.entry_date FROM time_entries te \
                         WHERE te.user_id = $1 AND te.id = ANY($2) \
                         AND te.status = 'submitted'",
        )
        .bind(user_id)
        .bind(ids)
        .fetch_all(&self.pool)
        .await?)
    }

    // ── Count helpers for reopen/submission checks ─────────────────────────

    pub async fn count_non_draft_in_week(
        &self,
        user_id: i64,
        week_start: NaiveDate,
        week_end: NaiveDate,
    ) -> AppResult<i64> {
        // Exclude resolved rejected entries – they are pure history and must not
        // make a week count as non-draft.
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries te \
                         WHERE te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
                         AND (te.status IN ('submitted','approved') \
                              OR (te.status='rejected' AND te.rejection_resolved_at IS NULL))",
        )
        .bind(user_id)
        .bind(week_start)
        .bind(week_end)
        .fetch_one(&self.pool)
        .await?)
    }

    // ── Mutations ──────────────────────────────────────────────────────────

    pub async fn create(&self, user_id: i64, entry: &NewEntryData) -> AppResult<TimeEntry> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        validate_entry(&mut tx, user_id, entry, None).await?;
        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO time_entries(user_id, entry_date, start_time, end_time, \
             category_id, comment) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
        )
        .bind(user_id)
        .bind(entry.entry_date)
        .bind(&entry.start_time)
        .bind(&entry.end_time)
        .bind(entry.category_id)
        .bind(&entry.comment)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(
            QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1"))
                .build_query_as::<TimeEntry>()
                .bind(new_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn update(
        &self,
        entry_id: i64,
        requester_id: i64,
        requester_is_admin: bool,
        entry: &NewEntryData,
    ) -> AppResult<(TimeEntry, TimeEntry)> {
        let owner_id: i64 = sqlx::query_scalar("SELECT user_id FROM time_entries WHERE id=$1")
            .bind(entry_id)
            .fetch_one(&self.pool)
            .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(owner_id)
            .execute(&mut *tx)
            .await?;
        let prev: TimeEntry =
            QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1 FOR UPDATE"))
                .build_query_as::<TimeEntry>()
                .bind(entry_id)
                .fetch_one(&mut *tx)
                .await?;

        let admin_correction = requester_is_admin
            && prev.user_id != requester_id
            && (prev.status == "approved" || prev.status == "submitted");
        if !admin_correction {
            if prev.user_id != requester_id {
                return Err(AppError::forbidden());
            }
            if prev.status != "draft" {
                return Err(AppError::bad_request(
                    "Only draft entries can be edited. Submit a week edit request to make the whole week editable again.",
                ));
            }
        }
        validate_entry(&mut tx, prev.user_id, entry, Some(entry_id)).await?;
        sqlx::query(
            "UPDATE time_entries \
             SET entry_date=$1, start_time=$2, end_time=$3, category_id=$4, \
                 comment=$5, updated_at=CURRENT_TIMESTAMP \
             WHERE id=$6",
        )
        .bind(entry.entry_date)
        .bind(&entry.start_time)
        .bind(&entry.end_time)
        .bind(entry.category_id)
        .bind(&entry.comment)
        .bind(entry_id)
        .execute(&mut *tx)
        .await?;
        if admin_correction && prev.status == "approved" {
            Self::resolve_overlapping_rejected_entries_tx(
                &mut tx,
                prev.user_id,
                requester_id,
                &[entry_id],
            )
            .await?;
        }
        tx.commit().await?;
        let updated: TimeEntry = QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1"))
            .build_query_as::<TimeEntry>()
            .bind(entry_id)
            .fetch_one(&self.pool)
            .await?;
        Ok((prev, updated))
    }

    pub async fn delete(&self, entry_id: i64) -> AppResult<TimeEntry> {
        let owner_id: i64 = sqlx::query_scalar("SELECT user_id FROM time_entries WHERE id=$1")
            .bind(entry_id)
            .fetch_one(&self.pool)
            .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(owner_id)
            .execute(&mut *tx)
            .await?;
        let entry: TimeEntry =
            QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1 FOR UPDATE"))
                .build_query_as::<TimeEntry>()
                .bind(entry_id)
                .fetch_one(&mut *tx)
                .await?;
        if entry.status != "draft" {
            return Err(AppError::bad_request("Only drafts can be deleted."));
        }
        let rows = sqlx::query("DELETE FROM time_entries WHERE id=$1 AND status='draft'")
            .bind(entry_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if rows == 0 {
            return Err(AppError::conflict("Entry was modified concurrently."));
        }
        tx.commit().await?;
        Ok(entry)
    }

    /// Atomically transition a batch of draft entries to `submitted` for a
    /// specific user. Returns the full row for every entry that actually
    /// moved, so callers can audit and requeue reports without a second query.
    pub async fn submit_batch(&self, user_id: i64, ids: &[i64]) -> AppResult<Vec<TimeEntry>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let draft_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM time_entries \
             WHERE id = ANY($1) AND status='draft' AND user_id=$2 \
             ORDER BY id \
             FOR UPDATE",
        )
        .bind(ids)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;
        validate_entries_do_not_overlap_blocking_absences(&mut tx, &draft_ids, "submit").await?;

        let submitted = if draft_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query_as::<_, TimeEntry>(
                "UPDATE time_entries \
                 SET status='submitted', submitted_at=CURRENT_TIMESTAMP \
                 WHERE id = ANY($1) AND status='draft' AND user_id=$2 \
                 RETURNING id, user_id, entry_date, start_time, end_time, category_id, comment, \
                 status, submitted_at, reviewed_by, reviewed_at, rejection_reason, \
                 rejection_resolved_at, rejection_resolved_by, created_at, updated_at",
            )
            .bind(&draft_ids)
            .bind(user_id)
            .fetch_all(&mut *tx)
            .await?
        };
        tx.commit().await?;
        Ok(submitted)
    }

    /// Atomically mark a batch of draft entries as approved for a specific user,
    /// bypassing the 'submitted' stop entirely (draft -> approved directly).
    /// Used when the user has `allow_submission_without_approval=TRUE`: the
    /// system is the implicit reviewer, so `reviewed_by` is set to the user's
    /// own id. Returns the full row for every entry that actually moved, like
    /// [`Self::submit_batch`].
    pub async fn submit_batch_auto_approved(
        &self,
        user_id: i64,
        ids: &[i64],
    ) -> AppResult<Vec<TimeEntry>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let draft_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM time_entries \
             WHERE id = ANY($1) AND status='draft' AND user_id=$2 \
             ORDER BY id \
             FOR UPDATE",
        )
        .bind(ids)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;
        validate_entries_do_not_overlap_blocking_absences(&mut tx, &draft_ids, "approve").await?;

        let approved = if draft_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query_as::<_, TimeEntry>(
                "UPDATE time_entries \
                 SET status='approved', submitted_at=CURRENT_TIMESTAMP, \
                     reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP \
                 WHERE id = ANY($2) AND status='draft' AND user_id=$1 \
                 RETURNING id, user_id, entry_date, start_time, end_time, category_id, comment, \
                 status, submitted_at, reviewed_by, reviewed_at, rejection_reason, \
                 rejection_resolved_at, rejection_resolved_by, created_at, updated_at",
            )
            .bind(user_id)
            .bind(&draft_ids)
            .fetch_all(&mut *tx)
            .await?
        };
        let approved_ids: Vec<i64> = approved.iter().map(|e| e.id).collect();
        Self::resolve_overlapping_rejected_entries_tx(&mut tx, user_id, user_id, &approved_ids)
            .await?;
        tx.commit().await?;
        Ok(approved)
    }

    /// Batch approve submitted entries.
    /// Skips entries whose owner cannot be reviewed by `reviewer_id`.
    /// Returns all entries that were actually approved.
    pub async fn batch_approve(
        &self,
        ids: &[i64],
        reviewer_id: i64,
        reviewer_is_admin: bool,
    ) -> AppResult<Vec<TimeEntry>> {
        let mut tx = self.pool.begin().await?;
        let mut approved = Vec::new();
        let mut ordered_ids = ids.to_vec();
        ordered_ids.sort_unstable();
        ordered_ids.dedup();

        // Collect all distinct owner ids for the requested entries, sort them
        // ascending, and acquire all advisory locks up front before entering
        // the per-entry processing loop.  Lock ordering is the standard deadlock
        // prevention technique: two concurrent batch approvals that touch the
        // same two users' entries will always acquire owner locks in the same
        // order, so they cannot deadlock each other.
        let owner_ids: Vec<i64> = if ordered_ids.is_empty() {
            Vec::new()
        } else {
            let mut ids_sorted: Vec<i64> = sqlx::query_scalar(
                "SELECT DISTINCT user_id FROM time_entries WHERE id = ANY($1) ORDER BY user_id",
            )
            .bind(&ordered_ids)
            .fetch_all(&mut *tx)
            .await?;
            ids_sorted.sort_unstable();
            ids_sorted.dedup();
            ids_sorted
        };
        for owner_id in owner_ids {
            sqlx::query("SELECT pg_advisory_xact_lock($1)")
                .bind(owner_id)
                .execute(&mut *tx)
                .await?;
        }

        for id in ordered_ids {
            let Some(entry) =
                QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1 FOR UPDATE"))
                    .build_query_as::<TimeEntry>()
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?
            else {
                continue;
            };
            if entry.status != "submitted" {
                continue;
            }
            if entry.user_id == reviewer_id && !reviewer_is_admin {
                continue;
            }
            if !reviewer_is_admin {
                let ok = Self::check_direct_report_for_update(&mut tx, entry.user_id, reviewer_id)
                    .await?;
                if !ok {
                    continue;
                }
            }
            validate_entries_do_not_overlap_blocking_absences(&mut tx, &[entry.id], "approve")
                .await?;
            let rows = sqlx::query(
                "UPDATE time_entries \
                 SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP \
                 WHERE id=$2 AND status='submitted'",
            )
            .bind(reviewer_id)
            .bind(entry.id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if rows > 0 {
                approved.push(entry);
            }
        }
        let mut owner_ids_for_resolution: Vec<i64> =
            approved.iter().map(|entry| entry.user_id).collect();
        owner_ids_for_resolution.sort_unstable();
        owner_ids_for_resolution.dedup();
        for owner_id in owner_ids_for_resolution {
            let owner_entry_ids: Vec<i64> = approved
                .iter()
                .filter_map(|entry| (entry.user_id == owner_id).then_some(entry.id))
                .collect();
            Self::resolve_overlapping_rejected_entries_tx(
                &mut tx,
                owner_id,
                reviewer_id,
                &owner_entry_ids,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(approved)
    }

    /// Batch reject submitted entries.
    /// Skips entries the reviewer is not allowed to act on.
    /// Returns all entries that were actually rejected.
    pub async fn batch_reject(
        &self,
        ids: &[i64],
        reviewer_id: i64,
        reviewer_is_admin: bool,
        reason: &str,
    ) -> AppResult<Vec<TimeEntry>> {
        let mut tx = self.pool.begin().await?;
        let mut rejected = Vec::new();
        let mut ordered_ids = ids.to_vec();
        ordered_ids.sort_unstable();
        ordered_ids.dedup();

        // Mirroring batch_approve: collect distinct owners and lock in ascending order to prevent deadlocks.
        let owner_ids: Vec<i64> = if ordered_ids.is_empty() {
            Vec::new()
        } else {
            let mut ids_sorted: Vec<i64> = sqlx::query_scalar(
                "SELECT DISTINCT user_id FROM time_entries WHERE id = ANY($1) ORDER BY user_id",
            )
            .bind(&ordered_ids)
            .fetch_all(&mut *tx)
            .await?;
            ids_sorted.sort_unstable();
            ids_sorted.dedup();
            ids_sorted
        };
        for owner_id in owner_ids {
            sqlx::query("SELECT pg_advisory_xact_lock($1)")
                .bind(owner_id)
                .execute(&mut *tx)
                .await?;
        }

        for id in ordered_ids {
            let Some(entry) =
                QueryBuilder::<Postgres>::new(format!("{TE_SELECT} WHERE id=$1 FOR UPDATE"))
                    .build_query_as::<TimeEntry>()
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?
            else {
                continue;
            };
            if entry.status != "submitted" {
                continue;
            }
            if entry.user_id == reviewer_id && !reviewer_is_admin {
                continue;
            }
            if !reviewer_is_admin {
                let ok = Self::check_direct_report_for_update(&mut tx, entry.user_id, reviewer_id)
                    .await?;
                if !ok {
                    continue;
                }
            }
            let rows = sqlx::query(
                "UPDATE time_entries \
                 SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, \
                     rejection_reason=$2, \
                     rejection_resolved_at=NULL, rejection_resolved_by=NULL \
                 WHERE id=$3 AND status='submitted'",
            )
            .bind(reviewer_id)
            .bind(reason)
            .bind(entry.id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if rows > 0 {
                rejected.push(entry);
            }
        }
        tx.commit().await?;
        Ok(rejected)
    }

    pub async fn get_by_user_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<TimeEntry>> {
        Ok(QueryBuilder::<Postgres>::new(format!(
            "{TE_SELECT} WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3"
        ))
        .build_query_as::<TimeEntry>()
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// True when the user still has ANY entry in `submitted` status.
    /// Used to decide when legacy submitter-scoped submission notifications can
    /// be cleared after week-scoped notifications have already been resolved.
    pub async fn has_any_submitted_entries(&self, user_id: i64) -> AppResult<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries WHERE user_id=$1 AND status='submitted'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn has_submitted_entries_in_week(
        &self,
        user_id: i64,
        week_monday: NaiveDate,
    ) -> AppResult<bool> {
        let week_end = week_monday + chrono::Duration::days(6);
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries \
             WHERE user_id=$1 AND status='submitted' \
             AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(week_monday)
        .bind(week_end)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn get_submitted_dates_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<NaiveDate>> {
        // Submission completeness is workflow-based, not crediting-based: any
        // submitted/approved entry (including non-crediting categories) marks
        // the day as submitted.
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

    pub async fn get_incomplete_dates_in_range(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<NaiveDate>> {
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

    // ── Private helpers ────────────────────────────────────────────────────

    /// For submission-style checks: all entries by user in range grouped by month.
    pub async fn get_monthly_submission_stats(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(i32, i32, i64, i64)>> {
        let sql = format!(
            "SELECT \
                 EXTRACT(YEAR FROM te.entry_date)::int AS y, \
                 EXTRACT(MONTH FROM te.entry_date)::int AS m, \
                 COUNT(*) AS total, \
                 COUNT(*) FILTER (WHERE {INCOMPLETE_TIME_ENTRY_CONDITION}) AS incomplete \
             FROM time_entries te \
             WHERE te.user_id = $1 AND te.entry_date >= $2 AND te.entry_date <= $3 \
             GROUP BY y, m"
        );
        // AssertSqlSafe: the formatted fragment is a compile-time status predicate.
        Ok(
            sqlx::query_as::<_, (i32, i32, i64, i64)>(sqlx::AssertSqlSafe(sql))
                .bind(user_id)
                .bind(from)
                .bind(to)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub fn parse_time_pub(s: &str) -> AppResult<NaiveTime> {
        parse_time(s)
    }
}
