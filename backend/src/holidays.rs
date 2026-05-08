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

/// A single holiday from the Nager.Date API.
#[derive(Clone, Deserialize, Debug)]
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

pub struct PreparedHoliday {
    pub holiday_date: NaiveDate,
    pub name: String,
    pub local_name: String,
    pub year: i32,
}

const NAGER_BASE_URL: &str = "https://date.nager.at/api/v3";

async fn fetch_available_countries() -> Result<Vec<NagerCountry>, AppError> {
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
    resp.json()
        .await
        .map_err(|e| AppError::Internal(format!("Nager parse failed: {e}")))
}

fn collect_region_codes(holidays: &[NagerHoliday]) -> Vec<String> {
    let region_codes: std::collections::BTreeSet<String> = holidays
        .iter()
        .filter_map(|holiday| holiday.counties.as_ref())
        .flatten()
        .cloned()
        .collect();
    region_codes.into_iter().collect()
}

fn filter_holidays_by_region(
    holidays: Vec<NagerHoliday>,
    region: &str,
) -> Vec<(NaiveDate, String, String)> {
    holidays
        .into_iter()
        .filter(|holiday| {
            region.is_empty()
                || holiday
                    .counties
                    .as_ref()
                    .is_none_or(|codes| codes.iter().any(|code| code == region))
        })
        .map(|holiday| (holiday.date, holiday.name, holiday.local_name))
        .collect()
}

fn validate_region_selection(region: &str, available_regions: &[String]) -> AppResult<()> {
    if region.is_empty() || available_regions.iter().any(|code| code == region) {
        return Ok(());
    }

    Err(AppError::BadRequest(
        "Selected region is not valid for this country.".into(),
    ))
}

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

/// Proxy: returns all countries supported by Nager.Date.
pub async fn available_countries(_requester: User) -> AppResult<Json<Vec<NagerCountry>>> {
    Ok(Json(fetch_available_countries().await?))
}

/// Proxy: returns the ISO 3166-2 subdivision codes used by Nager for a given country,
/// derived from the county fields of the current year's public holidays.
pub async fn available_regions(
    Path(country): Path<String>,
    _requester: User,
) -> AppResult<Json<Vec<String>>> {
    let year = chrono::Local::now().year();
    let holidays = fetch_nager_holidays(&country, year).await?;
    Ok(Json(collect_region_codes(&holidays)))
}

/// Fetch holidays from https://date.nager.at for a given year and country.
/// Optionally filter by region (e.g. "DE-BW").
pub async fn fetch_holidays_from_api(
    country: &str,
    region: &str,
    year: i32,
) -> Result<Vec<(NaiveDate, String, String)>, AppError> {
    let holidays = fetch_nager_holidays(country, year).await?;

    Ok(filter_holidays_by_region(holidays, region))
}

pub async fn prepare_holiday_refresh(
    country: &str,
    region: &str,
) -> AppResult<Vec<PreparedHoliday>> {
    let normalized_country = country.trim().to_uppercase();
    let normalized_region = region.trim().to_string();

    if normalized_country.is_empty() {
        if normalized_region.is_empty() {
            return Ok(Vec::new());
        }
        return Err(AppError::BadRequest(
            "Region cannot be set without a country.".into(),
        ));
    }

    let countries = fetch_available_countries().await?;
    if !countries
        .iter()
        .any(|item| item.country_code == normalized_country)
    {
        return Err(AppError::BadRequest(
            "Selected country is not supported for holiday import.".into(),
        ));
    }

    let year = chrono::Local::now().year();
    let current_year_holidays = fetch_nager_holidays(&normalized_country, year).await?;
    let available_regions = collect_region_codes(&current_year_holidays);
    validate_region_selection(&normalized_region, &available_regions)?;

    let mut prepared: Vec<PreparedHoliday> = filter_holidays_by_region(
        current_year_holidays,
        &normalized_region,
    )
    .into_iter()
    .map(|(holiday_date, name, local_name)| PreparedHoliday {
        holiday_date,
        name,
        local_name,
        year,
    })
    .collect();

    let next_year = year + 1;
    prepared.extend(
        filter_holidays_by_region(
            fetch_nager_holidays(&normalized_country, next_year).await?,
            &normalized_region,
        )
        .into_iter()
        .map(|(holiday_date, name, local_name)| PreparedHoliday {
            holiday_date,
            name,
            local_name,
            year: next_year,
        }),
    );

    Ok(prepared)
}

pub async fn replace_auto_holidays_exec(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    holidays: &[PreparedHoliday],
) -> AppResult<()> {
    let prepared: Vec<crate::repository::PreparedHoliday> = holidays
        .iter()
        .map(|h| crate::repository::PreparedHoliday {
            holiday_date: h.holiday_date,
            name: h.name.clone(),
            local_name: h.local_name.clone(),
            year: h.year,
        })
        .collect();
    crate::repository::HolidayDb::replace_auto_tx(tx, &prepared).await
}

