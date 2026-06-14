use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::repository::absence_categories::{NewAbsenceCategory, UpdateAbsenceCategory};
use crate::AppState;

pub use crate::repository::AbsenceCategory;

fn is_valid_hex_color(color: &str) -> bool {
    let bytes = color.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

/// Normalize a user-supplied slug into the URL-safe form the DB constraint
/// requires (`^[a-z][a-z0-9_]*$`). Lowercases, replaces non-alphanumerics with
/// underscores, and collapses repeats. Returns `None` if the result would not
/// satisfy the constraint (empty or starting with a digit) so callers can
/// surface a 400 before sqlx maps it to a generic constraint error.
fn normalize_slug(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut prev_underscore = false;
    for char in raw.trim().chars() {
        // Alphanumeric → keep (lowercase). Any other character → separator
        // underscore (non-ASCII characters are skipped entirely since slug
        // characters are restricted to a-z, 0-9, _).
        let mapped = if char.is_ascii_alphanumeric() {
            Some(char.to_ascii_lowercase())
        } else if char.is_ascii() {
            Some('_')
        } else {
            None
        };
        if let Some(mapped_char) = mapped {
            if mapped_char == '_' {
                if prev_underscore || out.is_empty() {
                    continue;
                }
                prev_underscore = true;
            } else {
                prev_underscore = false;
            }
            out.push(mapped_char);
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    let first = out.chars().next()?;
    if !first.is_ascii_lowercase() {
        return None;
    }
    Some(out)
}

pub async fn list_active(app_state: &AppState) -> AppResult<Vec<AbsenceCategory>> {
    app_state.db.absence_categories.list_active().await
}

pub async fn list_all(app_state: &AppState, requester: &User) -> AppResult<Vec<AbsenceCategory>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    app_state.db.absence_categories.list_all().await
}

pub struct NewCategoryInput {
    pub slug: Option<String>,
    pub name: String,
    pub color: String,
    pub sort_order: Option<i64>,
    pub cost_type: String,
    pub auto_approve_past: bool,
}

pub async fn create(
    app_state: &AppState,
    requester: &User,
    input: NewCategoryInput,
) -> AppResult<AbsenceCategory> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let name = input.name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::BadRequest("Invalid category name.".into()));
    }
    if !is_valid_hex_color(input.color.trim()) {
        return Err(AppError::BadRequest("Invalid color.".into()));
    }
    // Reject unknown cost_type strings up front so the DB CHECK is a backup
    // for direct-SQL bypass, not the user-facing validation.
    crate::repository::absence_categories::validate_cost_type(&input.cost_type)?;
    let slug = match input.slug.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => normalize_slug(raw).ok_or_else(|| {
            AppError::BadRequest(
                "Slug must contain at least one letter and use only a-z, 0-9, _.".into(),
            )
        })?,
        None => normalize_slug(&name).ok_or_else(|| {
            AppError::BadRequest("Name must contain at least one letter to derive a slug.".into())
        })?,
    };
    let color = input.color.trim().to_string();
    let new_id = app_state
        .db
        .absence_categories
        .create(NewAbsenceCategory {
            slug: &slug,
            name: &name,
            color: &color,
            sort_order: input.sort_order.unwrap_or(0),
            active: true,
            cost_type: &input.cost_type,
            auto_approve_past: input.auto_approve_past,
        })
        .await?;
    app_state
        .db
        .absence_categories
        .find_by_id(new_id)
        .await?
        .ok_or_else(|| AppError::Internal("Created absence category not found".into()))
}

pub struct UpdateCategoryInput {
    pub name: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
    pub cost_type: Option<String>,
    pub auto_approve_past: Option<bool>,
}

