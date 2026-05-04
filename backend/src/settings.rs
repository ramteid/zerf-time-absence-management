use crate::auth::User;
use crate::db::sql;
use crate::error::{AppError, AppResult};
use crate::holidays;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

const UI_LANGUAGE_KEY: &str = "ui_language";
const COUNTRY_KEY: &str = "country";
const REGION_KEY: &str = "region";
const DEFAULT_WEEKLY_HOURS_KEY: &str = "default_weekly_hours";
const DEFAULT_ANNUAL_LEAVE_DAYS_KEY: &str = "default_annual_leave_days";
const DEFAULT_UI_LANGUAGE: &str = "en";
const DEFAULT_COUNTRY: &str = "DE";
const DEFAULT_REGION: &str = "DE-BW";

#[derive(Serialize)]
pub struct PublicSettings {
    pub ui_language: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
}

fn normalize_language(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "en" => Ok("en"),
        "de" => Ok("de"),
        _ => Err(AppError::BadRequest("Invalid language.".into())),
    }
}

async fn load_setting(
    pool: &crate::db::DatabasePool,
    key: &str,
    default: &str,
) -> AppResult<String> {
    let value: Option<String> = sqlx::query_scalar(&sql("SELECT value FROM app_settings WHERE key = $1"))
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value.unwrap_or_else(|| default.to_string()))
}

async fn save_setting(pool: &crate::db::DatabasePool, key: &str, value: &str) -> AppResult<String> {
    let saved: String = sqlx::query_scalar(
        &sql("INSERT INTO app_settings(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP \
         RETURNING value"),
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
        country: load_setting(pool, COUNTRY_KEY, DEFAULT_COUNTRY).await?,
        region: load_setting(pool, REGION_KEY, DEFAULT_REGION).await?,
        default_weekly_hours: dwh.parse().ok(),
        default_annual_leave_days: dal.parse().ok(),
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
    let previous = load_all_settings(&s.pool).await?;
    let country = body.country.trim().to_uppercase();
    let region = body.region.trim().to_string();

    if country.len() != 2 {
        return Err(AppError::BadRequest(
            "Country must be a 2-letter ISO code.".into(),
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

    save_setting(&s.pool, UI_LANGUAGE_KEY, language).await?;
    let saved_country = save_setting(&s.pool, COUNTRY_KEY, &country).await?;
    let saved_region = save_setting(&s.pool, REGION_KEY, &region).await?;

    if let Some(dwh) = body.default_weekly_hours {
        save_setting(&s.pool, DEFAULT_WEEKLY_HOURS_KEY, &dwh.to_string()).await?;
    }
    if let Some(dal) = body.default_annual_leave_days {
        save_setting(&s.pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, &dal.to_string()).await?;
    }

    if previous.country != saved_country || previous.region != saved_region {
        if let Err(e) = holidays::refresh_holidays(&s.pool, &saved_country, &saved_region).await {
            tracing::warn!("Failed to refresh holidays: {:?}", e);
        }
    }

    Ok(Json(load_all_settings(&s.pool).await?))
}
