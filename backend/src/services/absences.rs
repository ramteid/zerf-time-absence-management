use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::absence_balance::{
    validate_absence_has_workday, validate_backdating_window, validate_flextime_balance,
    validate_vacation_balance, workdays,
};
use crate::AppState;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize, Serializer};

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    crate::services::notifications::load_language(pool).await
}

async fn notify_absence(
    app_state: &AppState,
    language: &i18n::Language,
    recipient_id: i64,
    event: &str,
    params: Vec<(&'static str, String)>,
    absence_id: i64,
) {
    crate::services::notifications::create_translated(
        app_state,
        language,
        recipient_id,
        event,
        &format!("{event}_title"),
        &format!("{event}_body"),
        params,
        Some("absences"),
        Some(absence_id),
    )
    .await;
}

async fn notify_absence_inapp_only(
    app_state: &AppState,
    language: &i18n::Language,
    recipient_id: i64,
    event: &str,
    params: Vec<(&'static str, String)>,
    absence_id: i64,
) {
    crate::services::notifications::create_translated_inapp_only(
        app_state,
        language,
        recipient_id,
        event,
        &format!("{event}_title"),
        &format!("{event}_body"),
        params,
        Some("absences"),
        Some(absence_id),
    )
    .await;
}

pub async fn notify_approvers(
    app_state: &AppState,
    language: &i18n::Language,
    recipient_ids: &[i64],
    event: &str,
    params: Vec<(&'static str, String)>,
    absence_id: i64,
) {
    for &id in recipient_ids {
        notify_absence(app_state, language, id, event, params.clone(), absence_id).await;
    }
}

pub fn absence_period_params(
    language: &i18n::Language,
    requester: &User,
    absence: &Absence,
) -> Vec<(&'static str, String)> {
    vec![
        ("requester_name", requester.full_name()),
        (
            "kind",
            i18n::absence_kind_label(language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(language, absence.start_date),
        ),
        ("end_date", i18n::format_date(language, absence.end_date)),
    ]
}

pub fn repo_absence_to_service(a: crate::repository::Absence) -> Absence {
    Absence {
        id: a.id,
        user_id: a.user_id,
        category_id: a.category_id,
        kind: a.kind,
        category_name: a.category_name,
        category_color: a.category_color,
        cost_type: a.cost_type,
        auto_approve_past: a.auto_approve_past,
        start_date: a.start_date,
        end_date: a.end_date,
        comment: a.comment,
        status: a.status,
        reviewed_by: a.reviewed_by,
        reviewed_at: a.reviewed_at,
        rejection_reason: a.rejection_reason,
        created_at: a.created_at,
        review_type: None,
        previous_kind: None,
        previous_category_name: None,
        previous_start_date: None,
        previous_end_date: None,
        previous_comment: None,
    }
}

fn json_opt_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

fn json_opt_date(value: &serde_json::Value, key: &str) -> Option<NaiveDate> {
    let date_str = value.get(key)?.as_str()?;
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

pub fn enrich_absence_with_metadata(
    absence: &mut Absence,
    before_data_map: &std::collections::HashMap<i64, String>,
) {
    if absence.status == "cancellation_pending" {
        absence.review_type = Some("cancellation".to_string());
        return;
    }
    absence.review_type = Some("approval".to_string());
    let Some(before_data) = before_data_map.get(&absence.id) else {
        return;
    };
    let Ok(before_json) = serde_json::from_str::<serde_json::Value>(before_data) else {
        return;
    };
    absence.review_type = Some("change".to_string());
    absence.previous_kind = json_opt_string(&before_json, "kind");
    // `category_name` is present on audit-log payloads written after the
    // configurable-categories rollout (the Absence struct serializes it
    // alongside `kind`). Older rows pre-rollout won't have it and the
    // frontend's `absenceKindLabel` will fall through to translating the
    // raw slug — acceptable for legacy data.
    absence.previous_category_name = json_opt_string(&before_json, "category_name");
    absence.previous_start_date = json_opt_date(&before_json, "start_date");
    absence.previous_end_date = json_opt_date(&before_json, "end_date");
    absence.previous_comment = json_opt_string(&before_json, "comment");
}

pub async fn latest_update_before_data_batch(
    app_state: &AppState,
    ids: &[i64],
) -> AppResult<std::collections::HashMap<i64, String>> {
    crate::repository::AbsenceDb::latest_update_before_data_batch(&app_state.pool, ids).await
}

pub async fn absence_owner_id(pool: &crate::db::DatabasePool, absence_id: i64) -> AppResult<i64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone()).get_user_id(absence_id).await
}

#[derive(Serialize, Clone)]
pub struct Absence {
    pub id: i64,
    pub user_id: i64,
    pub category_id: i64,
    /// Slug of the absence category, kept for backward-compatible API consumers
    /// and i18n key lookup (e.g. `absence_kind_vacation`). The canonical
    /// reference is `category_id`.
    pub kind: String,
    /// Display name and color from the joined category row; present even when the
    /// category is inactive so the UI can render the absence correctly on edit.
    pub category_name: String,
    pub category_color: String,
    /// Joined from the category row: `'none'` | `'vacation'` | `'flextime'`.
    /// Replaces the pre-019 boolean pair.
    pub cost_type: String,
    pub auto_approve_past: bool,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
    pub status: String,
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub review_type: Option<String>,
    pub previous_kind: Option<String>,
    /// Stored display name for `previous_kind`. Pulled from the audit-log
    /// `before_data` JSON alongside `previous_kind` so the review dialog can
    /// localize a "Type changed from X to Y" diff even when the previous
    /// category has since been deactivated (and thus dropped from the
    /// active-only frontend store cache).
    pub previous_category_name: Option<String>,
    pub previous_start_date: Option<NaiveDate>,
    pub previous_end_date: Option<NaiveDate>,
    pub previous_comment: Option<String>,
}

impl Absence {
    /// True when the absence's category has `cost_type='vacation'` (deducts
    /// from the annual leave balance).
    pub fn is_vacation_cost(&self) -> bool {
        self.cost_type == crate::repository::absence_categories::COST_TYPE_VACATION
    }
    /// True when the absence's category has `cost_type='flextime'` (debits the
    /// flextime balance via the kept work target).
    pub fn is_flextime_cost(&self) -> bool {
        self.cost_type == crate::repository::absence_categories::COST_TYPE_FLEXTIME
    }
}

/// Validate the incoming request shape (comment length, date order/range).
/// Category-flag-aware checks (vacation balance, sick backdating) are applied
/// later once the category has been resolved.
pub fn validate_new_absence_shape(input: &NewAbsence) -> AppResult<()> {
    if let Some(comment) = &input.comment {
        if comment.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long (max 2000).".into()));
        }
    }
    if input.end_date < input.start_date {
        return Err(AppError::BadRequest(
            "end_date must be >= start_date.".into(),
        ));
    }
    if (input.end_date - input.start_date).num_days() > 365 {
        return Err(AppError::BadRequest(
            "Absence range exceeds one year.".into(),
        ));
    }
    Ok(())
}

/// Look up the requested category by id (preferred) or by slug (back-compat),
/// without checking `active`. Used internally by both create and update paths;
/// callers apply the active-check appropriate to their scenario.
async fn lookup_requested_category(
    app_state: &AppState,
    body: &NewAbsence,
) -> AppResult<crate::repository::AbsenceCategory> {
    let category = if let Some(category_id) = body.category_id {
        app_state
            .db
            .absence_categories
            .find_by_id(category_id)
            .await?
    } else if let Some(slug) = body.kind.as_deref().filter(|s| !s.is_empty()) {
        app_state.db.absence_categories.find_by_slug(slug).await?
    } else {
        return Err(AppError::BadRequest(
            "Absence category required (category_id or kind).".into(),
        ));
    };
    category.ok_or_else(|| AppError::BadRequest("Unknown absence category.".into()))
}

/// Resolve the requested category and reject inactive ones. Used by the
/// create path, where only active categories may accept new requests.
pub async fn resolve_requested_category(
    app_state: &AppState,
    body: &NewAbsence,
    user_id: i64,
) -> AppResult<crate::repository::AbsenceCategory> {
    let category = lookup_requested_category(app_state, body).await?;
    if !category.active {
        return Err(AppError::BadRequest(
            "Absence category is no longer active.".into(),
        ));
    }
    if !app_state
        .db
        .absence_categories
        .is_enabled_for_user(category.id, user_id)
        .await?
    {
        return Err(AppError::BadRequest(
            "Absence category not available for you.".into(),
        ));
    }
    Ok(category)
}

/// Resolve the requested category for an edit. Inactive categories are
/// allowed ONLY when the user is not changing the category (i.e. the same
/// category_id as the existing absence). This preserves the ability to edit
/// other fields (dates, comment) on a requested absence even after an admin
/// has deactivated the category — without letting the user switch INTO an
/// inactive category from outside. The same bypass applies to the per-user
/// enabled check: an employee who keeps their existing category may still
/// edit other fields even if it was later disabled for them.
pub async fn resolve_requested_category_for_edit(
    app_state: &AppState,
    body: &NewAbsence,
    current_category_id: i64,
    user_id: i64,
) -> AppResult<crate::repository::AbsenceCategory> {
    let category = lookup_requested_category(app_state, body).await?;
    if category.id != current_category_id {
        if !category.active {
            return Err(AppError::BadRequest(
                "Absence category is no longer active.".into(),
            ));
        }
        if !app_state
            .db
            .absence_categories
            .is_enabled_for_user(category.id, user_id)
            .await?
        {
            return Err(AppError::BadRequest(
                "Absence category not available for you.".into(),
            ));
        }
    }
    Ok(category)
}

#[derive(Deserialize)]
pub struct NewAbsence {
    /// Preferred reference. Falls back to `kind` (slug) for legacy callers.
    #[serde(default)]
    pub category_id: Option<i64>,
    #[serde(default)]
    pub kind: Option<String>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
}

/// Return `Forbidden` when the requesting user has time tracking disabled.
/// Delegates to the canonical implementation in `services::users`.
pub fn require_tracks_time(user: &User) -> AppResult<()> {
    crate::services::users::require_tracks_time(user)
}

/// Verify that `requester` is allowed to access data for `target_uid`.
/// Delegates to the canonical implementation in `services::users`.
pub async fn assert_can_access_user(
    app_state: &AppState,
    requester: &User,
    target_uid: i64,
) -> AppResult<()> {
    crate::services::users::assert_can_access_user(app_state, requester, target_uid).await
}

pub async fn create_absence(
    app_state: &AppState,
    requester: &User,
    body: NewAbsence,
) -> AppResult<Absence> {
    require_tracks_time(requester)?;
    let today_date = crate::services::settings::app_today(&app_state.pool).await;
    validate_new_absence_shape(&body)?;
    let category = resolve_requested_category(app_state, &body, requester.id).await?;
    validate_backdating_window(&category, body.start_date, today_date)?;
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    validate_absence_has_workday(
        &app_state.pool,
        requester.workdays_per_week,
        body.start_date,
        body.end_date,
    )
    .await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, requester.id).await?;
    crate::repository::AbsenceDb::assert_no_overlap_tx(
        &mut transaction,
        requester.id,
        body.start_date,
        body.end_date,
        None,
    )
    .await?;
    // Time-entry conflict is checked at approval, not at request creation.
    // This lets employees request an absence even if they have forgotten to remove
    // existing time entries; the approver then sees and handles the conflict.
    if category.is_vacation_cost() {
        validate_vacation_balance(
            &app_state.pool,
            &mut transaction,
            requester,
            body.start_date,
            body.end_date,
            None,
            false,
        )
        .await?;
    }
    if category.is_flextime_cost() {
        validate_flextime_balance(
            &app_state.pool,
            &mut transaction,
            requester,
            body.start_date,
            body.end_date,
            None,
        )
        .await?;
    }
    let initial_status = if category.auto_approve_past && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    let new_absence_id = crate::repository::AbsenceDb::insert_tx(
        &mut transaction,
        requester.id,
        category.id,
        body.start_date,
        body.end_date,
        body.comment.as_deref(),
        initial_status,
    )
    .await?;
    transaction.commit().await?;
    let created_absence =
        repo_absence_to_service(app_state.db.absences.find_by_id(new_absence_id).await?);
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "absences",
        new_absence_id,
        None,
        serde_json::to_value(&created_absence).ok(),
    )
    .await;
    if created_absence.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids =
            crate::services::auth::required_approval_recipient_ids(&app_state.pool, requester)
                .await?;
        notify_approvers(
            app_state,
            &language,
            &approver_ids,
            "absence_requested",
            absence_period_params(&language, requester, &created_absence),
            new_absence_id,
        )
        .await;
    } else if created_absence.auto_approve_past && created_absence.status == "approved" {
        notify_sick_auto_approved(app_state, requester, &created_absence, new_absence_id).await;
    }
    Ok(created_absence)
}

