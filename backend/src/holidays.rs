use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
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

/// A country entry from the Nager.Date AvailableCountries API.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NagerCountry {
    pub country_code: String,
    pub name: String,
}

const NAGER_BASE_URL: &str = "https://date.nager.at/api/v3";

/// Fetch raw holidays from the Nager.Date API for a given country and year.
async fn fetch_nager_holidays(country: &str, year: i32) -> Result<Vec<NagerHoliday>, AppError> {
    let url = format!("{}/PublicHolidays/{}/{}", NAGER_BASE_URL, year, country);
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| AppError::Internal(format!("Nager API request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "Nager API returned status {}",
            resp.status()
        )));
    }
    resp.json()
        .await
        .map_err(|e| AppError::Internal(format!("Nager parse failed: {e}")))
}

/// Proxy: returns all countries supported by Nager.Date (compatible country codes).
pub async fn available_countries(_u: User) -> AppResult<Json<Vec<NagerCountry>>> {
    let url = format!("{}/AvailableCountries", NAGER_BASE_URL);
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| AppError::Internal(format!("Nager API request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "Nager API returned status {}",
            resp.status()
        )));
    }
    let countries: Vec<NagerCountry> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Nager parse failed: {e}")))?;
    Ok(Json(countries))
}

/// Proxy: returns the ISO 3166-2 subdivision codes used by Nager for a given country,
/// derived from the county fields of the current year's public holidays.
pub async fn available_regions(
    Path(country): Path<String>,
    _u: User,
) -> AppResult<Json<Vec<String>>> {
    let year = chrono::Local::now().year();
    let holidays = fetch_nager_holidays(&country, year).await?;

    let mut codes: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for h in &holidays {
        if let Some(counties) = &h.counties {
            for c in counties {
                codes.insert(c.clone());
            }
        }
    }
    Ok(Json(codes.into_iter().collect()))
}

/// Fetch holidays from https://date.nager.at for a given year and country.
/// Optionally filter by region (e.g. "DE-BW").
pub async fn fetch_holidays_from_api(
    country: &str,
    region: &str,
    year: i32,
) -> Result<Vec<(NaiveDate, String, String)>, AppError> {
    let holidays = fetch_nager_holidays(country, year).await?;

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
                        "INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
                         VALUES ($1, $2, $3, $4, TRUE) \
                         ON CONFLICT (holiday_date) DO NOTHING",
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
        sqlx::query_scalar("SELECT COUNT(*) FROM holidays WHERE year = $1 AND is_auto = TRUE")
            .bind(year)
            .fetch_one(pool)
            .await?;
    if count > 0 {
        return Ok(());
    }

    // Load country/region from settings
    let country: String =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'country'")
            .fetch_optional(pool)
            .await?
            .unwrap_or_default();
    let region: String = sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'region'")
        .fetch_optional(pool)
        .await?
        .unwrap_or_default();

    // Country not yet configured — skip silently until admin sets it up.
    if country.is_empty() {
        return Ok(());
    }

    match fetch_holidays_from_api(&country, &region, year).await {
        Ok(list) => {
            for (date, name, local_name) in list {
                sqlx::query(
                    "INSERT INTO holidays(holiday_date, name, local_name, year, is_auto) \
                     VALUES ($1, $2, $3, $4, TRUE) \
                     ON CONFLICT (holiday_date) DO NOTHING",
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
    /// Optional UI language code used to choose the display name.
    pub lang: Option<String>,
}

pub async fn list(
    State(s): State<AppState>,
    _u: User,
    Query(q): Query<HolidayQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());

    let language = match q.lang {
        Some(code) => i18n::Language::from_setting(&code),
        None => i18n::load_ui_language(&s.pool).await?,
    };

    let rows = sqlx::query_as::<_, Holiday>(
        "SELECT id, holiday_date, name, local_name, year, is_auto FROM holidays WHERE year=$1 ORDER BY holiday_date",
    )
    .bind(year)
    .fetch_all(&s.pool)
    .await?;

    let result: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|h| {
            let display_name = i18n::holiday_display_name(&language, h.name, h.local_name);
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
    let name = b.name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::BadRequest("Invalid holiday name.".into()));
    }
    sqlx::query("INSERT INTO holidays(holiday_date, name, year, is_auto) VALUES ($1,$2,$3, FALSE)")
        .bind(b.holiday_date)
        .bind(&name)
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
    sqlx::query("DELETE FROM holidays WHERE id=$1")
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
