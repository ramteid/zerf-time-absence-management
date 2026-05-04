use crate::auth::User;
use crate::db::sql;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// A single holiday from the Nager.Date API.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NagerHoliday {
    date: NaiveDate,
    local_name: String,
    name: String,
    /// County codes like ["DE-BW","DE-BY"]. null means nation-wide.
    counties: Option<Vec<String>>,
}

/// Fetch holidays from https://date.nager.at for a given year and country.
/// Optionally filter by region (e.g. "DE-BW").
pub async fn fetch_holidays_from_api(
    country: &str,
    region: &str,
    year: i32,
) -> Result<Vec<(NaiveDate, String, String)>, AppError> {
    let url = format!(
        "https://date.nager.at/api/v3/PublicHolidays/{}/{}",
        year, country
    );
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| AppError::Internal(format!("Holiday API request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "Holiday API returned status {}",
            resp.status()
        )));
    }

    let holidays: Vec<NagerHoliday> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Holiday API parse failed: {e}")))?;

    // Filter by region if set: keep nation-wide (counties=null) and matching region
    let filtered: Vec<(NaiveDate, String, String)> = holidays
        .into_iter()
        .filter(|h| {
            if region.is_empty() {
                return true;
            }
            match &h.counties {
                None => true, // nation-wide
                Some(c) => c.iter().any(|cc| cc == region),
            }
        })
        .map(|h| (h.date, h.name, h.local_name))
        .collect();

    Ok(filtered)
}