pub async fn update_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
    body: NewAbsence,
) -> AppResult<Absence> {
    require_tracks_time(requester)?;
    let today_date = crate::services::settings::app_today(&app_state.pool).await;
    validate_new_absence_shape(&body)?;
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    validate_absence_has_workday(
        &app_state.pool,
        requester.workdays_per_week,
        body.start_date,
        body.end_date,
    )
    .await?;
    let current_owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, current_owner_id).await?;
    let absence_before_update = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    if absence_before_update.user_id != requester.id {
        return Err(AppError::Forbidden);
    }
    if absence_before_update.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be edited.".into(),
        ));
    }
    // Resolve the requested category. An inactive category is allowed ONLY
    // when the user is not changing category (i.e. they're editing dates or
    // comment on a request whose category was deactivated by an admin in the
    // meantime). Switching INTO an inactive category from outside is rejected.
    let category = resolve_requested_category_for_edit(
        app_state,
        &body,
        absence_before_update.category_id,
        requester.id,
    )
    .await?;
    validate_backdating_window(&category, body.start_date, today_date)?;
    // Auto-approve categories (sick-like) have an entirely different workflow:
    // they bypass approval and tolerate same-day time entries. Allowing a
    // requested-status absence to switch INTO or OUT of an auto-approve
    // category mid-edit would either skip required approval or reopen one for
    // a record that the user expected to be confidential. Keep this guard.
    if absence_before_update.auto_approve_past != category.auto_approve_past {
        return Err(AppError::BadRequest(
            "Cannot change between auto-approve (e.g. sick) and review categories.".into(),
        ));
    }
    // Changing the cost type (none ↔ vacation ↔ flextime) would silently
    // alter the financial meaning of the absence without re-triggering
    // approver review. The user must cancel and re-request with the
    // correct category. With cost_type collapsed into one field the check
    // is a single comparison instead of two ORed bool diffs.
    if absence_before_update.cost_type != category.cost_type {
        return Err(AppError::BadRequest(
            "Cannot change absence category cost type (vacation \u{2194} flextime). \
             Cancel and re-request with the new category."
                .into(),
        ));
    }
    crate::repository::AbsenceDb::assert_no_overlap_tx(
        &mut transaction,
        requester.id,
        body.start_date,
        body.end_date,
        Some(absence_id),
    )
    .await?;
    // Time-entry conflict is checked at approval, not at edit time —
    // consistent with create_absence behavior.
    if category.is_vacation_cost() {
        validate_vacation_balance(
            &app_state.pool,
            &mut transaction,
            requester,
            body.start_date,
            body.end_date,
            Some(absence_id),
            false,
        )
        .await?;
    }
    if category.is_flextime_cost() {
        validate_flextime_balance(
            &app_state.pool,
            &mut transaction,
            requester,
            body.start_date,
            body.end_date,
            Some(absence_id),
        )
        .await?;
    }
    let updated_status = if category.auto_approve_past && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    crate::repository::AbsenceDb::update_fields_tx(
        &mut transaction,
        absence_id,
        category.id,
        body.start_date,
        body.end_date,
        body.comment.as_deref(),
        updated_status,
    )
    .await?;
    transaction.commit().await?;
    let absence_after_update =
        repo_absence_to_service(app_state.db.absences.find_by_id(absence_id).await?);
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "absences",
        absence_id,
        serde_json::to_value(&absence_before_update).ok(),
        serde_json::to_value(&absence_after_update).ok(),
    )
    .await;
    if absence_after_update.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids =
            crate::services::auth::required_approval_recipient_ids(&app_state.pool, requester)
                .await?;
        notify_approvers(
            app_state,
            &language,
            &approver_ids,
            "absence_updated",
            absence_period_params(&language, requester, &absence_after_update),
            absence_id,
        )
        .await;
    } else if absence_after_update.auto_approve_past && absence_after_update.status == "approved" {
        notify_sick_auto_approved(app_state, requester, &absence_after_update, absence_id).await;
    }
    Ok(absence_after_update)
}

