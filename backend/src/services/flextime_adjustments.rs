//! Admin-made, dated changes to an employee's flextime balance.
//!
//! Every flextime balance in the app is "worked minutes minus target minutes,
//! accumulated day by day". Some changes to that balance have no worked hours
//! behind them at all: the hours somebody already carried when their account
//! was opened, an overtime payout, a negotiated reset. Those are recorded here
//! as one dated booking each, and the report pipeline folds them into the
//! ledger on their effective date exactly the way it folds in a day's
//! worked-minus-target difference.
//!
//! This replaces the old `users.overtime_start_balance_min` setting, where a
//! single editable number silently rewrote a person's entire flextime history
//! whenever it changed. See migration 043.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::repository::{
    FlextimeAdjustment, FlextimeAdjustmentDb, KIND_CORRECTION, MAX_ADJUSTMENT_MIN,
};
use crate::roles::is_assistant_role;
use crate::AppState;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Longest note accepted with a booking.
const MAX_REASON_LEN: usize = 500;

/// One flextime account as the admin UI shows it: the running balance plus
/// every booking that ever moved it.
#[derive(Serialize)]
pub struct FlextimeAccount {
    pub user_id: i64,
    pub user_name: String,
    /// FALSE for assistants (no flextime account at all) and for pure-admin
    /// users with time tracking switched off. The UI hides the whole booking
    /// form in that case; the API rejects writes for the same reason.
    pub has_flextime_account: bool,
    /// The employee's contract start date — the earliest effective date a
    /// booking may carry, and the date the ledger begins at.
    pub start_date: NaiveDate,
    /// Current balance, or `None` when the user has no flextime account.
    pub balance_min: Option<i64>,
    /// Date the balance is stated as of (end of the last fully approved week).
    pub balance_as_of: Option<NaiveDate>,
    pub adjustments: Vec<FlextimeAdjustment>,
}

/// Payload for booking one adjustment.
#[derive(Deserialize)]
pub struct NewAdjustment {
    pub effective_date: NaiveDate,
    pub minutes: i64,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Whether this user can have a flextime account at all. Assistants are paid
/// for the hours they are present and have no work target, so there is no
/// balance to correct; a pure-admin user has no time data whatsoever.
fn has_flextime_account(user: &crate::repository::User) -> bool {
    !is_assistant_role(&user.role) && user.tracks_time
}

/// Read one user's flextime account. Visible to admins, to a team lead for
/// their own direct reports, and to the employee themselves — reading your own
/// balance history is the only way a change to it is ever explainable.
pub async fn account(
    app_state: &AppState,
    requester: &User,
    target_user_id: i64,
) -> AppResult<FlextimeAccount> {
    crate::services::users::assert_can_access_user(app_state, requester, target_user_id).await?;
    let target = app_state
        .db
        .users
        .find_by_id(target_user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let user_name = format!("{} {}", target.first_name, target.last_name);
    let start_date = target.start_date;
    let eligible = has_flextime_account(&target);
    let adjustments = app_state
        .db
        .flextime_adjustments
        .list_for_user(target_user_id)
        .await?;

    let (balance_min, balance_as_of) = if eligible {
        let auth_user = crate::services::users::repo_user_to_auth_user(target);
        // The ledger over a single day at the cutoff yields that day's running
        // balance, which is exactly the number every other view calls "the
        // flextime balance".
        let cutoff = crate::services::reports::flex_balance_cutoff_date(
            &app_state.pool,
            target_user_id,
            auth_user.start_date,
            auth_user.workdays_per_week,
        )
        .await?;
        let today = crate::services::settings::app_today(&app_state.pool).await;
        // Read the ledger through today rather than through the cutoff so a
        // booking dated after the last approved week is visible immediately —
        // otherwise an admin would book a correction and see nothing change.
        let read_through = today.max(cutoff);
        let balance = if read_through < auth_user.start_date {
            0
        } else {
            let (days, _) = crate::services::reports::build_flextime_for_user(
                &app_state.pool,
                &auth_user,
                read_through,
                read_through,
            )
            .await?;
            days.first().map(|day| day.cumulative_min).unwrap_or(0)
        };
        (Some(balance), Some(cutoff))
    } else {
        (None, None)
    };

    Ok(FlextimeAccount {
        user_id: target_user_id,
        user_name,
        has_flextime_account: eligible,
        start_date,
        balance_min,
        balance_as_of,
        adjustments,
    })
}

/// Book one adjustment. Admin only: this moves an employee's balance without
/// any worked time behind it, so it is deliberately not delegated to team
/// leads the way assistant management is.
pub async fn create(
    app_state: &AppState,
    requester: &User,
    target_user_id: i64,
    body: NewAdjustment,
) -> AppResult<FlextimeAdjustment> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let target = app_state
        .db
        .users
        .find_by_id(target_user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !has_flextime_account(&target) {
        return Err(AppError::BadRequest(
            "This user has no flextime account.".into(),
        ));
    }
    if body.minutes == 0 {
        return Err(AppError::BadRequest(
            "The adjustment must not be zero.".into(),
        ));
    }
    if !(-MAX_ADJUSTMENT_MIN..=MAX_ADJUSTMENT_MIN).contains(&body.minutes) {
        return Err(AppError::BadRequest("The adjustment is too large.".into()));
    }
    // Any date from the contract start onwards, including one still ahead —
    // an overtime payout agreed for the end of next month is recorded when it
    // is agreed, and takes effect when that day arrives. Only dates before the
    // ledger itself begins are refused, and refused rather than silently moved
    // so the admin is never shown a date that was not stored.
    if body.effective_date < target.start_date {
        return Err(AppError::BadRequest(
            "The date must be on or after the user's start date.".into(),
        ));
    }
    let reason = normalize_reason(body.reason)?;

    let mut transaction = app_state.db.users.begin().await?;
    let new_id = FlextimeAdjustmentDb::create_tx(
        &mut transaction,
        target_user_id,
        body.effective_date,
        body.minutes,
        KIND_CORRECTION,
        reason.as_deref(),
        Some(requester.id),
        None,
    )
    .await?;
    transaction.commit().await?;

    // A booking changes the closing balance of every archived month from its
    // own date through today, so any of those months already exported (PDF
    // uploaded to Nextcloud) is now stale and must be regenerated. A future
    // effective_date re-queues nothing (start > end), matching "not applied
    // yet" — see the module doc.
    let today = crate::services::settings::app_today(&app_state.pool).await;
    crate::services::reports::requeue_export_for_absence_period(
        &app_state.pool,
        target_user_id,
        body.effective_date,
        today,
    )
    .await;

    let created = app_state
        .db
        .flextime_adjustments
        .find_by_id(new_id)
        .await?
        .ok_or(AppError::NotFound)?;
    crate::audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "flextime_adjustments",
        new_id,
        None,
        serde_json::to_value(&created).ok(),
    )
    .await;
    Ok(created)
}

/// Cancel a booking out by writing its opposite on the same date.
///
/// Admin only, and deliberately not a delete: removing a row would move every
/// balance reported since its date with nothing left to show what happened —
/// the exact problem the editable carry-in setting used to have. The reversal
/// restores the balance while both rows stay on the record.
pub async fn reverse(
    app_state: &AppState,
    requester: &User,
    id: i64,
) -> AppResult<FlextimeAdjustment> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let original = app_state
        .db
        .flextime_adjustments
        .find_by_id(id)
        .await?
        .ok_or(AppError::NotFound)?;
    if original.reverses_id.is_some() {
        return Err(AppError::BadRequest(
            "This entry is itself a reversal and cannot be reversed.".into(),
        ));
    }
    if original.reversed {
        return Err(AppError::BadRequest(
            "This entry has already been reversed.".into(),
        ));
    }