pub async fn update(
    app_state: &AppState,
    requester: &User,
    category_id: i64,
    input: UpdateCategoryInput,
) -> AppResult<AbsenceCategory> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if let Some(ref new_name) = input.name {
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed.len() > 200 {
            return Err(AppError::BadRequest("Invalid category name.".into()));
        }
    }
    if let Some(ref new_color) = input.color {
        if !is_valid_hex_color(new_color.trim()) {
            return Err(AppError::BadRequest("Invalid color.".into()));
        }
    }
    // We need to know whether the resulting row would violate the
    // vacation-XOR-flextime invariant; load the current values and merge in
    // whichever flag is being changed so we can decide whether the
    // Bug-9 in-use lock applies.
    let current = app_state
        .db
        .absence_categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(ref new_cost_type) = input.cost_type {
        crate::repository::absence_categories::validate_cost_type(new_cost_type)?;
    }
    let final_cost_type = input
        .cost_type
        .clone()
        .unwrap_or_else(|| current.cost_type.clone());
    let final_auto = input.auto_approve_past.unwrap_or(current.auto_approve_past);
    // Toggling the cost type or the auto-approve behavior would silently
    // change the financial / approval meaning of EXISTING absences
    // referencing this category — past balance recomputations would
    // suddenly debit/credit different ledgers, and approval-flow guards
    // would relax or tighten without the affected employees seeing it.
    // We refuse such changes whenever there is at least one absence row
    // in this category. Admins who need a different policy must
    // deactivate the existing category and create a new one.
    //
    // Renames, color, sort_order, and active toggles are safe and pass
    // through.
    let cost_type_changed = final_cost_type != current.cost_type;
    let auto_changed = final_auto != current.auto_approve_past;
    if cost_type_changed || auto_changed {
        let usage = app_state
            .db
            .absence_categories
            .usage_count(category_id)
            .await?;
        if usage > 0 {
            return Err(AppError::BadRequest(
                "Cannot change the cost or approval behavior of a category that \
                 already has absences. Deactivate this category and create a new one \
                 with the desired flags instead."
                    .into(),
            ));
        }
    }
    let normalized_name = input.name.map(|n| n.trim().to_string());
    let normalized_color = input.color.map(|c| c.trim().to_string());
    app_state
        .db
        .absence_categories
        .update(
            category_id,
            UpdateAbsenceCategory {
                name: normalized_name.as_deref(),
                color: normalized_color.as_deref(),
                sort_order: input.sort_order,
                active: input.active,
                cost_type: input.cost_type.as_deref(),
                auto_approve_past: input.auto_approve_past,
            },
        )
        .await?;
    app_state
        .db
        .absence_categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_slug_accepts_simple_lowercase() {
        assert_eq!(normalize_slug("vacation").as_deref(), Some("vacation"));
    }

    #[test]
    fn normalize_slug_lowercases_and_replaces_spaces() {
        assert_eq!(
            normalize_slug("Bereavement Leave").as_deref(),
            Some("bereavement_leave")
        );
    }

    #[test]
    fn normalize_slug_collapses_repeated_separators() {
        assert_eq!(
            normalize_slug("---Foo  --  Bar---").as_deref(),
            Some("foo_bar")
        );
    }

    #[test]
    fn normalize_slug_drops_punctuation() {
        assert_eq!(normalize_slug("Care/Other!").as_deref(), Some("care_other"));
    }

    #[test]
    fn normalize_slug_rejects_empty_and_digit_prefixed() {
        assert!(normalize_slug("").is_none());
        assert!(normalize_slug("   ").is_none());
        // Leading digit fails the constraint (slug must start with [a-z]).
        assert!(normalize_slug("123abc").is_none());
    }

    #[test]
    fn is_valid_hex_color_accepts_six_digit_hex_with_hash() {
        assert!(is_valid_hex_color("#1a2b3c"));
        assert!(is_valid_hex_color("#FFFFFF"));
    }

    #[test]
    fn is_valid_hex_color_rejects_invalid_inputs() {
        assert!(!is_valid_hex_color(""));
        assert!(!is_valid_hex_color("1a2b3c"));
        assert!(!is_valid_hex_color("#1a2b3"));
        assert!(!is_valid_hex_color("#1g2b3c"));
    }
}
