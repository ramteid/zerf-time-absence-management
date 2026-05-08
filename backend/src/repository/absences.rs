use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashSet;

pub const ALLOWED_KINDS: &[&str] = &[
    "vacation",
    "sick",
    "training",
    "special_leave",
    "unpaid",
    "general_absence",
];

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct Absence {
    pub id: i64,
    pub user_id: i64,
    pub kind: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
    pub status: String,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct CalendarEntry {
    pub id: i64,
    pub user_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub kind: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
    pub status: String,
}

const ABS_SELECT: &str =
    "SELECT id, user_id, kind, start_date, end_date, comment, status, \
     reviewed_by, reviewed_at, rejection_reason, created_at FROM absences";

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

    /// Count working days between `from` and `to` (inclusive),
    /// excluding weekends and public holidays.
    pub async fn workdays(&self, from: NaiveDate, to: NaiveDate) -> AppResult<f64> {
        if to < from {
            return Ok(0.0);
        }
        let holidays = self.holidays_set(from, to).await?;
        let mut count = 0.0;
        let mut d = from;
        while d <= to {
            if d.weekday().num_days_from_monday() < 5 && !holidays.contains(&d) {
                count += 1.0;
            }
            d += Duration::days(1);
        }
        Ok(count)
    }

    pub async fn workdays_total(
        &self,
        user_id: i64,
        kind: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<f64> {
        let ranges: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
            "SELECT start_date, end_date FROM absences \
             WHERE user_id=$1 AND kind=$2 AND status='approved' \
             AND end_date >= $3 AND start_date <= $4",
        )
        .bind(user_id)
        .bind(kind)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        let mut total = 0.0;
        for (s, e) in ranges {
            let cs = std::cmp::max(s, from);
            let ce = std::cmp::min(e, to);
            total += self.workdays(cs, ce).await?;
        }
        Ok(total)
    }

    // ── Queries ────────────────────────────────────────────────────────────

    pub async fn find_by_id(&self, id: i64) -> AppResult<Absence> {
        Ok(
            sqlx::query_as::<_, Absence>(&format!("{ABS_SELECT} WHERE id=$1"))
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn get_user_id(&self, id: i64) -> AppResult<i64> {
        Ok(sqlx::query_scalar("SELECT user_id FROM absences WHERE id=$1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn list_for_user(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<Absence>> {
        Ok(sqlx::query_as::<_, Absence>(&format!(
            "{ABS_SELECT} WHERE user_id=$1 AND end_date >= $2 AND start_date <= $3 \
             ORDER BY start_date DESC"
        ))
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
        let mut builder = QueryBuilder::<Postgres>::new(&format!("{ABS_SELECT} WHERE TRUE"));
        if !is_admin {
            builder
                .push(" AND user_id IN (SELECT ua.user_id FROM user_approvers ua JOIN users u ON u.id=ua.user_id WHERE ua.approver_id = ")
                .push_bind(requester_id)
                .push(" AND u.role != 'admin')");
        }
        if let Some(f) = from {
            builder.push(" AND end_date >= ").push_bind(f);
        }
        if let Some(t) = to {
            builder.push(" AND start_date <= ").push_bind(t);
        }
        if let Some(s) = status_filter {
            builder.push(" AND status = ").push_bind(s.to_owned());
        }
        builder.push(" ORDER BY start_date DESC");
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
            let mut reports: Vec<i64> = sqlx::query_scalar(
                "SELECT ua.user_id FROM user_approvers ua \
                 JOIN users u ON u.id = ua.user_id \
                 WHERE ua.approver_id=$1 AND u.active=TRUE AND u.role!='admin'",
            )
            .bind(requester_id)
            .fetch_all(&self.pool)
            .await?;
            ids.append(&mut reports);
        }
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
            "SELECT a.id, a.user_id, u.first_name, u.last_name, a.kind, \
             a.start_date, a.end_date, a.comment, a.status \
             FROM absences a JOIN users u ON u.id=a.user_id \
             WHERE a.status IN ('requested','approved') \
             AND a.end_date >= ",
        );
        builder.push_bind(from);
        builder.push(" AND a.start_date <= ").push_bind(to);
        if let Some(ids) = scope_ids {
            builder.push(" AND a.user_id IN (");
            let mut sep = builder.separated(", ");
            for id in ids {
                sep.push_bind(*id);
            }
            sep.push_unseparated(")");
        }
        builder.push(" ORDER BY a.start_date");
        Ok(builder
            .build_query_as::<CalendarEntry>()
            .fetch_all(&self.pool)
            .await?)
    }

    /// Load vacation absences for balance calculation.
    pub async fn vacation_absences_in_year(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<Absence>> {
        Ok(sqlx::query_as::<_, Absence>(&format!(
            "{ABS_SELECT} WHERE user_id=$1 AND kind='vacation' \
             AND status IN ('requested','approved') \
             AND end_date >= $2 AND start_date <= $3"
        ))
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn approved_ranges_in_period(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate, String)>> {
        Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate, String)>(
            "SELECT start_date, end_date, kind FROM absences \
             WHERE user_id=$1 AND status='approved' \
             AND end_date >= $2 AND start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    // ── Mutations ──────────────────────────────────────────────────────────

    pub async fn create(
        &self,
        user_id: i64,
        kind: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        comment: Option<&str>,
        initial_status: &str,
    ) -> AppResult<Absence> {
        let mut tx = self.pool.begin().await?;
        Self::lock_user_scope_tx(&mut tx, user_id).await?;

        let overlap: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM absences WHERE user_id=$1 \
             AND status IN ('requested','approved') \
             AND end_date >= $2 AND start_date <= $3",
        )
        .bind(user_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_one(&mut *tx)
        .await?;
        if overlap > 0 {
            return Err(AppError::Conflict("Overlap with existing absence.".into()));
        }
        Self::ensure_no_time_conflict_tx(&mut tx, user_id, kind, start_date, end_date).await?;

        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO absences(user_id, kind, start_date, end_date, comment, status) \
             VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
        )
        .bind(user_id)
        .bind(kind)
        .bind(start_date)
        .bind(end_date)
        .bind(comment)
        .bind(initial_status)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(
            sqlx::query_as::<_, Absence>(&format!("{ABS_SELECT} WHERE id=$1"))
                .bind(new_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn update(
        &self,
        absence_id: i64,
        kind: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        comment: Option<&str>,
        updated_status: &str,
        owner_id: i64,
    ) -> AppResult<(Absence, Absence)> {
        let mut tx = self.pool.begin().await?;
        Self::lock_user_scope_tx(&mut tx, owner_id).await?;
        let before: Absence = sqlx::query_as::<_, Absence>(&format!(
            "{ABS_SELECT} WHERE id=$1 FOR UPDATE"
        ))
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;

        let overlap: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM absences WHERE id != $1 AND user_id=$2 \
             AND status IN ('requested','approved') \
             AND end_date >= $3 AND start_date <= $4",
        )
        .bind(absence_id)
        .bind(owner_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_one(&mut *tx)
        .await?;
        if overlap > 0 {
            return Err(AppError::Conflict("Overlap with existing absence.".into()));
        }
        Self::ensure_no_time_conflict_tx(&mut tx, owner_id, kind, start_date, end_date).await?;

        sqlx::query(
            "UPDATE absences SET kind=$1, start_date=$2, end_date=$3, comment=$4, \
             status=$5, reviewed_by=NULL, reviewed_at=NULL, rejection_reason=NULL \
             WHERE id=$6",
        )
        .bind(kind)
        .bind(start_date)
        .bind(end_date)
        .bind(comment)
        .bind(updated_status)
        .bind(absence_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let after: Absence = sqlx::query_as::<_, Absence>(&format!("{ABS_SELECT} WHERE id=$1"))
            .bind(absence_id)
            .fetch_one(&self.pool)
            .await?;
        Ok((before, after))
    }

    pub async fn cancel(&self, absence_id: i64, owner_id: i64) -> AppResult<Absence> {
        let mut tx = self.pool.begin().await?;
        Self::lock_user_scope_tx(&mut tx, owner_id).await?;
        let before: Absence = sqlx::query_as::<_, Absence>(&format!(
            "{ABS_SELECT} WHERE id=$1 FOR UPDATE"
        ))
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

    pub async fn find_for_update(
        tx: &mut sqlx::PgConnection,
        absence_id: i64,
    ) -> AppResult<Absence> {
        Ok(sqlx::query_as::<_, Absence>(&format!(
            "{ABS_SELECT} WHERE id=$1 FOR UPDATE"
        ))
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
            "SELECT TRUE FROM users u \
             WHERE u.id=$1 AND u.role!='admin' \
             AND EXISTS (SELECT 1 FROM user_approvers ua WHERE ua.user_id=$1 AND ua.approver_id=$2) \
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

    /// Load vacation ranges (requested+approved) for a year, optionally excluding one absence.
    pub async fn vacation_ranges_in_year_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        exclude_id: Option<i64>,
    ) -> AppResult<Vec<(NaiveDate, NaiveDate)>> {
        if let Some(excl) = exclude_id {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT start_date, end_date FROM absences \
                 WHERE id != $1 AND user_id=$2 AND kind='vacation' \
                 AND status IN ('requested','approved') \
                 AND end_date >= $3 AND start_date <= $4",
            )
            .bind(excl)
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(tx)
            .await?)
        } else {
            Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
                "SELECT start_date, end_date FROM absences \
                 WHERE user_id=$1 AND kind='vacation' \
                 AND status IN ('requested','approved') \
                 AND end_date >= $2 AND start_date <= $3",
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

    pub async fn ensure_no_time_conflict_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        kind: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> AppResult<()> {
        if kind == "sick" {
            return Ok(()); // sick leave doesn't block logged time
        }
        let conflict: Option<NaiveDate> = sqlx::query_scalar(
            "SELECT entry_date FROM time_entries \
             WHERE user_id=$1 AND status <> 'rejected' \
             AND entry_date BETWEEN $2 AND $3 \
             ORDER BY entry_date LIMIT 1",
        )
        .bind(user_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_optional(&mut **tx)
        .await?;
        if conflict.is_some() {
            return Err(AppError::BadRequest(
                "Non-sick absences cannot overlap days with logged time. \
                 Please remove or reject the time entries first."
                    .into(),
            ));
        }
        Ok(())
    }

    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
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
    pub async fn effective_annual_days(
        &self,
        user_id: i64,
        year: i32,
    ) -> AppResult<i64> {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT days FROM user_annual_leave WHERE user_id=$1 AND year=$2",
        )
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
