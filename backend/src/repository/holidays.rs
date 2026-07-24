use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use sqlx::FromRow;
use std::collections::HashSet;

#[derive(FromRow, Serialize, Clone)]
pub struct Holiday {
    pub id: i64,
    pub holiday_date: NaiveDate,
    pub name: String,
    #[sqlx(default)]
    pub local_name: Option<String>,
    pub year: i32,
    #[sqlx(default)]
    pub is_auto: bool,
    /// Whether this holiday also applies every year after `year` (optionally
    /// bounded by `recurrence_end_year`). Manual holidays only — is_auto rows
    /// are refreshed from the Nager.Date API every year and never recur.
    #[sqlx(default)]
    pub recurring: bool,
    /// Last year (inclusive) the recurrence still applies. `None` means it
    /// applies forever going forward. Meaningless unless `recurring` is true.
    #[sqlx(default)]
    pub recurrence_end_year: Option<i32>,
}

/// Whether a recurring holiday defined in `defining_year` (optionally ending
/// in `end_year`) still applies in `target_year`.
fn recurs_in_year(defining_year: i32, end_year: Option<i32>, target_year: i32) -> bool {
    target_year >= defining_year && end_year.is_none_or(|end| target_year <= end)
}

/// Project a recurring holiday's defining date onto `target_year` (same
/// month/day, different year). Returns `None` when the month/day has no
/// equivalent in that year (a Feb 29 definition projected onto a non-leap
/// year simply does not occur that year).
fn project_occurrence(original: NaiveDate, target_year: i32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(target_year, original.month(), original.day())
}

pub struct PreparedHoliday {
    pub holiday_date: NaiveDate,
    pub name: String,
    pub local_name: String,
    pub year: i32,
}

#[derive(Clone)]
pub struct HolidayDb {
    pool: DatabasePool,
}

