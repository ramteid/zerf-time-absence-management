use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::AppState;

pub use crate::repository::categories::Category;

pub async fn ensure_initial(pool: &crate::db::DatabasePool) -> AppResult<()> {
    let db = crate::repository::CategoryDb::new(pool.clone());
    db.ensure_initial().await
}

fn is_valid_hex_color(color: &str) -> bool {
    let bytes = color.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

/// Active categories enabled for the given employee — used to populate the
/// time-entry dropdown so each employee only sees what they're allowed to use.
pub async fn list_for_user(app_state: &AppState, user_id: i64) -> AppResult<Vec<Category>> {
    app_state.db.categories.list_active_for_user(user_id).await
}

pub async fn list_all(app_state: &AppState, requester: &User) -> AppResult<Vec<Category>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    app_state.db.categories.list_all().await
}

/// Employee ids currently enabled for a category. Admin-only.
pub async fn category_users(
    app_state: &AppState,
    requester: &User,
    category_id: i64,
) -> AppResult<Vec<i64>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    app_state
        .db
        .categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)?;
    app_state.db.categories.enabled_user_ids(category_id).await
}

/// Replace the full set of employees enabled for a category. Admin-only.
pub async fn set_category_users(
    app_state: &AppState,
    requester: &User,
    category_id: i64,
    user_ids: Vec<i64>,
) -> AppResult<()> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    app_state
        .db
        .categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)?;
    app_state
        .db
        .categories
        .set_enabled_user_ids(category_id, &user_ids)
        .await
}

pub async fn create(
    app_state: &AppState,
    requester: &User,
    name: String,
    description: Option<String>,
    color: String,
    sort_order: Option<i64>,
    counts_as_work: Option<bool>,
) -> AppResult<Category> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let name = name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::BadRequest("Invalid category name.".into()));
    }
    let color = color.trim().to_string();
    if !is_valid_hex_color(&color) {
        return Err(AppError::BadRequest("Invalid color.".into()));
    }
    let new_id = app_state
        .db
        .categories
        .create(
            &name,
            description.as_deref(),
            &color,
            sort_order.unwrap_or(0),
            counts_as_work.unwrap_or(true),
        )
        .await?;
    app_state
        .db
        .categories
        .find_by_id(new_id)
        .await?
        .ok_or_else(|| AppError::Internal("Created category not found".into()))
}

pub struct UpdateCategory {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub counts_as_work: Option<bool>,
    pub active: Option<bool>,
}

pub async fn update(
    app_state: &AppState,
    requester: &User,
    category_id: i64,
    body: UpdateCategory,
) -> AppResult<Category> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if let Some(ref new_name) = body.name {
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed.len() > 200 {
            return Err(AppError::BadRequest("Invalid category name.".into()));
        }
    }
    if let Some(ref new_color) = body.color {
        let trimmed = new_color.trim();
        if !is_valid_hex_color(trimmed) {
            return Err(AppError::BadRequest("Invalid color.".into()));
        }
    }
    let normalized_name = body.name.map(|n| n.trim().to_string());
    let normalized_color = body.color.map(|c| c.trim().to_string());
    app_state
        .db
        .categories
        .update(
            category_id,
            normalized_name,
            body.description,
            normalized_color,
            body.sort_order,
            body.counts_as_work,
            body.active,
        )
        .await?;
    app_state
        .db
        .categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_hex_color_accepts_six_digit_hex_with_hash() {
        assert!(is_valid_hex_color("#1a2b3c"));
        assert!(is_valid_hex_color("#FFFFFF"));
        assert!(is_valid_hex_color("#000000"));
        assert!(is_valid_hex_color("#aAbBcC"));
        assert!(is_valid_hex_color("#012345"));
        assert!(is_valid_hex_color("#6789ab"));
    }

    #[test]
    fn is_valid_hex_color_rejects_invalid_inputs() {
        assert!(!is_valid_hex_color(""));
        assert!(!is_valid_hex_color("1a2b3c")); // missing #
        assert!(!is_valid_hex_color("#1a2b3")); // 5 hex digits (too short)
        assert!(!is_valid_hex_color("#1a2b3cd")); // 7 hex digits (too long)
        assert!(!is_valid_hex_color("#1g2b3c")); // 'g' is not hex
        assert!(!is_valid_hex_color("#")); // just hash
        assert!(!is_valid_hex_color("##aabbcc")); // double hash
        assert!(!is_valid_hex_color("#rgb(0,0,0)")); // not hex
    }
}
