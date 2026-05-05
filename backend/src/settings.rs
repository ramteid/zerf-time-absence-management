use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::holidays;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

const UI_LANGUAGE_KEY: &str = "ui_language";
const TIME_FORMAT_KEY: &str = "time_format";
const COUNTRY_KEY: &str = "country";
const REGION_KEY: &str = "region";
const DEFAULT_WEEKLY_HOURS_KEY: &str = "default_weekly_hours";
const DEFAULT_ANNUAL_LEAVE_DAYS_KEY: &str = "default_annual_leave_days";
const CARRYOVER_EXPIRY_DATE_KEY: &str = "carryover_expiry_date";
const DEFAULT_UI_LANGUAGE: &str = "en";
const DEFAULT_TIME_FORMAT: &str = "24h";
const DEFAULT_COUNTRY: &str = "";
const DEFAULT_REGION: &str = "";
const DEFAULT_CARRYOVER_EXPIRY_DATE: &str = "03-31";

#[derive(Serialize)]
pub struct PublicSettings {
    pub ui_language: String,
    pub time_format: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
    pub carryover_expiry_date: String,
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
    pub time_format: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
    pub carryover_expiry_date: Option<String>,
}

fn normalize_language(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "en" => Ok("en"),
        "de" => Ok("de"),
        _ => Err(AppError::BadRequest("Invalid language.".into())),
    }
}

fn normalize_time_format(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "24h" => Ok("24h"),
        "12h" => Ok("12h"),
        _ => Err(AppError::BadRequest("Invalid time format.".into())),
    }
}

pub async fn load_setting(
    pool: &crate::db::DatabasePool,
    key: &str,
    default: &str,
) -> AppResult<String> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value.unwrap_or_else(|| default.to_string()))
}

async fn save_setting(pool: &crate::db::DatabasePool, key: &str, value: &str) -> AppResult<String> {
    let saved: String = sqlx::query_scalar(
        "INSERT INTO app_settings(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP \
         RETURNING value",
    )
    .bind(key)
    .bind(value)
    .fetch_one(pool)
    .await?;
    Ok(saved)
}

async fn load_all_settings(pool: &crate::db::DatabasePool) -> AppResult<PublicSettings> {
    let dwh = load_setting(pool, DEFAULT_WEEKLY_HOURS_KEY, "").await?;
    let dal = load_setting(pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, "").await?;
    Ok(PublicSettings {
        ui_language: load_setting(pool, UI_LANGUAGE_KEY, DEFAULT_UI_LANGUAGE).await?,
        time_format: load_setting(pool, TIME_FORMAT_KEY, DEFAULT_TIME_FORMAT).await?,
        country: load_setting(pool, COUNTRY_KEY, DEFAULT_COUNTRY).await?,
        region: load_setting(pool, REGION_KEY, DEFAULT_REGION).await?,
        default_weekly_hours: dwh.parse().ok(),
        default_annual_leave_days: dal.parse().ok(),
        carryover_expiry_date: load_setting(pool, CARRYOVER_EXPIRY_DATE_KEY, DEFAULT_CARRYOVER_EXPIRY_DATE).await?,
    })
}

pub async fn public_settings(State(s): State<AppState>) -> AppResult<Json<PublicSettings>> {
    Ok(Json(load_all_settings(&s.pool).await?))
}

pub async fn admin_settings(
    State(s): State<AppState>,
    user: User,
) -> AppResult<Json<PublicSettings>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(load_all_settings(&s.pool).await?))
}

pub async fn update_admin_settings(
    State(s): State<AppState>,
    user: User,
    Json(body): Json<UpdateSettings>,
) -> AppResult<Json<PublicSettings>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let language = normalize_language(&body.ui_language)?;
    let time_format = normalize_time_format(&body.time_format)?;
    let previous = load_all_settings(&s.pool).await?;
    let country = body.country.trim().to_uppercase();
    let region = body.region.trim().to_string();

    if !country.is_empty() && country.len() != 2 {
        return Err(AppError::BadRequest(
            "Country must be a 2-letter ISO code (or empty to clear).".into(),
        ));
    }
    if region.len() > 20 {
        return Err(AppError::BadRequest(
            "Region code must be at most 20 characters.".into(),
        ));
    }
    if let Some(dwh) = body.default_weekly_hours {
        if !(0.0..=168.0).contains(&dwh) {
            return Err(AppError::BadRequest("Invalid default_weekly_hours.".into()));
        }
    }
    if let Some(dal) = body.default_annual_leave_days {
        if !(0..=366).contains(&dal) {
            return Err(AppError::BadRequest(
                "Invalid default_annual_leave_days.".into(),
            ));
        }
    }

    // Validate and save carryover expiry date (MM-DD format).
    if let Some(ref ced) = body.carryover_expiry_date {
        let ced = ced.trim();
        let parts: Vec<&str> = ced.split('-').collect();
        if parts.len() != 2 {
            return Err(AppError::BadRequest("carryover_expiry_date must be MM-DD.".into()));
        }
        let month: u32 = parts[0].parse().map_err(|_| AppError::BadRequest("Invalid month in carryover_expiry_date.".into()))?;
        let day: u32 = parts[1].parse().map_err(|_| AppError::BadRequest("Invalid day in carryover_expiry_date.".into()))?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(AppError::BadRequest("Invalid carryover_expiry_date.".into()));
        }
        // Validate that the date actually exists (use a non-leap year to be strict).
        if chrono::NaiveDate::from_ymd_opt(2025, month, day).is_none() {
            return Err(AppError::BadRequest("Invalid carryover_expiry_date.".into()));
        }
        save_setting(&s.pool, CARRYOVER_EXPIRY_DATE_KEY, ced).await?;
    }

    save_setting(&s.pool, UI_LANGUAGE_KEY, language).await?;
    save_setting(&s.pool, TIME_FORMAT_KEY, time_format).await?;
    let saved_country = save_setting(&s.pool, COUNTRY_KEY, &country).await?;
    let saved_region = save_setting(&s.pool, REGION_KEY, &region).await?;

    let dwh_str = body
        .default_weekly_hours
        .map(|v| v.to_string())
        .unwrap_or_default();
    save_setting(&s.pool, DEFAULT_WEEKLY_HOURS_KEY, &dwh_str).await?;

    let dal_str = body
        .default_annual_leave_days
        .map(|v| v.to_string())
        .unwrap_or_default();
    save_setting(&s.pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, &dal_str).await?;

    if !saved_country.is_empty()
        && (previous.country != saved_country || previous.region != saved_region)
    {
        if let Err(e) = holidays::refresh_holidays(&s.pool, &saved_country, &saved_region).await {
            tracing::warn!("Failed to refresh holidays: {:?}", e);
        }
    }

    Ok(Json(load_all_settings(&s.pool).await?))
}
