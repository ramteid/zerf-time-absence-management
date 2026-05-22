use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::settings::app_current_year;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

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
    let year = match query.year {
        Some(value) => value,
        None => app_current_year(&app_state.pool).await,
    };

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

/// Proxy: returns all countries supported by Nager.Date.
pub async fn available_countries(_requester: User) -> AppResult<Json<Vec<crate::services::holidays::NagerCountry>>> {
    Ok(Json(crate::services::holidays::fetch_available_countries().await?))
}

/// Proxy: returns the ISO 3166-2 subdivision codes used by Nager for a given country,
/// derived from the county fields of the current year's public holidays.
pub async fn available_regions(
    State(app_state): State<AppState>,
    Path(country): Path<String>,
    _requester: User,
) -> AppResult<Json<Vec<String>>> {
    Ok(Json(
        crate::services::holidays::fetch_available_regions(&app_state.pool, &country).await?,
    ))
}
