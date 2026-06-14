use crate::error::AppResult;
use crate::middleware::auth::User;
use crate::services::absence_categories::{self, AbsenceCategory};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;

pub async fn list(
    State(app_state): State<AppState>,
    _requester: User,
) -> AppResult<Json<Vec<AbsenceCategory>>> {
    Ok(Json(absence_categories::list_active(&app_state).await?))
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<AbsenceCategory>>> {
    Ok(Json(
        absence_categories::list_all(&app_state, &requester).await?,
    ))
}

fn default_cost_type() -> String {
    crate::repository::absence_categories::COST_TYPE_NONE.to_string()
}

#[derive(Deserialize)]
pub struct NewAbsenceCategoryRequest {
    pub slug: Option<String>,
    pub name: String,
    pub color: String,
    pub sort_order: Option<i64>,
    /// `'none'` | `'vacation'` | `'flextime'`. Replaces the pre-019
    /// `counts_as_vacation` / `keeps_work_target` boolean pair.
    #[serde(default = "default_cost_type")]
    pub cost_type: String,
    #[serde(default)]
    pub auto_approve_past: bool,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewAbsenceCategoryRequest>,
) -> AppResult<Json<AbsenceCategory>> {
    Ok(Json(
        absence_categories::create(
            &app_state,
            &requester,
            absence_categories::NewCategoryInput {
                slug: body.slug,
                name: body.name,
                color: body.color,
                sort_order: body.sort_order,
                cost_type: body.cost_type,
                auto_approve_past: body.auto_approve_past,
            },
        )
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct UpdateAbsenceCategoryRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
    pub cost_type: Option<String>,
    pub auto_approve_past: Option<bool>,
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(category_id): Path<i64>,
    Json(body): Json<UpdateAbsenceCategoryRequest>,
) -> AppResult<Json<AbsenceCategory>> {
    Ok(Json(
        absence_categories::update(
            &app_state,
            &requester,
            category_id,
            absence_categories::UpdateCategoryInput {
                name: body.name,
                color: body.color,
                sort_order: body.sort_order,
                active: body.active,
                cost_type: body.cost_type,
                auto_approve_past: body.auto_approve_past,
            },
        )
        .await?,
    ))
}