/// Delete all auto-imported holidays and re-import for the given years.
pub async fn refresh_holidays(
    pool: &crate::db::DatabasePool,
    country: &str,
    region: &str,
) -> AppResult<()> {
    // Delete all auto-imported holidays
    sqlx::query("DELETE FROM holidays WHERE is_auto = TRUE")
        .execute(pool)
        .await?;

    let year = chrono::Local::now().year();
    for y in [year, year + 1] {
        match fetch_holidays_from_api(country, region, y).await {
            Ok(list) => {
                for (date, name, local_name) in list {
                    sqlx::query(
                        &sql("INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
                         VALUES ($1, $2, $3, $4, TRUE) \
                         ON CONFLICT (holiday_date) DO NOTHING"),
                    )
                    .bind(date)
                    .bind(&name)
                    .bind(&local_name)
                    .bind(y)
                    .execute(pool)
                    .await?;
                }
            }
            Err(e) => {
                tracing::warn!("Failed to fetch holidays for {}/{}: {:?}", country, y, e);
            }
        }
    }

    Ok(())
}

/// Ensure holidays exist for a given year (called on startup).
pub async fn ensure_holidays(pool: &crate::db::DatabasePool, year: i32) -> AppResult<()> {
    // Check if any auto holidays exist for this year
    let count: i64 =
        sqlx::query_scalar(&sql("SELECT COUNT(*) FROM holidays WHERE year = $1 AND is_auto = TRUE"))
            .bind(year)
            .fetch_one(pool)
            .await?;
    if count > 0 {
        return Ok(());
    }

    // Load country/region from settings
    let country: String =
        sqlx::query_scalar(&sql("SELECT value FROM app_settings WHERE key = 'country'"))
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| "DE".to_string());
    let region: String = sqlx::query_scalar(&sql("SELECT value FROM app_settings WHERE key = 'region'"))
        .fetch_optional(pool)
        .await?
        .unwrap_or_else(|| "DE-BW".to_string());

    match fetch_holidays_from_api(&country, &region, year).await {
        Ok(list) => {
            for (date, name, local_name) in list {
                sqlx::query(
                    &sql("INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
                     VALUES ($1, $2, $3, $4, TRUE) \
                     ON CONFLICT (holiday_date) DO NOTHING"),
                )
                .bind(date)
                .bind(&name)
                .bind(&local_name)
                .bind(year)
                .execute(pool)
                .await?;
            }
        }
        Err(e) => {
            tracing::warn!("Failed to fetch holidays for {}/{}: {:?}", country, year, e);
        }
    }

    Ok(())
}

pub fn next_monday_noon(now: DateTime<Local>) -> AppResult<DateTime<Local>> {
    let weekday = now.weekday().num_days_from_monday();
    let days_ahead = if weekday == 0 && now.hour() < 12 {
        0
    } else {
        7 - weekday
    };
    let target_date = now.date_naive() + Duration::days(i64::from(days_ahead));
    let target_naive = target_date.and_hms_opt(12, 0, 0).ok_or_else(|| {
        AppError::Internal("Failed to calculate holiday scheduler target.".into())
    })?;
    Local
        .from_local_datetime(&target_naive)
        .single()
        .ok_or_else(|| AppError::Internal("Failed to resolve local scheduler time.".into()))
}

pub fn duration_until_next_monday_noon(now: DateTime<Local>) -> AppResult<std::time::Duration> {
    (next_monday_noon(now)? - now)
        .to_std()
        .map_err(|_| AppError::Internal("Holiday scheduler target is in the past.".into()))
}

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

#[derive(Deserialize)]
pub struct HolidayQuery {
    pub year: Option<i32>,
    /// Pass "de" or "en" to control which name field is returned as `name`.
    pub lang: Option<String>,
}

pub async fn list(
    State(s): State<AppState>,
    _u: User,
    Query(q): Query<HolidayQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());

    // Load UI language from settings if not passed as query param
    let lang = match q.lang {
        Some(l) => l,
        None => sqlx::query_scalar(&sql("SELECT value FROM app_settings WHERE key = 'ui_language'"))
            .fetch_optional(&s.pool)
            .await?
            .unwrap_or_else(|| "en".to_string()),
    };

    let rows = sqlx::query_as::<_, Holiday>(
        &sql("SELECT id, holiday_date, name, local_name, year, is_auto FROM holidays WHERE year=$1 ORDER BY holiday_date"),
    )
    .bind(year)
    .fetch_all(&s.pool)
    .await?;

    // Pick the display name based on language.
    // For non-English languages, prefer local_name if available.
    let result: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|h| {
            let display_name = if lang != "en" {
                h.local_name.clone().unwrap_or_else(|| h.name.clone())
            } else {
                h.name.clone()
            };
            serde_json::json!({
                "id": h.id,
                "holiday_date": h.holiday_date,
                "name": display_name,
                "year": h.year,
                "is_auto": h.is_auto,
            })
        })
        .collect();

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct NewHoliday {
    pub holiday_date: NaiveDate,
    pub name: String,
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewHoliday>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query(&sql("INSERT INTO holidays(holiday_date, name, year, is_auto) VALUES ($1,$2,$3, FALSE)"))
        .bind(b.holiday_date)
        .bind(&b.name)
        .bind(b.holiday_date.year())
        .execute(&s.pool)
        .await
        .map_err(|_| AppError::Conflict("Holiday already exists".into()))?;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn delete(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query(&sql("DELETE FROM holidays WHERE id=$1"))
        .bind(id)
        .execute(&s.pool)
        .await?;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_at(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn next_monday_noon_uses_same_day_before_noon() {
        let now = local_at(2026, 5, 4, 11);
        let target = next_monday_noon(now).unwrap();
        assert_eq!(target.date_naive(), now.date_naive());
        assert_eq!(target.hour(), 12);
    }

    #[test]
    fn next_monday_noon_advances_after_monday_noon() {
        let now = local_at(2026, 5, 4, 12);
        let target = next_monday_noon(now).unwrap();
        assert_eq!(
            target.date_naive(),
            NaiveDate::from_ymd_opt(2026, 5, 11).unwrap()
        );
        assert_eq!(target.hour(), 12);
    }

    #[test]
    fn next_monday_noon_advances_from_midweek() {
        let now = local_at(2026, 5, 6, 9);
        let target = next_monday_noon(now).unwrap();
        assert_eq!(
            target.date_naive(),
            NaiveDate::from_ymd_opt(2026, 5, 11).unwrap()
        );
        assert_eq!(target.hour(), 12);
    }
}
