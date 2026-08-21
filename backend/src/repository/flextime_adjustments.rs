use crate::db::DatabasePool;
use crate::error::AppResult;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::FromRow;

/// The carry-in balance booked once when a flextime account is opened.
/// At most one per user (enforced by a partial unique index).
pub const KIND_OPENING_BALANCE: &str = "opening_balance";
/// Every later admin-made change to a flextime balance.
pub const KIND_CORRECTION: &str = "correction";

/// Largest absolute adjustment accepted, in minutes (one year). Mirrors the
/// bound the old `users.overtime_start_balance_min` column was validated
/// against and the CHECK constraint in migration 043.
pub const MAX_ADJUSTMENT_MIN: i64 = 525_600;

/// One dated, signed change to a user's flextime balance that did not come
/// from worked time. See migration 043 for the reasoning.
#[derive(Clone, Debug, FromRow, Serialize)]
pub struct FlextimeAdjustment {
    pub id: i64,
    pub user_id: i64,
    pub effective_date: NaiveDate,
    pub minutes: i64,
    pub kind: String,
    pub reason: Option<String>,
    pub created_by: Option<i64>,
    /// Full name of the admin who booked it. `None` for rows written by the
    /// migration and for rows whose author has since been deleted — the UI
    /// labels both as "System".
    pub created_by_name: Option<String>,
    /// Set when this row cancels an earlier one out.
    pub reverses_id: Option<i64>,
    /// TRUE when a later row cancels this one out, so the UI can show it as
    /// struck through rather than as a live part of the balance.
    pub reversed: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct FlextimeAdjustmentDb {
    pool: DatabasePool,
}

// The author's name is resolved in the query rather than in a second
// round-trip because every caller that reads adjustments also renders who made
// them. sqlx only accepts `&'static str`, so the shared select list is spelled
// out per query instead of being composed with `format!`.
const LIST_FOR_USER_SQL: &str = "SELECT a.id, a.user_id, a.effective_date, a.minutes, a.kind, \
     a.reason, a.created_by, \
     CASE WHEN author.id IS NULL THEN NULL \
          ELSE author.first_name || ' ' || author.last_name END AS created_by_name, \
     a.reverses_id, \
     EXISTS (SELECT 1 FROM flextime_adjustments r WHERE r.reverses_id = a.id) AS reversed, \
     a.created_at \
     FROM flextime_adjustments a \
     LEFT JOIN users author ON author.id = a.created_by \
     WHERE a.user_id = $1 ORDER BY a.effective_date, a.id";

const FIND_BY_ID_SQL: &str = "SELECT a.id, a.user_id, a.effective_date, a.minutes, a.kind, \
     a.reason, a.created_by, \
     CASE WHEN author.id IS NULL THEN NULL \
          ELSE author.first_name || ' ' || author.last_name END AS created_by_name, \
     a.reverses_id, \
     EXISTS (SELECT 1 FROM flextime_adjustments r WHERE r.reverses_id = a.id) AS reversed, \
     a.created_at \
     FROM flextime_adjustments a \
     LEFT JOIN users author ON author.id = a.created_by \
     WHERE a.id = $1";

impl FlextimeAdjustmentDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Every adjustment for one user, oldest effective date first.
    pub async fn list_for_user(&self, user_id: i64) -> AppResult<Vec<FlextimeAdjustment>> {
        Ok(sqlx::query_as::<_, FlextimeAdjustment>(LIST_FOR_USER_SQL)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_by_id(&self, id: i64) -> AppResult<Option<FlextimeAdjustment>> {
        Ok(sqlx::query_as::<_, FlextimeAdjustment>(FIND_BY_ID_SQL)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Signed minutes that have taken effect on or before `through`.
    ///
    /// `ledger_start` is the user's contract start date: an adjustment dated
    /// before it is pulled forward to that date, because the ledger itself
    /// does not exist earlier. Without the clamp, moving a start date forward
    /// (which the admin UI allows) would silently drop an adjustment out of
    /// every balance instead of just relocating it.
    pub async fn sum_through(
        &self,
        user_id: i64,
        ledger_start: NaiveDate,
        through: NaiveDate,
    ) -> AppResult<i64> {
        // SUM over BIGINT yields NUMERIC in Postgres, which does not decode
        // into i64 — the cast is load-bearing, not cosmetic.
        let sum: Option<i64> = sqlx::query_scalar(
            "SELECT SUM(minutes)::BIGINT FROM flextime_adjustments \
             WHERE user_id = $1 AND GREATEST(effective_date, $2::date) <= $3",
        )
        .bind(user_id)
        .bind(ledger_start)
        .bind(through)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    /// Signed minutes taking effect within `[from, to]`, with the same
    /// start-date clamping as [`Self::sum_through`]. An empty or inverted
    /// range yields 0.
    pub async fn sum_in_range(
        &self,
        user_id: i64,
        ledger_start: NaiveDate,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<i64> {
        let sum: Option<i64> = sqlx::query_scalar(
            "SELECT SUM(minutes)::BIGINT FROM flextime_adjustments \
             WHERE user_id = $1 AND GREATEST(effective_date, $2::date) BETWEEN $3 AND $4",
        )
        .bind(user_id)
        .bind(ledger_start)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    /// Signed minutes taking effect on or after `from`, with no upper bound.
    ///
    /// Used by the flextime floor check, which must treat every booking that
    /// is not yet reflected in the approved-hours ledger as already committed
    /// — including one dated ahead of today, which happens when an account is
    /// opened with a carry-in balance and a future contract start.
    pub async fn sum_from(
        &self,
        user_id: i64,
        ledger_start: NaiveDate,
        from: NaiveDate,
    ) -> AppResult<i64> {
        let sum: Option<i64> = sqlx::query_scalar(
            "SELECT SUM(minutes)::BIGINT FROM flextime_adjustments \
             WHERE user_id = $1 AND GREATEST(effective_date, $2::date) >= $3",
        )
        .bind(user_id)
        .bind(ledger_start)
        .bind(from)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    /// Adjustment totals grouped by the `YYYY-MM` month they take effect in,
    /// with the same start-date clamping as [`Self::sum_through`]. Keyed to
    /// match the month labels the overtime rows are addressed by.
    /// `effective_through` caps which bookings count at all — callers pass
    /// today, because a booking dated later has not moved the balance yet.
    /// Without the cap a payout dated later this month would land in the
    /// current month's bucket and inflate the balance the dashboard shows.
    pub async fn totals_by_month(
        &self,
        user_id: i64,
        ledger_start: NaiveDate,
        effective_through: NaiveDate,
    ) -> AppResult<Vec<(String, i64)>> {
        Ok(sqlx::query_as::<_, (String, i64)>(
            "SELECT to_char(GREATEST(effective_date, $2::date), 'YYYY-MM') AS m, \
                    SUM(minutes)::BIGINT \
             FROM flextime_adjustments WHERE user_id = $1 \
             AND GREATEST(effective_date, $2::date) <= $3 \
             GROUP BY m ORDER BY m",
        )
        .bind(user_id)
        .bind(ledger_start)
        .bind(effective_through)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Adjustment totals per effective date within `[from, to]`, with the same
    /// start-date clamping as [`Self::sum_through`]. Returned as a list of
    /// `(date, minutes)` pairs so callers can bucket them however they need
    /// (per day for the flextime ledger, per month for the overtime rows).
    pub async fn totals_by_date(
        &self,
        user_id: i64,
        ledger_start: NaiveDate,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, i64)>> {
        Ok(sqlx::query_as::<_, (NaiveDate, i64)>(
            "SELECT GREATEST(effective_date, $2::date) AS d, SUM(minutes)::BIGINT \
             FROM flextime_adjustments \
             WHERE user_id = $1 AND GREATEST(effective_date, $2::date) BETWEEN $3 AND $4 \
             GROUP BY d ORDER BY d",
        )
        .bind(user_id)
        .bind(ledger_start)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Insert one adjustment inside the caller's transaction. `reverses_id`
    /// is set only when the row cancels an earlier one out.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        effective_date: NaiveDate,
        minutes: i64,
        kind: &str,
        reason: Option<&str>,
        created_by: Option<i64>,
        reverses_id: Option<i64>,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "INSERT INTO flextime_adjustments \
             (user_id, effective_date, minutes, kind, reason, created_by, reverses_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id",
        )
        .bind(user_id)
        .bind(effective_date)
        .bind(minutes)
        .bind(kind)
        .bind(reason)
        .bind(created_by)
        .bind(reverses_id)
        .fetch_one(&mut *tx)
        .await?)
    }
}
