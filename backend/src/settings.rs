use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::holidays;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

const UI_LANGUAGE_KEY: &str = "ui_language";
const COUNTRY_KEY: &str = "country";
const REGION_KEY: &str = "region";
const DEFAULT_UI_LANGUAGE: &str = "en";
const DEFAULT_COUNTRY: &str = "DE";
const DEFAULT_REGION: &str = "DE-BW";

#[derive(Serialize)]
pub struct PublicSettings {
    pub ui_language: String,
    pub country: String,
    pub region: String,
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
    pub country: String,
    pub region: String,
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
    Ok(PublicSettings {
        ui_language: load_setting(pool, UI_LANGUAGE_KEY, DEFAULT_UI_LANGUAGE).await?,
        country: load_setting(pool, COUNTRY_KEY, DEFAULT_COUNTRY).await?,
        region: load_setting(pool, REGION_KEY, DEFAULT_REGION).await?,
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
    let country = body.country.trim().to_uppercase();
    let region = body.region.trim().to_string();

    if country.len() != 2 {
        return Err(AppError::BadRequest(
            "Country must be a 2-letter ISO code.".into(),
        ));
    }

    let saved_lang = save_setting(&s.pool, UI_LANGUAGE_KEY, language).await?;
    let saved_country = save_setting(&s.pool, COUNTRY_KEY, &country).await?;
    let saved_region = save_setting(&s.pool, REGION_KEY, &region).await?;

    // Refresh holidays from API with new country/region
    if let Err(e) = holidays::refresh_holidays(&s.pool, &saved_country, &saved_region).await {
        tracing::warn!("Failed to refresh holidays: {:?}", e);
    }

    Ok(Json(PublicSettings {
        ui_language: saved_lang,
        country: saved_country,
        region: saved_region,
    }))
}