impl HolidayDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    pub async fn count_auto_for_year(&self, year: i32) -> AppResult<i64> {
        Ok(
            sqlx::query_scalar("SELECT COUNT(*) FROM holidays WHERE year = $1 AND is_auto = TRUE")
                .bind(year)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn list_for_year(&self, year: i32) -> AppResult<Vec<Holiday>> {
        let jan1 = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
        let dec31 = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
        self.holidays_in_range(jan1, dec31).await
    }

    /// Fetch all holidays whose effective date falls within `[from, to]`,
    /// inclusive. This is the single source of truth for "what holidays are
    /// visible in this window" — it accounts for recurring manual holidays by
    /// projecting their defining (month, day) onto every year they still
    /// apply to, not just the year they were first added for.
    async fn holidays_in_range(&self, from: NaiveDate, to: NaiveDate) -> AppResult<Vec<Holiday>> {
        // Non-recurring rows, and — for recurring rows — their own defining
        // occurrence, when its literal date falls in range.
        let literal: Vec<Holiday> = sqlx::query_as(
            "SELECT id, holiday_date, name, local_name, year, is_auto, recurring, recurrence_end_year \
             FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;

        // All recurring definitions, unconditionally. The table holds one
        // country's public holidays across at most a few decades (at most a
        // few hundred rows even with recurring entries piling up over 20+
        // years), so doing all interval arithmetic in Rust — including the
        // exact-date recheck below — is both simpler and cheap. A year-level
        // SQL prefilter here would need that same per-date recheck anyway
        // (year overlap alone is not sufficient: e.g. a June-defined
        // recurring holiday must NOT be admitted into an unrelated
        // Dec 28 - Jan 3 window just because both years are in range), so
        // it would add complexity without removing any work.
        let candidates: Vec<Holiday> = sqlx::query_as(
            "SELECT id, holiday_date, name, local_name, year, is_auto, recurring, recurrence_end_year \
             FROM holidays WHERE recurring = TRUE",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut seen: HashSet<NaiveDate> = literal.iter().map(|h| h.holiday_date).collect();
        let mut result = literal;
        for candidate in &candidates {
            for target_year in from.year()..=to.year() {
                if !recurs_in_year(candidate.year, candidate.recurrence_end_year, target_year) {
                    continue;
                }
                let Some(projected) = project_occurrence(candidate.holiday_date, target_year)
                else {
                    continue;
                };
                // Two different recurring holidays (or a recurring holiday
                // and an unrelated one-off) can coincidentally project onto
                // the same future date; `seen` keeps whichever is fetched
                // first and silently drops the other. Both agree the day is
                // a holiday, so exclusion-from-workday correctness is
                // unaffected — only the cosmetic name/id shown for that slot
                // could differ, mirroring how a manual holiday already
                // silently shadows an auto-imported one on the same date.
                if projected >= from && projected <= to && seen.insert(projected) {
                    result.push(Holiday {
                        holiday_date: projected,
                        year: target_year,
                        ..candidate.clone()
                    });
                }
            }
        }
        result.sort_by_key(|h| h.holiday_date);
        Ok(result)
    }

    /// Fetch all holiday dates in a date range (for workday calculations).
    pub async fn get_dates_in_range(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<HashSet<NaiveDate>> {
        Ok(self
            .holidays_in_range(from, to)
            .await?
            .into_iter()
            .map(|h| h.holiday_date)
            .collect())
    }

    /// Fetch holiday date+name+local_name rows in a range (for reports).
    pub async fn get_rows_in_range(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> AppResult<Vec<(NaiveDate, String, Option<String>)>> {
        Ok(self
            .holidays_in_range(from, to)
            .await?
            .into_iter()
            .map(|h| (h.holiday_date, h.name, h.local_name))
            .collect())
    }

    /// Load country setting from app_settings.
    pub async fn get_country_setting(&self) -> AppResult<String> {
        Ok(
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'country'")
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or_default(),
        )
    }

    /// Load region setting from app_settings.
    pub async fn get_region_setting(&self) -> AppResult<String> {
        Ok(
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'region'")
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or_default(),
        )
    }

    pub async fn insert(
        &self,
        holiday_date: NaiveDate,
        name: &str,
        local_name: &str,
        year: i32,
        is_auto: bool,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (holiday_date) DO NOTHING",
        )
        .bind(holiday_date)
        .bind(name)
        .bind(local_name)
        .bind(year)
        .bind(is_auto)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_manual(
        &self,
        holiday_date: NaiveDate,
        name: &str,
        recurring: bool,
        recurrence_end_year: Option<i32>,
    ) -> AppResult<i64> {
        let year = holiday_date.year();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO holidays(holiday_date, name, year, is_auto, recurring, recurrence_end_year) \
             VALUES ($1,$2,$3, FALSE, $4, $5) RETURNING id",
        )
        .bind(holiday_date)
        .bind(name)
        .bind(year)
        .bind(recurring)
        .bind(recurrence_end_year)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| AppError::conflict("Holiday already exists"))?;
        Ok(id)
    }

    /// Fetch a single holiday by id (used to snapshot state before delete for the audit log).
    pub async fn find_by_id(&self, id: i64) -> AppResult<Option<Holiday>> {
        Ok(sqlx::query_as::<_, Holiday>(
            "SELECT id, holiday_date, name, local_name, year, is_auto, recurring, recurrence_end_year \
             FROM holidays WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn delete(&self, id: i64) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM holidays WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// Delete all auto-imported holidays and bulk-insert new ones (within a tx).
    pub async fn replace_auto_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        holidays: &[PreparedHoliday],
    ) -> AppResult<()> {
        if holidays.is_empty() {
            sqlx::query("DELETE FROM holidays WHERE is_auto = TRUE")
                .execute(&mut **tx)
                .await?;
        } else {
            let years: Vec<i32> = holidays
                .iter()
                .map(|holiday| holiday.year)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            sqlx::query("DELETE FROM holidays WHERE is_auto = TRUE AND year = ANY($1)")
                .bind(&years)
                .execute(&mut **tx)
                .await?;
        }
        for h in holidays {
            sqlx::query(
                "INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
                 VALUES ($1, $2, $3, $4, TRUE) \
                 ON CONFLICT (holiday_date) DO NOTHING",
            )
            .bind(h.holiday_date)
            .bind(&h.name)
            .bind(&h.local_name)
            .bind(h.year)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    /// Begin a transaction on this pool.
    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }

    /// Delete all auto-imported holidays and re-insert from prepared list.
    pub async fn replace_auto_holidays(&self, holidays: &[PreparedHoliday]) -> AppResult<()> {
        let mut tx = self.begin().await?;
        Self::replace_auto_tx(&mut tx, holidays).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Delete auto-imported holidays for one year and re-insert the prepared
    /// list. Manual holidays are preserved.
    pub async fn replace_auto_holidays_for_year(
        &self,
        year: i32,
        holidays: &[PreparedHoliday],
    ) -> AppResult<()> {
        let mut tx = self.begin().await?;
        sqlx::query("DELETE FROM holidays WHERE is_auto = TRUE AND year = $1")
            .bind(year)
            .execute(&mut *tx)
            .await?;
        for h in holidays {
            sqlx::query(
                "INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
                 VALUES ($1, $2, $3, $4, TRUE) \
                 ON CONFLICT (holiday_date) DO NOTHING",
            )
            .bind(h.holiday_date)
            .bind(&h.name)
            .bind(&h.local_name)
            .bind(h.year)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Insert auto holidays without deleting existing ones (for initial population).
    pub async fn insert_auto_holidays(&self, holidays: &[PreparedHoliday]) -> AppResult<()> {
        for h in holidays {
            self.insert(h.holiday_date, &h.name, &h.local_name, h.year, true)
                .await?;
        }
        Ok(())
    }
}
