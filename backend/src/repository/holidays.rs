use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use sqlx::FromRow;
use std::collections::HashSet;

#[derive(FromRow, Serialize)]
pub struct Holiday {
    pub id: i64,
    pub holiday_date: NaiveDate,
    pub name: String,
    #[sqlx(default)]
    pub local_name: Option<String>,
    pub year: i32,
    #[sqlx(default)]
    pub is_auto: bool,
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
        Ok(sqlx::query_as::<_, Holiday>(
            "SELECT id, holiday_date, name, local_name, year, is_auto \
             FROM holidays WHERE year=$1 ORDER BY holiday_date",
        )
        .bind(year)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Fetch all holiday dates in a date range (for workday calculations).
    pub async fn get_dates_in_range(
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

    /// Fetch holiday date+name+local_name rows in a range (for reports).
    pub async fn get_rows_in_range(
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

    pub async fn create_manual(&self, holiday_date: NaiveDate, name: &str) -> AppResult<()> {
        let year = holiday_date.year();
        sqlx::query(
            "INSERT INTO holidays(holiday_date, name, year, is_auto) \
             VALUES ($1,$2,$3, FALSE)",
        )
        .bind(holiday_date)
        .bind(name)
        .bind(year)
        .execute(&self.pool)
        .await
        .map_err(|_| AppError::Conflict("Holiday already exists".into()))?;
        Ok(())
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

    /// Insert auto holidays without deleting existing ones (for initial population).
    pub async fn insert_auto_holidays(&self, holidays: &[PreparedHoliday]) -> AppResult<()> {
        for h in holidays {
            self.insert(h.holiday_date, &h.name, &h.local_name, h.year, true)
                .await?;
        }
        Ok(())
    }
}
