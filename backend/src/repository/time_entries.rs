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
        use chrono::TimeZone;
        let dt = tz
            .from_local_datetime(&d.and_hms_opt(23, 59, 59).unwrap())
            .single()
            .unwrap_or_else(|| Utc::now().with_timezone(&tz));
        return Ok(dt);
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
     submitted_at, reviewed_by, reviewed_at, rejection_reason, created_at, updated_at \
     FROM time_entries";

/// Validate that a new/updated time entry is acceptable.
/// Called within a transaction; `exclude_id` skips the entry being edited.
pub(crate) async fn validate_entry(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    te: &NewEntryData,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    if let Some(c) = &te.comment {
        if c.len() > 2000 {
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
    let cat_state: Option<(bool, bool)> =
        sqlx::query_as("SELECT active, counts_as_work FROM categories WHERE id = $1")
            .bind(te.category_id)
            .fetch_optional(&mut *conn)
            .await?;
    if cat_state.is_none() {
        return Err(AppError::bad_request("Category not found."));
    }
    let (cat_active, new_counts_as_work) = cat_state.unwrap();
    if !cat_active {
        return Err(AppError::bad_request("Category is inactive."));
    }
    let app_now = app_now(conn).await?;
    let today = app_now.date_naive();
    if te.entry_date > today {
        return Err(AppError::bad_request(
            "Entries in the future are not allowed.",
        ));
    }
    // Validate that end is strictly after start.
    let _ = duration_min(&te.start_time, &te.end_time)?;
    let start_n = parse_time(&te.start_time)?;
    let end_n = parse_time(&te.end_time)?;
    if te.entry_date == today && end_n > app_now.time() {
        return Err(AppError::bad_request("End time cannot be in the future."));
    }

    let existing: Vec<(i64, String, String, String, bool)> = sqlx::query_as(
        "SELECT te.id, te.start_time, te.end_time, te.status, c.counts_as_work \
         FROM time_entries te JOIN categories c ON c.id = te.category_id \
         WHERE te.user_id=$1 AND te.entry_date=$2",
    )
    .bind(user_id)
    .bind(te.entry_date)
    .fetch_all(&mut *conn)
    .await?;

    let mut parsed_existing: Vec<(bool, NaiveTime, NaiveTime)> = Vec::new();
    for (eid, start_str, end_str, status, counts_as_work) in &existing {
        if Some(*eid) == exclude_id || status == "rejected" {
            continue;
        }
        let es = parse_time(start_str)?;
        let ee = parse_time(end_str)?;
        parsed_existing.push((*counts_as_work, es, ee));
    }

    for (_, es, ee) in &parsed_existing {
        if start_n < *ee && *es < end_n {
            return Err(AppError::bad_request("Overlap with an existing entry."));
        }
    }

    let mut credited_intervals: Vec<(NaiveTime, NaiveTime)> = Vec::new();
    if new_counts_as_work {
        credited_intervals.push((start_n, end_n));
    }
    for (counts_as_work, es, ee) in &parsed_existing {
        if *counts_as_work {
            credited_intervals.push((*es, *ee));
        }
    }
    credited_intervals.sort_by_key(|(start, _)| *start);
    let mut day_total = 0_i64;
    let mut merged: Option<(NaiveTime, NaiveTime)> = None;
    for (start, end) in credited_intervals {
        if let Some((cur_start, cur_end)) = merged {
            if start <= cur_end {
                merged = Some((cur_start, cur_end.max(end)));
            } else {
                day_total += (cur_end - cur_start).num_minutes();
                merged = Some((start, end));
            }
        } else {
            merged = Some((start, end));
        }
    }
    if let Some((cur_start, cur_end)) = merged {
        day_total += (cur_end - cur_start).num_minutes();
    }
    if day_total > 14 * 60 {
        return Err(AppError::bad_request("Day total exceeds 14 hours."));
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
    category_id: i64,
    comment: Option<String>,
    status: String,
    counts_as_work: bool,
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
        "SELECT te.id, te.entry_date, te.start_time, te.end_time, te.category_id, \
                te.comment, te.status, c.counts_as_work \
         FROM time_entries te \
         JOIN categories c ON c.id = te.category_id \
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

    for entry in &affected_entries {
        let effective_entry = NewEntryData {
            entry_date: entry.entry_date,
            start_time: entry.start_time.clone(),
            end_time: entry.end_time.clone(),
            category_id: entry.category_id,
            comment: entry.comment.clone(),
        };
        validate_entry(conn, user_id, &effective_entry, Some(entry.id)).await?;
    }

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
        "SELECT te.id, te.entry_date, te.start_time, te.end_time, te.category_id, \
                te.comment, te.status, c.counts_as_work \
         FROM time_entries te \
         JOIN categories c ON c.id = te.category_id \
         WHERE te.user_id=$1 AND te.entry_date = ANY($2) \
         ORDER BY te.entry_date, te.start_time, te.id",
    )
    .bind(user_id)
    .bind(&affected_dates)
    .fetch_all(&mut *conn)
    .await?;

    let mut entries_by_date: BTreeMap<NaiveDate, Vec<(bool, NaiveTime, NaiveTime)>> =
        BTreeMap::new();
    for entry in date_entries {
        if entry.status == "rejected" && !affected_id_set.contains(&entry.id) {
            continue;
        }
        entries_by_date.entry(entry.entry_date).or_default().push((
            entry.counts_as_work,
            parse_time(&entry.start_time)?,
            parse_time(&entry.end_time)?,
        ));
    }

    for entries in entries_by_date.values_mut() {
        entries.sort_by_key(|(_, start, end)| (*start, *end));
        for window in entries.windows(2) {
            let (_, _, previous_end) = window[0];
            let (_, next_start, _) = window[1];
            if next_start < previous_end {
                return Err(AppError::bad_request(
                    "Editing would create overlapping draft entries.",
                ));
            }
        }

        let mut credited_intervals: Vec<(NaiveTime, NaiveTime)> = entries
            .iter()
            .filter_map(|(counts_as_work, start, end)| counts_as_work.then_some((*start, *end)))
            .collect();
        credited_intervals.sort_by_key(|(start, _)| *start);

        let mut day_total = 0_i64;
        let mut merged: Option<(NaiveTime, NaiveTime)> = None;
        for (start, end) in credited_intervals {
            if let Some((cur_start, cur_end)) = merged {
                if start <= cur_end {
                    merged = Some((cur_start, cur_end.max(end)));
                } else {
                    day_total += (cur_end - cur_start).num_minutes();
                    merged = Some((start, end));
                }
            } else {
                merged = Some((start, end));
            }
        }
        if let Some((cur_start, cur_end)) = merged {
            day_total += (cur_end - cur_start).num_minutes();
        }
        if day_total > 14 * 60 {
            return Err(AppError::bad_request(
                "Editing would exceed the 14 hour day limit.",
            ));
        }
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
        // Always exclude entries from users who have time tracking disabled.
        // Their historical rows are kept immutably but must not surface in any
        // team or approval view.
        let mut builder = QueryBuilder::<Postgres>::new(format!(
            "{TE_SELECT} WHERE user_id IN (SELECT id FROM users WHERE tracks_time=TRUE)"
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

    /// Revert all submitted entries for a user back to draft. Used when
    /// time tracking is disabled so they don't linger in the approval queue
    /// and don't reappear if tracking is later re-enabled.
    pub async fn revert_submitted_to_draft_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE time_entries SET status='draft' WHERE user_id=$1 AND status='submitted'",
        )
        .bind(user_id)
        .execute(tx)
        .await?;
        Ok(())
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

    /// Returns the distinct entry dates for the given entry IDs.
    /// A single query replaces the previous N+1 per-entry date fetches.
    pub async fn entry_dates_for_ids(&self, ids: &[i64]) -> AppResult<Vec<NaiveDate>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        Ok(
            sqlx::query_scalar("SELECT DISTINCT entry_date FROM time_entries WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?,
        )
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
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM time_entries te \
                         WHERE te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
                         AND te.status IN ('submitted','approved','rejected')",
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

    /// Atomically mark a batch of entries as submitted for a specific user.
    /// Returns the IDs that were actually transitioned from draft → submitted.
    pub async fn submit_batch(&self, user_id: i64, ids: &[i64]) -> AppResult<Vec<i64>> {
        let mut tx = self.pool.begin().await?;
        let mut submitted = Vec::new();
        for &id in ids {
            let rows = sqlx::query(
                "UPDATE time_entries \
                 SET status='submitted', submitted_at=CURRENT_TIMESTAMP \
                 WHERE id=$1 AND status='draft' AND user_id=$2",
            )
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if rows > 0 {
                submitted.push(id);
            }
        }
        tx.commit().await?;
        Ok(submitted)
    }

    /// Atomically mark a batch of draft entries as approved for a specific user,
    /// bypassing the 'submitted' stop entirely (draft -> approved directly).
    /// Used when the user has `allow_submission_without_approval=TRUE`: the
    /// system is the implicit reviewer, so `reviewed_by` is set to the user's
    /// own id. Returns the IDs that were actually transitioned.
    pub async fn submit_batch_auto_approved(&self, user_id: i64, ids: &[i64]) -> AppResult<Vec<i64>> {
        let mut tx = self.pool.begin().await?;
        let mut approved = Vec::new();
        for &id in ids {
            let rows = sqlx::query(
                "UPDATE time_entries \
                 SET status='approved', submitted_at=CURRENT_TIMESTAMP, \
                     reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP \
                 WHERE id=$2 AND status='draft' AND user_id=$1",
            )
            .bind(user_id)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if rows > 0 {
                approved.push(id);
            }
        }
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
                     rejection_reason=$2 \
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

    // ── Private helpers ────────────────────────────────────────────────────

    /// For submission-style checks: all entries by user in range grouped by month.
    pub async fn get_monthly_submission_stats(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(i32, i32, i64, i64)>> {
        Ok(sqlx::query_as::<_, (i32, i32, i64, i64)>(
            "SELECT \
                 EXTRACT(YEAR FROM entry_date)::int AS y, \
                 EXTRACT(MONTH FROM entry_date)::int AS m, \
                 COUNT(*) AS total, \
                                 COUNT(*) FILTER (WHERE status NOT IN ('submitted','approved')) AS incomplete \
                         FROM time_entries \
                         WHERE user_id = $1 AND entry_date >= $2 AND entry_date <= $3 \
             GROUP BY y, m",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    pub fn parse_time_pub(s: &str) -> AppResult<NaiveTime> {
        parse_time(s)
    }
}
