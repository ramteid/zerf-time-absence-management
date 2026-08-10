use crate::audit;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::repository::absence_categories::{NewAbsenceCategory, UpdateAbsenceCategory};
use crate::repository::{AbsenceCategoryDb, UserDb};
use crate::AppState;

pub use crate::repository::AbsenceCategory;

fn is_valid_hex_color(color: &str) -> bool {
    let bytes = color.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_leave_account_carryover_expiry(value: &str) -> AppResult<String> {
    let value = value.trim();
    let Some((month, day)) = value.split_once('-') else {
        return Err(AppError::BadRequest(
            "Leave-account carryover expiry must use MM-DD.".into(),
        ));
    };
    if month.len() != 2 || day.len() != 2 || day.contains('-') {
        return Err(AppError::BadRequest(
            "Leave-account carryover expiry must use MM-DD.".into(),
        ));
    }
    let month: u32 = month.parse().map_err(|_| {
        AppError::BadRequest("Invalid month in leave-account carryover expiry.".into())
    })?;
    let day: u32 = day.parse().map_err(|_| {
        AppError::BadRequest("Invalid day in leave-account carryover expiry.".into())
    })?;
    if chrono::NaiveDate::from_ymd_opt(2024, month, day).is_none() {
        return Err(AppError::BadRequest(
            "Invalid leave-account carryover expiry.".into(),
        ));
    }
    Ok(format!("{month:02}-{day:02}"))
}

fn validate_leave_account_days(days: i64) -> AppResult<()> {
    if !(0..=366).contains(&days) {
        return Err(AppError::BadRequest(
            "Leave-account days must be between 0 and 366.".into(),
        ));
    }
    Ok(())
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

/// Employee-scoped absence categories for the frontend store.
/// Used to populate the frontend `absenceCategories` store, which provides
/// both the request dropdown (filtered to `active=true` client-side) and the
/// behavior lookups (`absenceRemovesTarget`, `absenceBlocksEntry`) that must
/// resolve deactivated or access-revoked categories whose live absence rows
/// still carry their original behavior. For this endpoint, `active=false` can
/// mean either globally inactive or no longer enabled for this employee.
pub async fn list_for_user(app_state: &AppState, user_id: i64) -> AppResult<Vec<AbsenceCategory>> {
    app_state
        .db
        .absence_categories
        .list_all_for_user(user_id)
        .await
}

pub async fn list_all(app_state: &AppState, requester: &User) -> AppResult<Vec<AbsenceCategory>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    app_state.db.absence_categories.list_all().await
}

/// Employee ids currently enabled for an absence category. Admin-only.
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
        .absence_categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)?;
    app_state
        .db
        .absence_categories
        .enabled_user_ids(category_id)
        .await
}

