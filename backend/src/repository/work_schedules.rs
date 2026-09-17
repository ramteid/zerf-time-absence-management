//! A person's working weekdays over time.
//!
//! See migration 048 for why this is a dated history rather than one stored
//! pattern: Zerf recomputes every flextime and leave figure on each query, so a
//! single value would be applied to the whole past as well and a change of
//! working days would silently re-judge every week already worked.

use crate::db::DatabasePool;
use crate::error::AppResult;
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
    /// and for people who have none at all — the assistants, who have no work
    /// target to place. It is the old stored count, so those users keep exactly
    /// the behaviour they have today.
    pub async fn history_for_user(
        &self,
        user_id: i64,
        fallback_days_per_week: i16,
    ) -> AppResult<WorkScheduleHistory> {
        let rows = self.list_for_user(user_id).await?;
        Ok(Self::history_from_rows(
            &rows,
            fallback_days_per_week,
        ))
    }

    /// Build a timeline from already-loaded rows, so a caller that fetched many
    /// people's patterns at once does not go back to the database per person.
    ///
    /// A row whose stored weekdays are all outside Monday to Friday cannot form
    /// a schedule and is skipped; the database refuses to store one, so this is
    /// only reachable through a direct edit.
    pub fn history_from_rows(
        rows: &[WorkWeekdays],
        fallback_days_per_week: i16,
    ) -> WorkScheduleHistory {
        let entries = rows
            .iter()
            .filter_map(|row| {
                let days: Vec<u8> = row
                    .weekdays
                    .iter()
                    .filter_map(|day| u8::try_from(*day).ok())
                    .collect();
                WorkSchedule::fixed(&days).map(|schedule| (row.valid_from, schedule))
            })
            .collect();
        WorkScheduleHistory::new(
            entries,
            WorkSchedule::without_fixed_days(fallback_days_per_week),
        )
    }

    /// Record the weekdays a person works from `valid_from` on.
    ///
    /// Writing the same date twice replaces that day's pattern rather than
    /// adding a second one, which is what correcting a typo needs. Recording a
    /// real change means passing the date it takes effect, which leaves every
    /// earlier pattern — and so every week already worked — untouched.
    ///
    /// Weekdays are sorted and de-duplicated here: the stored value is read as
    /// a set, and a CHECK constraint cannot reject duplicates because it may
    /// not contain a subquery.
    pub async fn set_for_user(
        &self,
        user_id: i64,
        valid_from: NaiveDate,
        weekdays: &[i16],
        created_by: Option<i64>,
    ) -> AppResult<WorkWeekdays> {
        let mut normalised: Vec<i16> = weekdays.to_vec();
        normalised.sort_unstable();
        normalised.dedup();
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
        let mut normalised: Vec<i16> = weekdays.to_vec();
        normalised.sort_unstable();
        normalised.dedup();
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

    /// The weekdays a contract of `workdays_per_week` days defaults to: Monday
    /// onwards, capped at Friday.
    ///
    /// A stand-in, not a statement of fact. The stored count never said *which*
    /// days, so this is the same guess migration 048 makes for the existing
    /// roster, and it is wrong for anybody whose week does not start on Monday.
    /// An admin corrects it; until they do, the person is charged and credited
    /// on Monday-first days.
    pub fn default_weekdays(workdays_per_week: i16) -> Vec<i16> {
        (1..=workdays_per_week.clamp(1, 5)).collect()
    }

    /// Remove one dated pattern. Used to undo a mistaken entry; a genuine
    /// change of working days adds a row instead.
    pub async fn delete(&self, user_id: i64, valid_from: NaiveDate) -> AppResult<u64> {
        Ok(
            sqlx::query("DELETE FROM user_work_weekdays WHERE user_id = $1 AND valid_from = $2")
                .bind(user_id)
                .bind(valid_from)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }
}
