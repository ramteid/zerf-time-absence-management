//! A person's working weekdays over time.
//!
//! See migration 048 for why this is a dated history rather than one stored
//! pattern: Zerf recomputes every flextime and leave figure on each query, so a
//! single value would be applied to the whole past as well and a change of
//! working days would silently re-judge every week already worked.

use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::time_calc::{WorkSchedule, WorkScheduleHistory};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::FromRow;

/// One dated statement of which weekdays a person works.
#[derive(Clone, Debug, FromRow, Serialize)]
pub struct WorkWeekdays {
    pub id: i64,
    pub user_id: i64,
    /// First day this pattern applies to.
    pub valid_from: NaiveDate,
    /// ISO weekday numbers, Monday is 1 through Friday is 5.
    pub weekdays: Vec<i16>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<i64>,
}

#[derive(Clone)]
pub struct WorkScheduleDb {
    pool: DatabasePool,
}

/// The contract facts a working-day timeline is built from.
///
/// Loaded in one query because every caller needs all four together: the role
/// and the time-tracking flag decide whether the recorded weekdays apply at
/// all, the day count answers for dates no pattern reaches, and the start date
/// bounds every calculation.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ContractShape {
    pub role: String,
    pub tracks_time: bool,
    pub workdays_per_week: i16,
    pub start_date: NaiveDate,
}

