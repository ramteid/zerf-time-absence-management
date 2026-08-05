use crate::error::AppResult;
use crate::middleware::auth::User;
use crate::services::absence_categories::{self, AbsenceCategory};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

/// Public category shape. The internal leave-account start year is deliberately
/// omitted: it controls historical entitlement calculations but is not an
/// editable or visible category setting.
#[derive(Serialize)]
pub struct AbsenceCategoryResponse {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub color: String,
    pub sort_order: i64,
    pub active: bool,
    pub cost_type: String,
    pub auto_approve_past: bool,
    pub unpaid: bool,
    pub leave_account_default_days: Option<i64>,
    pub leave_account_carryover_expiry: Option<String>,
}

impl From<AbsenceCategory> for AbsenceCategoryResponse {
    fn from(category: AbsenceCategory) -> Self {
        Self {
            id: category.id,
            slug: category.slug,
            name: category.name,
            color: category.color,
            sort_order: category.sort_order,
            active: category.active,
            cost_type: category.cost_type,
            auto_approve_past: category.auto_approve_past,
            unpaid: category.unpaid,
            leave_account_default_days: category.leave_account_default_days,
            leave_account_carryover_expiry: category.leave_account_carryover_expiry,
        }
    }
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<AbsenceCategoryResponse>>> {
    let categories = absence_categories::list_for_user(&app_state, requester.id).await?;
    Ok(Json(categories.into_iter().map(Into::into).collect()))
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<AbsenceCategoryResponse>>> {
    let categories = absence_categories::list_all(&app_state, &requester).await?;
    Ok(Json(categories.into_iter().map(Into::into).collect()))
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
    #[serde(default)]
    pub unpaid: bool,
    /// Required for a category with `cost_type = "vacation"` and otherwise
    /// rejected. Existing user accounts keep their own base values.
    pub leave_account_default_days: Option<i64>,
    /// Required for a category with `cost_type = "vacation"` in `MM-DD`
    /// format and otherwise rejected.
    pub leave_account_carryover_expiry: Option<String>,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewAbsenceCategoryRequest>,
) -> AppResult<Json<AbsenceCategoryResponse>> {
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
                unpaid: body.unpaid,
                leave_account_default_days: body.leave_account_default_days,
                leave_account_carryover_expiry: body.leave_account_carryover_expiry,
            },
        )
        .await?
        .into(),
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
    pub unpaid: Option<bool>,
    pub leave_account_default_days: Option<i64>,
    pub leave_account_carryover_expiry: Option<String>,
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(category_id): Path<i64>,
    Json(body): Json<UpdateAbsenceCategoryRequest>,
) -> AppResult<Json<AbsenceCategoryResponse>> {
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
                unpaid: body.unpaid,
                leave_account_default_days: body.leave_account_default_days,
                leave_account_carryover_expiry: body.leave_account_carryover_expiry,
            },
        )
        .await?
        .into(),
    ))
}

pub async fn list_users(
    State(app_state): State<AppState>,
    requester: User,
    Path(category_id): Path<i64>,
) -> AppResult<Json<Vec<i64>>> {
    Ok(Json(
        absence_categories::category_users(&app_state, &requester, category_id).await?,
    ))
}

#[derive(Deserialize)]
pub struct SetCategoryUsers {
    pub user_ids: Vec<i64>,
}

#[derive(Serialize)]
pub struct Ack {
    pub ok: bool,
}

pub async fn set_users(
    State(app_state): State<AppState>,
    requester: User,
    Path(category_id): Path<i64>,
    Json(body): Json<SetCategoryUsers>,
) -> AppResult<Json<Ack>> {
    absence_categories::set_category_users(&app_state, &requester, category_id, body.user_ids)
        .await?;
    Ok(Json(Ack { ok: true }))
}