    let mut transaction = app_state.db.users.begin().await?;
    // The unique index on `reverses_id` is the real guard against two
    // reversals racing each other; the check above only turns the common case
    // into a readable message.
    let new_id = FlextimeAdjustmentDb::create_tx(
        &mut transaction,
        original.user_id,
        original.effective_date,
        -original.minutes,
        KIND_CORRECTION,
        None,
        Some(requester.id),
        Some(original.id),
    )
    .await
    .map_err(|e| {
        tracing::warn!(target: "zerf::flextime", "reverse adjustment failed: {e}");
        AppError::Conflict("This entry has already been reversed.".into())
    })?;
    transaction.commit().await?;

    // Same reasoning as `create`: the reversal changes archived months' closing
    // balances too, dated the same as the entry it cancels.
    let today = crate::services::settings::app_today(&app_state.pool).await;
    crate::services::reports::requeue_export_for_absence_period(
        &app_state.pool,
        original.user_id,
        original.effective_date,
        today,
    )
    .await;

    let created = app_state
        .db
        .flextime_adjustments
        .find_by_id(new_id)
        .await?
        .ok_or(AppError::NotFound)?;
    crate::audit::log(
        &app_state.pool,
        requester.id,
        "reversed",
        "flextime_adjustments",
        original.id,
        serde_json::to_value(&original).ok(),
        serde_json::to_value(&created).ok(),
    )
    .await;
    Ok(created)
}

/// Trim the note and reject an over-long one. An empty note is stored as NULL
/// so "no note" has a single representation.
fn normalize_reason(reason: Option<String>) -> AppResult<Option<String>> {
    let Some(raw) = reason else { return Ok(None) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_REASON_LEN {
        return Err(AppError::BadRequest("The note is too long.".into()));
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_reason_trims_and_nulls_blank_input() {
        assert_eq!(normalize_reason(None).unwrap(), None);
        assert_eq!(normalize_reason(Some("   ".into())).unwrap(), None);
        assert_eq!(
            normalize_reason(Some("  payout 2026  ".into())).unwrap(),
            Some("payout 2026".to_string())
        );
    }

    #[test]
    fn normalize_reason_rejects_over_long_notes() {
        let long = "a".repeat(MAX_REASON_LEN + 1);
        assert!(normalize_reason(Some(long)).is_err());
        let exact = "a".repeat(MAX_REASON_LEN);
        assert!(normalize_reason(Some(exact)).is_ok());
    }

    #[test]
    fn normalize_reason_counts_characters_not_bytes() {
        // A multi-byte note at the character limit must still be accepted;
        // counting bytes would reject it at roughly a third of the length.
        let umlauts = "ü".repeat(MAX_REASON_LEN);
        assert!(normalize_reason(Some(umlauts)).is_ok());
    }
}
