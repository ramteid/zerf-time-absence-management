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

#[derive(Deserialize)]
pub struct NewAbsenceCategoryRequest {
    pub slug: Option<String>,
    pub name: String,
    pub color: String,
    pub sort_order: Option<i64>,
    #[serde(default)]
    pub counts_as_vacation: bool,
    #[serde(default)]
    pub keeps_work_target: bool,
    #[serde(default)]
    pub auto_approve_past: bool,
    #[serde(default)]
    pub team_visible: bool,
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
                counts_as_vacation: body.counts_as_vacation,
                keeps_work_target: body.keeps_work_target,
                auto_approve_past: body.auto_approve_past,
                team_visible: body.team_visible,
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
    pub counts_as_vacation: Option<bool>,
    pub keeps_work_target: Option<bool>,
    pub auto_approve_past: Option<bool>,
    pub team_visible: Option<bool>,
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
                counts_as_vacation: body.counts_as_vacation,
                keeps_work_target: body.keeps_work_target,
                auto_approve_past: body.auto_approve_past,
                team_visible: body.team_visible,
            },
        )
        .await?,
    ))
}
