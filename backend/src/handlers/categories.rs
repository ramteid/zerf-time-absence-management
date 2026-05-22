use crate::error::AppResult;
use crate::middleware::auth::User;
use crate::services::categories::{self, Category};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Deserializer};

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

pub async fn list(
    State(app_state): State<AppState>,
    _requester: User,
) -> AppResult<Json<Vec<Category>>> {
    Ok(Json(categories::list(&app_state).await?))
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<Category>>> {
    Ok(Json(categories::list_all(&app_state, &requester).await?))
}

#[derive(Deserialize)]
pub struct NewCategory {
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub sort_order: Option<i64>,
    pub counts_as_work: Option<bool>,
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewCategory>,
) -> AppResult<Json<Category>> {
    Ok(Json(
        categories::create(
            &app_state,
            &requester,
            body.name,
            body.description,
            body.color,
            body.sort_order,
            body.counts_as_work,
        )
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct UpdateCategory {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub description: Option<Option<String>>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub counts_as_work: Option<bool>,
    pub active: Option<bool>,
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(category_id): Path<i64>,
    Json(body): Json<UpdateCategory>,
) -> AppResult<Json<Category>> {
    Ok(Json(
        categories::update(
            &app_state,
            &requester,
            category_id,
            categories::UpdateCategory {
                name: body.name,
                description: body.description,
                color: body.color,
                sort_order: body.sort_order,
                counts_as_work: body.counts_as_work,
                active: body.active,
            },
        )
        .await?,
    ))
}