/// Replace the full set of employees enabled for an absence category.
/// Admin-only.
///
/// For a leave-account category, access changes are reconciled with each
/// affected user's `user_leave_accounts` row in the same transaction as the
/// access change: a user who loses access has their account zeroed (and its
/// yearly overrides cleared); a user who gains access has it restored to the
/// category default. This keeps a user's visible entitlement from ever
/// outliving the access decision that grants it — see the leave-account
/// access addendum in `PLAN.md`. Non-leave-account categories keep the
/// original, simpler access-only replacement.
pub async fn set_category_users(
    app_state: &AppState,
    requester: &User,
    category_id: i64,
    user_ids: Vec<i64>,
) -> AppResult<()> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let category = app_state
        .db
        .absence_categories
        .find_by_id(category_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if !category.has_leave_account() {
        let mut transaction = app_state.db.users.begin().await?;
        crate::services::auth::lock_user_graph(&mut transaction).await?;
        AbsenceCategoryDb::set_enabled_user_ids_tx(&mut transaction, category_id, &user_ids)
            .await?;
        transaction.commit().await?;
        return Ok(());
    }

    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
    let next_year = current_year + 1;
    let new_ids: std::collections::HashSet<i64> = user_ids.iter().copied().collect();

    let mut transaction = app_state.db.users.begin().await?;
    crate::services::auth::lock_user_graph(&mut transaction).await?;
    // Read the current access set under the lock, not before it, so a
    // concurrent access edit for the same category cannot make this diff
    // stale between the read and the write below.
    let previous_ids: std::collections::HashSet<i64> =
        AbsenceCategoryDb::enabled_user_ids_tx(&mut transaction, category_id)
            .await?
            .into_iter()
            .collect();
    let removed_ids: Vec<i64> = previous_ids.difference(&new_ids).copied().collect();
    let added_ids: Vec<i64> = new_ids.difference(&previous_ids).copied().collect();
    AbsenceCategoryDb::set_enabled_user_ids_tx(&mut transaction, category_id, &user_ids).await?;
    for user_id in removed_ids {
        UserDb::revoke_leave_account_tx(&mut transaction, user_id, category_id).await?;
    }
    for user_id in added_ids {
        UserDb::grant_leave_account_tx(
            &mut transaction,
            user_id,
            category_id,
            current_year,
            next_year,
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub struct NewCategoryInput {
    pub slug: Option<String>,
    pub name: String,
    pub color: String,
    pub sort_order: Option<i64>,
    pub cost_type: String,
    pub auto_approve_past: bool,
    pub unpaid: bool,
    pub medical_certificate_relevant: bool,
    pub leave_account_default_days: Option<i64>,
    pub leave_account_carryover_expiry: Option<String>,
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
    let is_leave_account =
        input.cost_type == crate::repository::absence_categories::COST_TYPE_VACATION;
    let leave_account_default_days = if is_leave_account {
        let days = input.leave_account_default_days.ok_or_else(|| {
            AppError::BadRequest(
                "Leave-account default days are required for vacation categories.".into(),
            )
        })?;
        validate_leave_account_days(days)?;
        Some(days)
    } else {
        if input.leave_account_default_days.is_some()
            || input.leave_account_carryover_expiry.is_some()
        {
            return Err(AppError::BadRequest(
                "Leave-account fields are only allowed for vacation categories.".into(),
            ));
        }
        None
    };
    let leave_account_carryover_expiry = if is_leave_account {
        let expiry = input
            .leave_account_carryover_expiry
            .as_deref()
            .ok_or_else(|| {
                AppError::BadRequest(
                    "Leave-account carryover expiry is required for vacation categories.".into(),
                )
            })?;
        Some(normalize_leave_account_carryover_expiry(expiry)?)
    } else {
        None
    };
    // A category cannot simultaneously deduct vacation days AND auto-approve
    // itself: it would let employees bypass review for vacation deductions,
    // and would double-count days in the team report (vacation column AND
    // sick/auto-approve column see the same absences).
    if input.cost_type == crate::repository::absence_categories::COST_TYPE_VACATION
        && input.auto_approve_past
    {
        return Err(AppError::BadRequest(
            "A category cannot deduct leave days and auto-approve at the same time.".into(),
        ));
    }
    // "Unpaid" only makes sense for cost_type='none': vacation and flextime
    // categories are always paid through their own balance mechanics, so
    // marking either unpaid would be a contradictory, confusing state.
    if input.unpaid && input.cost_type != crate::repository::absence_categories::COST_TYPE_NONE {
        return Err(AppError::BadRequest(
            "Unpaid can only be set when cost_type is 'none'.".into(),
        ));
    }
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
    let leave_account_start_year = if is_leave_account {
        Some(crate::services::settings::app_current_year(&app_state.pool).await)
    } else {
        None
    };
    let mut transaction = app_state.db.users.begin().await?;
    crate::services::auth::lock_user_graph(&mut transaction).await?;
    let new_id = AbsenceCategoryDb::create_tx(
        &mut transaction,
        NewAbsenceCategory {
            slug: &slug,
            name: &name,
            color: &color,
            sort_order: input.sort_order.unwrap_or(0),
            active: true,
            cost_type: &input.cost_type,
            auto_approve_past: input.auto_approve_past,
            unpaid: input.unpaid,
            medical_certificate_relevant: input.medical_certificate_relevant,
            leave_account_default_days,
            leave_account_carryover_expiry: leave_account_carryover_expiry.as_deref(),
            leave_account_start_year,
        },
    )
    .await?;
    AbsenceCategoryDb::grant_default_access_to_all_users_tx(&mut transaction, new_id).await?;
    if is_leave_account {
        UserDb::seed_leave_accounts_for_category_tx(&mut transaction, new_id).await?;
    }
    let created = AbsenceCategoryDb::find_by_id_tx(&mut transaction, new_id)
        .await?
        .ok_or_else(|| AppError::Internal("Created absence category not found".into()))?;
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "absence_categories",
        new_id,
        None,
        serde_json::to_value(&created).ok(),
    )
    .await;
    Ok(created)
}

pub struct UpdateCategoryInput {
    pub name: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub active: Option<bool>,
    pub cost_type: Option<String>,
    pub auto_approve_past: Option<bool>,
    pub unpaid: Option<bool>,
    pub medical_certificate_relevant: Option<bool>,
    pub leave_account_default_days: Option<i64>,
    pub leave_account_carryover_expiry: Option<String>,
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
    let UpdateCategoryInput {
        name,
        color,
        sort_order,
        active,
        cost_type,
        auto_approve_past,
        unpaid,
        medical_certificate_relevant,
        leave_account_default_days,
        leave_account_carryover_expiry,
    } = input;
    if let Some(ref new_name) = name {
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed.len() > 200 {
            return Err(AppError::BadRequest("Invalid category name.".into()));
        }
    }
    if let Some(ref new_color) = color {
        if !is_valid_hex_color(new_color.trim()) {
            return Err(AppError::BadRequest("Invalid color.".into()));
        }
    }
    let mut transaction = app_state.db.users.begin().await?;
    let current = AbsenceCategoryDb::find_by_id_tx(&mut transaction, category_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(ref new_cost_type) = cost_type {
        crate::repository::absence_categories::validate_cost_type(new_cost_type)?;
    }
    let final_cost_type = cost_type
        .clone()
        .unwrap_or_else(|| current.cost_type.clone());
    let final_auto = auto_approve_past.unwrap_or(current.auto_approve_past);
    let final_unpaid = unpaid.unwrap_or(current.unpaid);
    let final_has_leave_account =
        final_cost_type == crate::repository::absence_categories::COST_TYPE_VACATION;
    if !current.has_leave_account() && final_has_leave_account {
        return Err(AppError::BadRequest(
            "An existing category cannot be changed into a leave-account category.".into(),
        ));
    }
    if current.has_leave_account() && !final_has_leave_account {
        return Err(AppError::BadRequest(
            "A leave-account category cannot be changed to another cost type.".into(),
        ));
    }
    let normalized_carryover_expiry = if final_has_leave_account {
        if let Some(days) = leave_account_default_days {
            validate_leave_account_days(days)?;
        }
        leave_account_carryover_expiry
            .as_deref()
            .map(normalize_leave_account_carryover_expiry)
            .transpose()?
    } else {
        if leave_account_default_days.is_some() || leave_account_carryover_expiry.is_some() {
            return Err(AppError::BadRequest(
                "Leave-account fields are only allowed for leave-account categories.".into(),
            ));
        }
        None
    };
    if final_cost_type == crate::repository::absence_categories::COST_TYPE_VACATION && final_auto {
        return Err(AppError::BadRequest(
            "A category cannot deduct leave days and auto-approve at the same time.".into(),
        ));
    }
    if final_unpaid && final_cost_type != crate::repository::absence_categories::COST_TYPE_NONE {
        return Err(AppError::BadRequest(
            "Unpaid can only be set when cost_type is 'none'.".into(),
        ));
    }
    let cost_type_changed = final_cost_type != current.cost_type;
    let auto_changed = final_auto != current.auto_approve_past;
    let unpaid_changed = final_unpaid != current.unpaid;
    // The default day count and the carryover expiry stay editable for a
    // category that is already in use. The default only seeds the per-user
    // entitlement when an account is first granted, so changing it never
    // rewrites anyone's existing balance — it just applies to whoever is
    // onboarded next. Cost type, unpaid and auto-approval are different:
    // those *would* re-interpret absences that were already booked, so they
    // remain locked once the category has been used.
    // Counted inside the transaction so the check cannot race the update.
    if cost_type_changed || auto_changed || unpaid_changed {
        let usage: i64 = AbsenceCategoryDb::usage_count_tx(&mut transaction, category_id).await?;
        if usage > 0 {
            return Err(AppError::BadRequest(
                "This category is already in use. Create a new one instead.".into(),
            ));
        }
    }
    let normalized_name = name.map(|value| value.trim().to_string());
    let normalized_color = color.map(|value| value.trim().to_string());
    AbsenceCategoryDb::update_tx(
        &mut transaction,
        category_id,
        UpdateAbsenceCategory {
            name: normalized_name.as_deref(),
            color: normalized_color.as_deref(),
            sort_order,
            active,
            cost_type: cost_type.as_deref(),
            auto_approve_past,
            unpaid,
            medical_certificate_relevant,
            leave_account_default_days,
            leave_account_carryover_expiry: normalized_carryover_expiry.as_deref(),
            leave_account_start_year: None,
        },
    )
    .await?;
    let updated = AbsenceCategoryDb::find_by_id_tx(&mut transaction, category_id)
        .await?
        .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "absence_categories",
        category_id,
        serde_json::to_value(&current).ok(),
        serde_json::to_value(&updated).ok(),
    )
    .await;
    Ok(updated)
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