impl WorkScheduleDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Every pattern ever recorded for one person, oldest first.
    pub async fn list_for_user(&self, user_id: i64) -> AppResult<Vec<WorkWeekdays>> {
        Ok(sqlx::query_as::<_, WorkWeekdays>(
            "SELECT id, user_id, valid_from, weekdays, created_at, created_by \
             FROM user_work_weekdays WHERE user_id = $1 ORDER BY valid_from, id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// The timeline one person's calculations run against.
    ///
    /// `fallback_days_per_week` answers for dates no recorded pattern reaches,
    /// and for people who have none at all — the assistants and the irregular
    /// contracts, which have no work target to place. It is the old stored
    /// count, and a count is a quota rather than a set of days, so those users
    /// keep exactly the behaviour they have today: see
    /// [`WorkSchedule::weekly_leave_cap`], which is what makes that true rather
    /// than merely intended.
    pub async fn history_for_user(
        &self,
        user_id: i64,
        fallback_days_per_week: i16,
    ) -> AppResult<WorkScheduleHistory> {
        let rows = self.list_for_user(user_id).await?;
        Ok(Self::history_from_rows(&rows, fallback_days_per_week))
    }

    /// Build a timeline from already-loaded rows, so a caller that fetched many
    /// people's patterns at once does not go back to the database per person.
    ///
    /// A row carrying anything that is not a weekday cannot form a schedule and
    /// is skipped whole — one bad value refuses the row rather than being
    /// dropped out of it, for the same reason [`WorkSchedule::fixed`] refuses
    /// one: reading `[-1, 1]` as "Mondays only" would quietly hand somebody a
    /// one-day contract. Skipping falls back to their recorded day count, which
    /// at least still says how many days they work. The database refuses such a
    /// row, so this is only reachable through a direct edit.
    pub fn history_from_rows(
        rows: &[WorkWeekdays],
        fallback_days_per_week: i16,
    ) -> WorkScheduleHistory {
        let entries = rows
            .iter()
            .filter_map(|row| {
                let days: Option<Vec<u8>> =
                    row.weekdays.iter().map(|day| u8::try_from(*day).ok()).collect();
                days.as_deref()
                    .and_then(WorkSchedule::fixed)
                    .map(|schedule| (row.valid_from, schedule))
            })
            .collect();
        WorkScheduleHistory::new(
            entries,
            WorkSchedule::without_fixed_days(fallback_days_per_week),
        )
    }

    /// Sort, de-duplicate and check a pattern before it is stored.
    ///
    /// The table's CHECK constraint says the same thing, but reaching it turns
    /// a plain mistake into a 500, and it cannot reject duplicates at all
    /// because a CHECK may not contain a subquery. Every write goes through
    /// here, so a stored pattern is always a sorted set of Monday to Friday.
    pub fn normalised_weekdays(weekdays: &[i16]) -> AppResult<Vec<i16>> {
        let mut normalised: Vec<i16> = weekdays.to_vec();
        normalised.sort_unstable();
        normalised.dedup();
        if normalised.is_empty() {
            return Err(AppError::BadRequest(
                "Working days must name at least one weekday.".into(),
            ));
        }
        if normalised.iter().any(|day| !(1..=5).contains(day)) {
            return Err(AppError::BadRequest(
                "Working days must be weekdays, Monday (1) to Friday (5).".into(),
            ));
        }
        Ok(normalised)
    }

    /// Record the weekdays a person works from `valid_from` on.
    ///
    /// Writing the same date twice replaces that day's pattern rather than
    /// adding a second one, which is what correcting a typo needs. Recording a
    /// real change means passing the date it takes effect, which leaves every
    /// earlier pattern — and so every week already worked — untouched.
    ///
    /// Weekdays are normalised by [`Self::normalised_weekdays`] on the way in.
    pub async fn set_for_user(
        &self,
        user_id: i64,
        valid_from: NaiveDate,
        weekdays: &[i16],
        created_by: Option<i64>,
    ) -> AppResult<WorkWeekdays> {
        let normalised = Self::normalised_weekdays(weekdays)?;
        Ok(sqlx::query_as::<_, WorkWeekdays>(
            "INSERT INTO user_work_weekdays (user_id, valid_from, weekdays, created_by) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, valid_from) DO UPDATE \
               SET weekdays = EXCLUDED.weekdays, created_by = EXCLUDED.created_by \
             RETURNING id, user_id, valid_from, weekdays, created_at, created_by",
        )
        .bind(user_id)
        .bind(valid_from)
        .bind(&normalised)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?)
    }

    /// As [`Self::set_for_user`], inside the caller's transaction.
    ///
    /// Creating a user and recording the days they work belong together: a
    /// person with a work target but no pattern would fall back to the old
    /// spread over Monday to Friday without anyone noticing.
    pub async fn set_for_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        valid_from: NaiveDate,
        weekdays: &[i16],
        created_by: Option<i64>,
    ) -> AppResult<()> {
        let normalised = Self::normalised_weekdays(weekdays)?;
        sqlx::query(
            "INSERT INTO user_work_weekdays (user_id, valid_from, weekdays, created_by) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, valid_from) DO UPDATE \
               SET weekdays = EXCLUDED.weekdays, created_by = EXCLUDED.created_by",
        )
        .bind(user_id)
        .bind(valid_from)
        .bind(&normalised)
        .bind(created_by)
        .execute(&mut *tx)
        .await?;
        Ok(())
    }

    /// Give a person a starting pattern if they have none at all.
    ///
    /// Returns true when one was written. Somebody can gain a work target after
    /// they were created — time tracking switched on for an admin account, an
    /// assistant promoted to employee — and both arrive here with no pattern.
    /// Without one every calculation falls back to the old spread over Monday
    /// to Friday, silently and for that person alone.
    pub async fn ensure_for_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        valid_from: NaiveDate,
        weekdays: &[i16],
        created_by: Option<i64>,
    ) -> AppResult<bool> {
        let normalised = Self::normalised_weekdays(weekdays)?;
        let rows = sqlx::query(
            "INSERT INTO user_work_weekdays (user_id, valid_from, weekdays, created_by) \
             SELECT $1, $2, $3, $4 \
             WHERE NOT EXISTS (SELECT 1 FROM user_work_weekdays WHERE user_id = $1)",
        )
        .bind(user_id)
        .bind(valid_from)
        .bind(&normalised)
        .bind(created_by)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        Ok(rows > 0)
    }

    /// Pull the oldest pattern back to `new_start` when a contract start moves
    /// earlier, so no employed day is left without one.
    ///
    /// This extends the first pattern backwards rather than recording a change:
    /// the days it now covers were never worked under anything else, so nothing
    /// about the past is being rewritten. A start date moving *later* needs
    /// nothing — the pattern simply covers more than it has to, and days before
    /// the contract began carry no target anyway.
    pub async fn extend_earliest_to_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        new_start: NaiveDate,
    ) -> AppResult<u64> {
        Ok(sqlx::query(
            "UPDATE user_work_weekdays SET valid_from = $2 \
             WHERE id = ( \
                 SELECT id FROM user_work_weekdays \
                 WHERE user_id = $1 ORDER BY valid_from, id LIMIT 1 \
             ) AND valid_from > $2",
        )
        .bind(user_id)
        .bind(new_start)
        .execute(&mut *tx)
        .await?
        .rows_affected())
    }

    /// The weekdays a contract of `workdays_per_week` days defaults to: Monday
    /// onwards, capped at Friday.
    ///
    /// A stand-in, not a statement of fact. The stored count never said *which*
    /// days, so this is the same guess migration 048 makes for the existing
    /// roster, and it is wrong for anybody whose week does not start on Monday.
    /// An admin corrects it; until they do, the person is charged and credited
    /// on Monday-first days.
    ///
    /// Only 1 to 5 ever reaches here: that is the range the user endpoints
    /// accept, and the one contract that carries 7 — an assistant's sentinel —
    /// never gets a pattern at all. The clamp is a floor and a ceiling for a
    /// value that should not arrive, not support for a six- or seven-day week:
    /// no such pattern can be stored, so a six-day contract cannot be expressed
    /// here and must keep its recorded day count instead.
    pub fn default_weekdays(workdays_per_week: i16) -> Vec<i16> {
        (1..=workdays_per_week.clamp(1, 5)).collect()
    }

    /// Bring the recorded pattern in line with a changed *number* of working
    /// days, from `effective_from` on. Returns true when a row was written.
    ///
    /// The admin screen sets a count, not a set of days, and that count and the
    /// pattern are two statements of the same fact. Left alone they drift: an
    /// employee moved from five days a week to three kept a Monday-to-Friday
    /// pattern, so every calculation went on giving them a five-day target and
    /// charging them five days of leave a week.
    ///
    /// Dated from the day the change is made, never backwards: the weeks
    /// already worked keep the pattern they were worked under, which is the
    /// whole reason this is a history.
    ///
    /// A pattern that already works that many days is left exactly as it is. An
    /// admin re-saving a contract, or correcting a stale count, must not
    /// overwrite weekdays somebody picked by hand with the Monday-first guess.
    pub async fn align_day_count_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        effective_from: NaiveDate,
        workdays_per_week: i16,
        created_by: Option<i64>,
    ) -> AppResult<bool> {
        let in_force: Option<Vec<i16>> = sqlx::query_scalar(
            "SELECT weekdays FROM user_work_weekdays \
             WHERE user_id = $1 AND valid_from <= $2 \
             ORDER BY valid_from DESC, id DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(effective_from)
        .fetch_optional(&mut *tx)
        .await?;
        let wanted = Self::default_weekdays(workdays_per_week);
        if in_force.is_some_and(|days| days.len() == wanted.len()) {
            return Ok(false);
        }
        Self::set_for_user_tx(tx, user_id, effective_from, &wanted, created_by).await?;
        Ok(true)
    }

    /// Withdraw a later change of working days.
    ///
    /// The oldest row is the contract's starting pattern and this can never
    /// remove it. That is a property of the statement itself rather than a
    /// check a caller has to remember: leaving somebody with no working days
    /// recorded would send every calculation for them back to the old spread
    /// over Monday to Friday, silently and for that person alone, and no
    /// caller — present or future — can reach that state through here.
    ///
    /// A wrong starting pattern is corrected by writing the same date again,
    /// which replaces it. That is the operation an admin fixing a typo wants,
    /// and it keeps the contract covered throughout.
    ///
    /// Returns how many rows went, so a caller can tell a withdrawn change from
    /// an attempt on the starting pattern.
    pub async fn delete_change(&self, user_id: i64, valid_from: NaiveDate) -> AppResult<u64> {
        Ok(sqlx::query(
            "DELETE FROM user_work_weekdays \
             WHERE user_id = $1 AND valid_from = $2 \
               AND valid_from > ( \
                   SELECT MIN(valid_from) FROM user_work_weekdays WHERE user_id = $1 \
               )",
        )
        .bind(user_id)
        .bind(valid_from)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    /// The contract facts behind one person's working-day timeline.
    pub async fn contract_for_user(&self, user_id: i64) -> AppResult<ContractShape> {
        Ok(sqlx::query_as::<_, ContractShape>(
            "SELECT role, tracks_time, workdays_per_week, start_date \
             FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }
}