async fn notify_sick_auto_approved(
    app_state: &AppState,
    requester: &User,
    absence: &Absence,
    absence_id: i64,
) {
    let language = notification_language(&app_state.pool).await;
    let mut approver_ids =
        crate::services::auth::approval_recipient_ids(&app_state.pool, requester).await;
    approver_ids.retain(|id| *id != requester.id);
    notify_approvers(
        app_state,
        &language,
        &approver_ids,
        "absence_auto_approved_notice",
        absence_period_params(&language, requester, absence),
        absence_id,
    )
    .await;
}

pub async fn cancel_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
) -> AppResult<serde_json::Value> {
    require_tracks_time(requester)?;
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    if absence.user_id != requester.id {
        return Err(AppError::Forbidden);
    }
    let language = notification_language(&app_state.pool).await;
    let approver_params = vec![
        ("requester_name", requester.full_name()),
        (
            "kind",
            i18n::absence_kind_label(&language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    match absence.status.as_str() {
        "requested" => {
            crate::repository::AbsenceDb::cancel_requested_tx(&mut transaction, absence_id).await?;
            transaction.commit().await?;
            audit::log(
                &app_state.pool,
                requester.id,
                "cancelled",
                "absences",
                absence_id,
                serde_json::to_value(&absence).ok(),
                Some(serde_json::json!({"status": "cancelled"})),
            )
            .await;
            let approver_ids =
                crate::services::auth::approval_recipient_ids(&app_state.pool, requester).await;
            notify_approvers(
                app_state,
                &language,
                &approver_ids,
                "absence_cancelled",
                approver_params,
                absence_id,
            )
            .await;
            Ok(serde_json::json!({"ok": true}))
        }
        "approved" => {
            let approver_ids =
                crate::services::auth::required_approval_recipient_ids(&app_state.pool, requester)
                    .await?;
            let rows =
                crate::repository::AbsenceDb::request_cancellation_tx(&mut transaction, absence_id)
                    .await?;
            if rows == 0 {
                return Err(AppError::Conflict(
                    "Absence status changed concurrently.".into(),
                ));
            }
            transaction.commit().await?;
            audit::log(
                &app_state.pool,
                requester.id,
                "cancellation_requested",
                "absences",
                absence_id,
                serde_json::to_value(&absence).ok(),
                Some(serde_json::json!({"status": "cancellation_pending"})),
            )
            .await;
            notify_approvers(
                app_state,
                &language,
                &approver_ids,
                "absence_cancellation_requested",
                approver_params,
                absence_id,
            )
            .await;
            Ok(serde_json::json!({"ok": true, "pending": true}))
        }
        _ => Err(AppError::BadRequest(
            "Only requested or approved absences can be cancelled.".into(),
        )),
    }
}

pub async fn approve_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
) -> AppResult<serde_json::Value> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin()
        && !crate::repository::AbsenceDb::is_direct_report_for_update(
            &mut transaction,
            absence.user_id,
            requester.id,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    if absence.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be approved.".into(),
        ));
    }
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(
        &mut transaction,
        absence.user_id,
        absence.auto_approve_past,
        absence.start_date,
        absence.end_date,
    )
    .await?;
    // Re-validate at approval time: the user's balance may have changed since
    // the request was filed, and other balance-affecting absences may have
    // landed in between. Without this, a request that passed validation at
    // creation could be approved into a balance breach.
    if absence.is_vacation_cost() || absence.is_flextime_cost() {
        let repo_user = app_state
            .db
            .users
            .find_by_id(absence.user_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let absence_owner = crate::services::users::repo_user_to_auth_user(repo_user);
        if absence.is_vacation_cost() {
            validate_vacation_balance(
                &app_state.pool,
                &mut transaction,
                &absence_owner,
                absence.start_date,
                absence.end_date,
                Some(absence_id),
                true,
            )
            .await?;
        }
        if absence.is_flextime_cost() {
            validate_flextime_balance(
                &app_state.pool,
                &mut transaction,
                &absence_owner,
                absence.start_date,
                absence.end_date,
                Some(absence_id),
            )
            .await?;
        }
    }
    let rows_updated =
        crate::repository::AbsenceDb::approve_tx(&mut transaction, absence_id, requester.id)
            .await?;
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    // Drop the pending entry from every other approver's queue (and clear
    // the requester's own dashboard chip). The history row stays in the
    // notifications table for audit purposes — only `is_read` flips.
    crate::services::notifications::clear_pending_for_reference(
        app_state,
        "absences",
        absence_id,
    )
    .await;
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "absences",
        absence_id,
        serde_json::to_value(&absence).ok(),
        Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        (
            "kind",
            i18n::absence_kind_label(&language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(
            app_state,
            &language,
            absence.user_id,
            "absence_approved",
            notify_params,
            absence_id,
        )
        .await;
    } else {
        notify_absence_inapp_only(
            app_state,
            &language,
            absence.user_id,
            "absence_approved",
            notify_params,
            absence_id,
        )
        .await;
    }
    Ok(serde_json::json!({"ok":true}))
}

pub async fn reject_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
    reason: &str,
) -> AppResult<serde_json::Value> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long (max 2000).".into()));
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin()
        && !crate::repository::AbsenceDb::is_direct_report_for_update(
            &mut transaction,
            absence.user_id,
            requester.id,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    if absence.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be rejected.".into(),
        ));
    }
    let rows_updated =
        crate::repository::AbsenceDb::reject_tx(&mut transaction, absence_id, requester.id, reason)
            .await?;
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    // See approve_absence: clear the pending entry from every approver's
    // queue once the decision has been recorded.
    crate::services::notifications::clear_pending_for_reference(
        app_state,
        "absences",
        absence_id,
    )
    .await;
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "absences",
        absence_id,
        serde_json::to_value(&absence).ok(),
        Some(serde_json::json!({"status": "rejected", "reason": reason})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        (
            "kind",
            i18n::absence_kind_label(&language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
        ("reason", reason.to_string()),
    ];
    if absence.user_id != requester.id {
        notify_absence(
            app_state,
            &language,
            absence.user_id,
            "absence_rejected",
            notify_params,
            absence_id,
        )
        .await;
    } else {
        notify_absence_inapp_only(
            app_state,
            &language,
            absence.user_id,
            "absence_rejected",
            notify_params,
            absence_id,
        )
        .await;
    }
    Ok(serde_json::json!({"ok":true}))
}

pub async fn approve_cancellation_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
) -> AppResult<serde_json::Value> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence =
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?;
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin()
        && !crate::repository::AbsenceDb::is_direct_report_for_update(
            &mut transaction,
            absence.user_id,
            requester.id,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    if absence.status != "cancellation_pending" {
        return Err(AppError::BadRequest(
            "Only cancellation-pending absences can have their cancellation approved.".into(),
        ));
    }
    let rows = crate::repository::AbsenceDb::approve_cancellation_tx(
        &mut transaction,
        absence_id,
        requester.id,
    )
    .await?;
    if rows == 0 {
        return Err(AppError::Conflict(
            "Absence status changed concurrently.".into(),
        ));
    }
    transaction.commit().await?;
    // Cancellation decided — drop the pending entry from every approver's queue.
    crate::services::notifications::clear_pending_for_reference(
        app_state,
        "absences",
        absence_id,
    )
    .await;
    audit::log(
        &app_state.pool,
        requester.id,
        "cancelled",
        "absences",
        absence_id,
        serde_json::to_value(&absence).ok(),
        Some(serde_json::json!({"status": "cancelled", "reviewed_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        (
            "kind",
            i18n::absence_kind_label(&language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(
            app_state,
            &language,
            absence.user_id,
            "absence_cancellation_approved",
            notify_params,
            absence_id,
        )
        .await;
    } else {
        notify_absence_inapp_only(
            app_state,
            &language,
            absence.user_id,
            "absence_cancellation_approved",
            notify_params,
            absence_id,
        )
        .await;
    }
    Ok(serde_json::json!({"ok": true}))
}

pub async fn reject_cancellation_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
) -> AppResult<serde_json::Value> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence =
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?;
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin()
        && !crate::repository::AbsenceDb::is_direct_report_for_update(
            &mut transaction,
            absence.user_id,
            requester.id,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    if absence.status != "cancellation_pending" {
        return Err(AppError::BadRequest(
            "Only cancellation-pending absences can have their cancellation rejected.".into(),
        ));
    }
    let rows = crate::repository::AbsenceDb::reject_cancellation_tx(
        &mut transaction,
        absence_id,
        requester.id,
    )
    .await?;
    if rows == 0 {
        return Err(AppError::Conflict(
            "Absence status changed concurrently.".into(),
        ));
    }
    transaction.commit().await?;
    // Cancellation decided — drop the pending entry from every approver's queue.
    crate::services::notifications::clear_pending_for_reference(
        app_state,
        "absences",
        absence_id,
    )
    .await;
    audit::log(
        &app_state.pool,
        requester.id,
        "cancellation_rejected",
        "absences",
        absence_id,
        serde_json::to_value(&absence).ok(),
        Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        (
            "kind",
            i18n::absence_kind_label(&language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(
            app_state,
            &language,
            absence.user_id,
            "absence_cancellation_rejected",
            notify_params,
            absence_id,
        )
        .await;
    } else {
        notify_absence_inapp_only(
            app_state,
            &language,
            absence.user_id,
            "absence_cancellation_rejected",
            notify_params,
            absence_id,
        )
        .await;
    }
    Ok(serde_json::json!({"ok": true}))
}

pub async fn revoke_absence(
    app_state: &AppState,
    requester: &User,
    absence_id: i64,
) -> AppResult<serde_json::Value> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    if absence.status != "approved" {
        return Err(AppError::BadRequest(
            "Only approved absences can be revoked.".into(),
        ));
    }
    crate::repository::AbsenceDb::revoke_tx(&mut transaction, absence_id, requester.id).await?;
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "revoked",
        "absences",
        absence_id,
        serde_json::to_value(&absence).ok(),
        Some(serde_json::json!({"status": "cancelled", "revoked_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        (
            "kind",
            i18n::absence_kind_label(&language, &absence.kind, &absence.category_name),
        ),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(
            app_state,
            &language,
            absence.user_id,
            "absence_revoked",
            notify_params,
            absence_id,
        )
        .await;
    } else {
        notify_absence_inapp_only(
            app_state,
            &language,
            absence.user_id,
            "absence_revoked",
            notify_params,
            absence_id,
        )
        .await;
    }
    Ok(serde_json::json!({"ok":true}))
}

fn serialize_day_count<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if (*value - value.round()).abs() < 1e-9 {
        serializer.serialize_i64(value.round() as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

#[derive(Serialize)]
pub struct LeaveBalance {
    pub annual_entitlement: i64,
    #[serde(serialize_with = "serialize_day_count")]
    pub already_taken: f64,
    #[serde(serialize_with = "serialize_day_count")]
    pub approved_upcoming: f64,
    #[serde(serialize_with = "serialize_day_count")]
    pub requested: f64,
    #[serde(serialize_with = "serialize_day_count")]
    pub available: f64,
    pub carryover_days: i64,
    #[serde(serialize_with = "serialize_day_count")]
    pub carryover_remaining: f64,
    pub carryover_expiry: Option<String>,
    pub carryover_expired: bool,
}

pub async fn compute_balance(
    app_state: &AppState,
    requester: &User,
    target_user_id: i64,
    year: i32,
) -> AppResult<LeaveBalance> {
    use crate::services::absence_balance::{
        carryover_remaining_days, parse_expiry_date, total_entitlement_with_carryover,
        vacation_year_context, workdays_for_ranges_in_window, CarryoverRemainingInput,
    };

    assert_can_access_user(app_state, requester, target_user_id).await?;
    let repo_user = app_state
        .db
        .users
        .find_by_id(target_user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let target_user = crate::services::users::repo_user_to_auth_user(repo_user);
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| AppError::BadRequest("Invalid year.".into()))?;
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31)
        .ok_or_else(|| AppError::BadRequest("Invalid year.".into()))?;
    let today = crate::services::settings::app_today(&app_state.pool).await;
    let vacation_absences: Vec<Absence> = app_state
        .db
        .absences
        .vacation_absences_in_year(target_user_id, year_from, year_to)
        .await?
        .into_iter()
        .map(repo_absence_to_service)
        .collect();
    let mut taken_days = 0.0;
    let mut upcoming_days = 0.0;
    let mut requested_days = 0.0;
    for absence in &vacation_absences {
        let clamped_start = std::cmp::max(absence.start_date, year_from);
        let clamped_end = std::cmp::min(absence.end_date, year_to);
        if absence.status == "approved" {
            if clamped_end <= today {
                taken_days +=
                    workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
            } else if clamped_start > today {
                upcoming_days +=
                    workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
            } else {
                taken_days +=
                    workdays(&app_state.pool, target_user.id, clamped_start, today).await?;
                let tomorrow = today + Duration::days(1);
                if tomorrow <= clamped_end {
                    upcoming_days +=
                        workdays(&app_state.pool, target_user.id, tomorrow, clamped_end).await?;
                }
            }
        } else if absence.status == "requested" || absence.status == "cancellation_pending" {
            requested_days +=
                workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
        }
    }
    let expiry_setting =
        crate::services::settings::load_setting(&app_state.pool, "carryover_expiry_date", "03-31")
            .await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let (effective_entitlement, carryover_days, carryover_expired) =
        vacation_year_context(&app_state.pool, &target_user, year, today, &expiry_setting).await?;
    let carryover_remaining = carryover_remaining_days(CarryoverRemainingInput {
        pool: &app_state.pool,
        user_id: target_user.id,
        vacation_absences: &vacation_absences,
        year_start: year_from,
        today,
        expiry_date,
        carryover_days,
        carryover_expired,
    })
    .await?;
    let total_entitlement =
        total_entitlement_with_carryover(effective_entitlement, carryover_days, carryover_expired);
    let available = if carryover_expired {
        if let Some(expiry) = expiry_date {
            let reserved_ranges: Vec<(NaiveDate, NaiveDate)> = vacation_absences
                .iter()
                .map(|a| (a.start_date, a.end_date))
                .collect();
            let pre_window_end = std::cmp::min(expiry, year_to);
            let post_window_start = expiry + Duration::days(1);
            let pre_reserved = if year_from <= pre_window_end {
                workdays_for_ranges_in_window(
                    &app_state.pool,
                    target_user.id,
                    &reserved_ranges,
                    year_from,
                    pre_window_end,
                )
                .await?
            } else {
                0.0
            };
            let post_reserved = if post_window_start <= year_to {
                workdays_for_ranges_in_window(
                    &app_state.pool,
                    target_user.id,
                    &reserved_ranges,
                    post_window_start,
                    year_to,
                )
                .await?
            } else {
                0.0
            };
            let base_consumed_before_or_on_expiry = (pre_reserved - carryover_days as f64).max(0.0);
            effective_entitlement as f64 - base_consumed_before_or_on_expiry - post_reserved
        } else {
            total_entitlement - taken_days - upcoming_days - requested_days
        }
    } else {
        total_entitlement - taken_days - upcoming_days - requested_days
    };
    Ok(LeaveBalance {
        annual_entitlement: effective_entitlement,
        already_taken: taken_days,
        approved_upcoming: upcoming_days,
        requested: requested_days,
        available,
        carryover_days,
        carryover_remaining,
        carryover_expiry: expiry_date.map(|d| d.to_string()),
        carryover_expired,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // ──────────────────────────────────────────────────────────────────────
    // Helpers
    // ──────────────────────────────────────────────────────────────────────

    fn sample_user(tracks_time: bool) -> User {
        User {
            id: 1,
            email: "user@example.com".to_string(),
            password_hash: "hash".to_string(),
            first_name: "Alice".to_string(),
            last_name: "Smith".to_string(),
            role: "employee".to_string(),
            weekly_hours: 40.0,
            workdays_per_week: 5,
            start_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            hire_date: None,
            active: true,
            must_change_password: false,
            created_at: Utc::now(),
            allow_reopen_without_approval: false,
            allow_submission_without_approval: false,
            dark_mode: false,
            overtime_start_balance_min: 0,
            tracks_time,
            annual_leave_days: 30,
            archived_at: None,
        }
    }

    fn sample_absence(id: i64, status: &str, kind: &str) -> Absence {
        let (cost_type, auto_approve_past) = match kind {
            "vacation" => ("vacation", false),
            "sick" => ("none", true),
            "flextime_reduction" => ("flextime", false),
            _ => ("none", false),
        };
        Absence {
            id,
            user_id: 1,
            category_id: 1,
            kind: kind.to_string(),
            category_name: kind.to_string(),
            category_color: "#000000".to_string(),
            cost_type: cost_type.to_string(),
            auto_approve_past,
            start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            comment: None,
            status: status.to_string(),
            reviewed_by: None,
            reviewed_at: None,
            rejection_reason: None,
            created_at: Utc::now(),
            review_type: None,
            previous_kind: None,
            previous_category_name: None,
            previous_start_date: None,
            previous_end_date: None,
            previous_comment: None,
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // validate_absence
    // ──────────────────────────────────────────────────────────────────────

    /// A well-formed payload must pass the shape validation.
    #[test]
    fn validate_new_absence_shape_accepts_valid_input() {
        let input = NewAbsence {
            category_id: Some(1),
            kind: None,
            start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            comment: None,
        };
        assert!(validate_new_absence_shape(&input).is_ok());
    }

    /// A comment exceeding 2000 characters must be rejected.
    #[test]
    fn validate_new_absence_shape_rejects_oversized_comment() {
        let input = NewAbsence {
            category_id: Some(1),
            kind: None,
            start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            comment: Some("x".repeat(2001)),
        };
        assert!(matches!(
            validate_new_absence_shape(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    /// end_date < start_date must be rejected.
    #[test]
    fn validate_new_absence_shape_rejects_inverted_date_range() {
        let input = NewAbsence {
            category_id: Some(1),
            kind: None,
            start_date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            comment: None,
        };
        assert!(matches!(
            validate_new_absence_shape(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    /// A range spanning more than 365 days must be rejected.
    #[test]
    fn validate_new_absence_shape_rejects_range_exceeding_one_year() {
        let input = NewAbsence {
            category_id: Some(1),
            kind: None,
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2027, 1, 3).unwrap(), // 367 days
            comment: None,
        };
        assert!(matches!(
            validate_new_absence_shape(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    // ──────────────────────────────────────────────────────────────────────
    // require_tracks_time
    // ──────────────────────────────────────────────────────────────────────

    /// A user with tracks_time = true must not be blocked.
    #[test]
    fn require_tracks_time_allows_tracking_user() {
        let user = sample_user(true);
        assert!(require_tracks_time(&user).is_ok());
    }

    /// A user with tracks_time = false (pure-admin mode) must be blocked
    /// with Forbidden.
    #[test]
    fn require_tracks_time_blocks_non_tracking_user() {
        let user = sample_user(false);
        assert!(matches!(
            require_tracks_time(&user).unwrap_err(),
            AppError::Forbidden
        ));
    }

    // ──────────────────────────────────────────────────────────────────────
    // enrich_absence_with_metadata
    // ──────────────────────────────────────────────────────────────────────

    /// A `cancellation_pending` absence must get review_type = "cancellation"
    /// and the function must return early without reading the before_data map.
    #[test]
    fn enrich_absence_sets_cancellation_type_for_pending_cancellations() {
        let mut absence = sample_absence(1, "cancellation_pending", "vacation");
        let map = std::collections::HashMap::new();
        enrich_absence_with_metadata(&mut absence, &map);
        assert_eq!(absence.review_type.as_deref(), Some("cancellation"));
        assert!(absence.previous_kind.is_none());
    }

    /// An absence without a matching entry in before_data_map must get
    /// review_type = "approval" (initial submission, nothing to diff).
    #[test]
    fn enrich_absence_sets_approval_type_when_no_before_data() {
        let mut absence = sample_absence(42, "requested", "vacation");
        let map = std::collections::HashMap::new(); // absence id 42 not in map
        enrich_absence_with_metadata(&mut absence, &map);
        assert_eq!(absence.review_type.as_deref(), Some("approval"));
        assert!(absence.previous_kind.is_none());
    }

    /// An absence with valid before_data JSON must get review_type = "change"
    /// and have the previous fields populated from the JSON.
    #[test]
    fn enrich_absence_sets_change_type_with_previous_data_when_before_data_present() {
        let mut absence = sample_absence(5, "requested", "vacation");
        let mut map = std::collections::HashMap::new();
        map.insert(
            5i64,
            r#"{"kind":"sick","start_date":"2026-05-01","end_date":"2026-05-03","comment":"was sick"}"#
                .to_string(),
        );
        enrich_absence_with_metadata(&mut absence, &map);
        assert_eq!(absence.review_type.as_deref(), Some("change"));
        assert_eq!(absence.previous_kind.as_deref(), Some("sick"));
        assert_eq!(
            absence.previous_start_date,
            NaiveDate::from_ymd_opt(2026, 5, 1)
        );
        assert_eq!(
            absence.previous_end_date,
            NaiveDate::from_ymd_opt(2026, 5, 3)
        );
        assert_eq!(absence.previous_comment.as_deref(), Some("was sick"));
    }

    /// Invalid JSON in before_data must leave the absence as review_type =
    /// "approval" (graceful degradation, not a hard error).
    #[test]
    fn enrich_absence_falls_back_to_approval_on_invalid_before_json() {
        let mut absence = sample_absence(7, "requested", "sick");
        let mut map = std::collections::HashMap::new();
        map.insert(7i64, "not-valid-json".to_string());
        enrich_absence_with_metadata(&mut absence, &map);
        // The function sets "approval" before trying to parse JSON, and the
        // parse failure causes an early return without overwriting to "change".
        assert_eq!(absence.review_type.as_deref(), Some("approval"));
        assert!(absence.previous_kind.is_none());
    }

    // ──────────────────────────────────────────────────────────────────────
    // absence_period_params
    // ──────────────────────────────────────────────────────────────────────

    /// The helper must produce four parameters with non-empty values
    /// for requester_name, kind, start_date, and end_date.
    #[test]
    fn absence_period_params_includes_all_required_keys() {
        use crate::i18n::Language;
        let language = Language::from_setting("en");
        let user = sample_user(true);
        let absence = sample_absence(1, "requested", "vacation");
        let params = absence_period_params(&language, &user, &absence);

        let keys: Vec<&str> = params.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"requester_name"));
        assert!(keys.contains(&"kind"));
        assert!(keys.contains(&"start_date"));
        assert!(keys.contains(&"end_date"));

        let by_key = |key: &str| {
            params
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        assert_eq!(by_key("requester_name"), "Alice Smith");
        // "vacation" localises to "Vacation" in English.
        assert_eq!(by_key("kind"), "Vacation");
    }

    // ──────────────────────────────────────────────────────────────────────
    // repo_absence_to_service
    // ──────────────────────────────────────────────────────────────────────

    /// The mapper must copy all repository fields and zero-initialise the
    /// service-only review metadata fields.
    #[test]
    fn repo_absence_to_service_maps_fields_and_clears_review_metadata() {
        let repo = crate::repository::Absence {
            id: 99,
            user_id: 7,
            category_id: 2,
            kind: "sick".to_string(),
            category_name: "Sick".to_string(),
            category_color: "#ef4444".to_string(),
            cost_type: "none".to_string(),
            auto_approve_past: true,
            start_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(),
            comment: Some("flu".to_string()),
            status: "approved".to_string(),
            reviewed_by: Some(2),
            reviewed_at: Some(Utc::now()),
            rejection_reason: None,
            created_at: Utc::now(),
        };
        let svc = repo_absence_to_service(repo.clone());
        assert_eq!(svc.id, 99);
        assert_eq!(svc.user_id, 7);
        assert_eq!(svc.category_id, 2);
        assert_eq!(svc.kind, "sick");
        assert!(svc.auto_approve_past);
        assert_eq!(svc.status, "approved");
        assert_eq!(svc.comment.as_deref(), Some("flu"));
        assert_eq!(svc.reviewed_by, Some(2));
        // Service-specific fields must default to None.
        assert!(svc.review_type.is_none());
        assert!(svc.previous_kind.is_none());
        assert!(svc.previous_start_date.is_none());
        assert!(svc.previous_end_date.is_none());
        assert!(svc.previous_comment.is_none());
    }

    // ──────────────────────────────────────────────────────────────────────
    // serialize_day_count (tested through LeaveBalance serialisation)
    // ──────────────────────────────────────────────────────────────────────

    /// Whole-number values must serialise as integers, fractional values as
    /// floats — this ensures the JSON surface stays clean for the frontend.
    #[test]
    fn serialize_day_count_emits_integer_for_whole_numbers_and_float_for_fractions() {
        let balance = LeaveBalance {
            annual_entitlement: 25,
            already_taken: 3.0,
            approved_upcoming: 2.5,
            requested: 0.0,
            available: 19.5,
            carryover_days: 0,
            carryover_remaining: 0.0,
            carryover_expiry: None,
            carryover_expired: false,
        };
        let json = serde_json::to_value(&balance).unwrap();
        // Whole numbers must serialise as JSON integers, not floats.
        assert_eq!(json["already_taken"], serde_json::json!(3));
        assert_eq!(json["requested"], serde_json::json!(0));
        // Fractional values must serialise as JSON floats.
        assert_eq!(json["approved_upcoming"], serde_json::json!(2.5));
        assert_eq!(json["available"], serde_json::json!(19.5));
    }
}