/// Delete all auto-imported holidays and re-import for the given years.
pub async fn refresh_holidays(
    pool: &crate::db::DatabasePool,
    country: &str,
    region: &str,
) -> AppResult<()> {
    let prepared = prepare_holiday_refresh(country, region).await?;
    let repo_prepared: Vec<crate::repository::PreparedHoliday> = prepared
        .iter()
        .map(|h| crate::repository::PreparedHoliday {
            holiday_date: h.holiday_date,
            name: h.name.clone(),
            local_name: h.local_name.clone(),
            year: h.year,
        })
        .collect();
    let db = crate::repository::HolidayDb::new(pool.clone());
    db.replace_auto_holidays(&repo_prepared).await
}

/// Ensure holidays exist for a given year (called on startup).
pub async fn ensure_holidays(pool: &crate::db::DatabasePool, year: i32) -> AppResult<()> {
    let db = crate::repository::HolidayDb::new(pool.clone());
    let count = db.count_auto_for_year(year).await?;
    if count > 0 {
        return Ok(());
    }

    // Load country/region from settings
    let settings_db = crate::repository::SettingsDb::new(pool.clone());
    let country = settings_db.get_raw("country").await?.unwrap_or_default();
    let region = settings_db.get_raw("region").await?.unwrap_or_default();

    // Country not yet configured — skip silently until admin sets it up.
    if country.is_empty() {
        return Ok(());
    }

    match fetch_holidays_from_api(&country, &region, year).await {
        Ok(list) => {
            let prepared: Vec<crate::repository::PreparedHoliday> = list
                .into_iter()
                .map(|(date, name, local_name)| crate::repository::PreparedHoliday {
                    holiday_date: date,
                    name,
                    local_name,
                    year,
                })
                .collect();
            db.insert_auto_holidays(&prepared).await?;
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

#[derive(Serialize)]
pub struct Holiday {
    pub id: i64,
    pub holiday_date: NaiveDate,
    pub name: String,
    pub local_name: Option<String>,
    pub year: i32,
    pub is_auto: bool,
}

#[derive(Deserialize)]
pub struct HolidayQuery {
    pub year: Option<i32>,
    /// Optional UI language code used to choose the display name.
    pub lang: Option<String>,
}

pub async fn list(
    State(app_state): State<AppState>,
    _requester: User,
    Query(query): Query<HolidayQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());

    let language = match query.lang {
        Some(code) => i18n::Language::from_setting(&code),
        None => i18n::load_ui_language(&app_state.pool).await?,
    };

    let holiday_rows = app_state.db.holidays.list_for_year(year).await?;

    let result: Vec<serde_json::Value> = holiday_rows
        .into_iter()
        .map(|holiday| {
            let display_name =
                i18n::holiday_display_name(&language, holiday.name, holiday.local_name);
            serde_json::json!({
                "id": holiday.id,
                "holiday_date": holiday.holiday_date,
                "name": display_name,
                "year": holiday.year,
                "is_auto": holiday.is_auto,
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
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewHoliday>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let holiday_name = body.name.trim().to_string();
    if holiday_name.is_empty() || holiday_name.len() > 200 {
        return Err(AppError::BadRequest("Invalid holiday name.".into()));
    }
    app_state
        .db
        .holidays
        .create_manual(body.holiday_date, &holiday_name)
        .await?;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn delete(
    State(app_state): State<AppState>,
    requester: User,
    Path(holiday_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    app_state.db.holidays.delete(holiday_id).await?;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_holidays() -> Vec<NagerHoliday> {
        vec![
            NagerHoliday {
                date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                local_name: "Neujahr".into(),
                name: "New Year's Day".into(),
                counties: None,
            },
            NagerHoliday {
                date: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
                local_name: "Heilige Drei Konige".into(),
                name: "Epiphany".into(),
                counties: Some(vec!["DE-BW".into(), "DE-BY".into()]),
            },
        ]
    }

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

    #[test]
    fn collect_region_codes_returns_sorted_unique_codes() {
        let regions = collect_region_codes(&sample_holidays());
        assert_eq!(regions, vec!["DE-BW", "DE-BY"]);
    }

    #[test]
    fn validate_region_selection_accepts_empty_or_known_codes() {
        let available_regions = vec!["DE-BW".to_string(), "DE-BY".to_string()];
        assert!(validate_region_selection("", &available_regions).is_ok());
        assert!(validate_region_selection("DE-BW", &available_regions).is_ok());
        assert!(validate_region_selection("DE-XX", &available_regions).is_err());
    }

    #[test]
    fn filter_holidays_by_region_keeps_nationwide_and_matching_entries() {
        let filtered = filter_holidays_by_region(sample_holidays(), "DE-BW");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].1, "New Year's Day");
        assert_eq!(filtered[1].1, "Epiphany");
    }
}
