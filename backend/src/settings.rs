use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

const UI_LANGUAGE_KEY: &str = "ui_language";
const DEFAULT_UI_LANGUAGE: &str = "en";

#[derive(Serialize)]
pub struct PublicSettings {
    pub ui_language: String,
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
}

fn normalize_language(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "en" => Ok("en"),
        "de" => Ok("de"),
        _ => Err(AppError::BadRequest("Invalid language.".into())),
    }
}

async fn load_ui_language(pool: &crate::db::DatabasePool) -> AppResult<String> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT value FROM app_settings WHERE key = $1",
    )
    .bind(UI_LANGUAGE_KEY)
    .fetch_optional(pool)
    .await?;

    Ok(value.unwrap_or_else(|| DEFAULT_UI_LANGUAGE.to_string()))
}

pub async fn public_settings(State(s): State<AppState>) -> AppResult<Json<PublicSettings>> {
    Ok(Json(PublicSettings {
        ui_language: load_ui_language(&s.pool).await?,
    }))
}

pub async fn admin_settings(
    State(s): State<AppState>,
    user: User,
) -> AppResult<Json<PublicSettings>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    Ok(Json(PublicSettings {
        ui_language: load_ui_language(&s.pool).await?,
    }))
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
    let saved: String = sqlx::query_scalar(
        "INSERT INTO app_settings(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP \
         RETURNING value",
    )
    .bind(UI_LANGUAGE_KEY)
    .bind(language)
    .fetch_one(&s.pool)
    .await?;

    Ok(Json(PublicSettings { ui_language: saved }))
}