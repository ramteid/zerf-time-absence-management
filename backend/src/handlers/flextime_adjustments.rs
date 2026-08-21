use crate::error::AppResult;
use crate::middleware::auth::User;
use crate::repository::FlextimeAdjustment;
use crate::services::flextime_adjustments::{self, FlextimeAccount, NewAdjustment};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};

/// GET /users/{id}/flextime-account — the user's balance plus every admin
/// booking behind it. Readable by admins, the user's approvers, and the user.
pub async fn get_account(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
) -> AppResult<Json<FlextimeAccount>> {
    Ok(Json(
        flextime_adjustments::account(&app_state, &requester, user_id).await?,
    ))
}

/// POST /users/{id}/flextime-adjustments — book one adjustment (admin only).
pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Path(user_id): Path<i64>,
    Json(body): Json<NewAdjustment>,
) -> AppResult<Json<FlextimeAdjustment>> {
    Ok(Json(
        flextime_adjustments::create(&app_state, &requester, user_id, body).await?,
    ))
}

/// POST /flextime-adjustments/{id}/reverse — cancel a booking out by writing
/// its opposite on the same date (admin only). There is no delete: removing a
/// row would move every balance reported since its date with nothing left on
/// the record to explain it.
pub async fn reverse(
    State(app_state): State<AppState>,
    requester: User,
    Path(id): Path<i64>,
) -> AppResult<Json<FlextimeAdjustment>> {
    Ok(Json(
        flextime_adjustments::reverse(&app_state, &requester, id).await?,
    ))
}
