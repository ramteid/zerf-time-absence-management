use crate::audit;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::services::absence_balance::{
    validate_absence_has_workday, validate_sick_start_date, validate_vacation_balance,
    workdays,
};
use crate::AppState;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize, Serializer};

use crate::repository::absences::ALLOWED_KINDS as ALLOWED_ABSENCE_KINDS;

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
        ("kind", i18n::absence_kind_label(language, &absence.kind)),
        ("start_date", i18n::format_date(language, absence.start_date)),
        ("end_date", i18n::format_date(language, absence.end_date)),
    ]
}

pub fn repo_absence_to_service(a: crate::repository::Absence) -> Absence {
    Absence {
        id: a.id,
        user_id: a.user_id,
        kind: a.kind,
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
    pub kind: String,
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
    pub previous_start_date: Option<NaiveDate>,
    pub previous_end_date: Option<NaiveDate>,
    pub previous_comment: Option<String>,
}

pub fn validate_absence(input: &NewAbsence) -> AppResult<&str> {
    if !ALLOWED_ABSENCE_KINDS.contains(&input.kind.as_str()) {
        return Err(AppError::BadRequest("Invalid kind".into()));
    }
    if let Some(comment) = &input.comment {
        if comment.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long (max 2000).".into()));
        }
    }
    if input.end_date < input.start_date {
        return Err(AppError::BadRequest("end_date must be >= start_date.".into()));
    }
    if (input.end_date - input.start_date).num_days() > 365 {
        return Err(AppError::BadRequest("Absence range exceeds one year.".into()));
    }
    Ok(&input.kind)
}

#[derive(Deserialize)]
pub struct NewAbsence {
    pub kind: String,
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
    let kind = validate_absence(&body)?;
    validate_sick_start_date(kind, body.start_date, today_date)?;
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest("Absence start date is before user start date.".into()));
    }
    validate_absence_has_workday(&app_state.pool, requester.workdays_per_week, body.start_date, body.end_date).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, requester.id).await?;
    crate::repository::AbsenceDb::assert_no_overlap_tx(&mut transaction, requester.id, body.start_date, body.end_date, None).await?;
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(&mut transaction, requester.id, kind, body.start_date, body.end_date).await?;
    if kind == "vacation" {
        validate_vacation_balance(&app_state.pool, &mut transaction, requester, body.start_date, body.end_date, None, false).await?;
    }
    let initial_status = if kind == "sick" && body.start_date <= today_date { "approved" } else { "requested" };
    let new_absence_id = crate::repository::AbsenceDb::insert_tx(&mut transaction, requester.id, kind, body.start_date, body.end_date, body.comment.as_deref(), initial_status).await?;
    transaction.commit().await?;
    let created_absence = repo_absence_to_service(app_state.db.absences.find_by_id(new_absence_id).await?);
    audit::log(&app_state.pool, requester.id, "created", "absences", new_absence_id, None, serde_json::to_value(&created_absence).ok()).await;
    if created_absence.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids = crate::services::auth::required_approval_recipient_ids(&app_state.pool, requester).await?;
        notify_approvers(app_state, &language, &approver_ids, "absence_requested", absence_period_params(&language, requester, &created_absence), new_absence_id).await;
    } else if created_absence.kind == "sick" && created_absence.status == "approved" {
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
    let kind = validate_absence(&body)?;
    validate_sick_start_date(kind, body.start_date, today_date)?;
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest("Absence start date is before user start date.".into()));
    }
    validate_absence_has_workday(&app_state.pool, requester.workdays_per_week, body.start_date, body.end_date).await?;
    let current_owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, current_owner_id).await?;
    let absence_before_update = repo_absence_to_service(crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?);
    if absence_before_update.user_id != requester.id { return Err(AppError::Forbidden); }
    if absence_before_update.status != "requested" { return Err(AppError::BadRequest("Only requested absences can be edited.".into())); }
    if absence_before_update.kind == "sick" && body.kind != "sick" { return Err(AppError::BadRequest("Sick absences cannot change type.".into())); }
    if absence_before_update.kind != "sick" && body.kind == "sick" { return Err(AppError::BadRequest("Create a separate sick leave request instead of converting another absence type.".into())); }
    crate::repository::AbsenceDb::assert_no_overlap_tx(&mut transaction, requester.id, body.start_date, body.end_date, Some(absence_id)).await?;
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(&mut transaction, requester.id, kind, body.start_date, body.end_date).await?;
    if kind == "vacation" {
        validate_vacation_balance(&app_state.pool, &mut transaction, requester, body.start_date, body.end_date, Some(absence_id), false).await?;
    }
    let updated_status = if kind == "sick" && body.start_date <= today_date { "approved" } else { "requested" };
    crate::repository::AbsenceDb::update_fields_tx(&mut transaction, absence_id, kind, body.start_date, body.end_date, body.comment.as_deref(), updated_status).await?;
    transaction.commit().await?;
    let absence_after_update = repo_absence_to_service(app_state.db.absences.find_by_id(absence_id).await?);
    audit::log(&app_state.pool, requester.id, "updated", "absences", absence_id, serde_json::to_value(&absence_before_update).ok(), serde_json::to_value(&absence_after_update).ok()).await;
    if absence_after_update.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids = crate::services::auth::required_approval_recipient_ids(&app_state.pool, requester).await?;
        notify_approvers(app_state, &language, &approver_ids, "absence_updated", absence_period_params(&language, requester, &absence_after_update), absence_id).await;
    } else if absence_after_update.kind == "sick" && absence_after_update.status == "approved" {
        notify_sick_auto_approved(app_state, requester, &absence_after_update, absence_id).await;
    }
    Ok(absence_after_update)
}

async fn notify_sick_auto_approved(app_state: &AppState, requester: &User, absence: &Absence, absence_id: i64) {
    let language = notification_language(&app_state.pool).await;
    let mut approver_ids = crate::services::auth::approval_recipient_ids(&app_state.pool, requester).await;
    approver_ids.retain(|id| *id != requester.id);
    notify_approvers(app_state, &language, &approver_ids, "absence_auto_approved_notice", absence_period_params(&language, requester, absence), absence_id).await;
}

pub async fn cancel_absence(app_state: &AppState, requester: &User, absence_id: i64) -> AppResult<serde_json::Value> {
    require_tracks_time(requester)?;
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?);
    if absence.user_id != requester.id { return Err(AppError::Forbidden); }
    let language = notification_language(&app_state.pool).await;
    let approver_params = vec![
        ("requester_name", requester.full_name()),
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    match absence.status.as_str() {
        "requested" => {
            crate::repository::AbsenceDb::cancel_requested_tx(&mut transaction, absence_id).await?;
            transaction.commit().await?;
            audit::log(&app_state.pool, requester.id, "cancelled", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "cancelled"}))).await;
            let approver_ids = crate::services::auth::approval_recipient_ids(&app_state.pool, requester).await;
            notify_approvers(app_state, &language, &approver_ids, "absence_cancelled", approver_params, absence_id).await;
            Ok(serde_json::json!({"ok": true}))
        }
        "approved" => {
            let approver_ids = crate::services::auth::required_approval_recipient_ids(&app_state.pool, requester).await?;
            let rows = crate::repository::AbsenceDb::request_cancellation_tx(&mut transaction, absence_id).await?;
            if rows == 0 { return Err(AppError::Conflict("Absence status changed concurrently.".into())); }
            transaction.commit().await?;
            audit::log(&app_state.pool, requester.id, "cancellation_requested", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "cancellation_pending"}))).await;
            notify_approvers(app_state, &language, &approver_ids, "absence_cancellation_requested", approver_params, absence_id).await;
            Ok(serde_json::json!({"ok": true, "pending": true}))
        }
        _ => Err(AppError::BadRequest("Only requested or approved absences can be cancelled.".into())),
    }
}

pub async fn approve_absence(app_state: &AppState, requester: &User, absence_id: i64) -> AppResult<serde_json::Value> {
    if !requester.is_lead() { return Err(AppError::Forbidden); }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?);
    if absence.user_id == requester.id && !requester.is_admin() { return Err(AppError::Forbidden); }
    if !requester.is_admin() && !crate::repository::AbsenceDb::is_direct_report_for_update(&mut transaction, absence.user_id, requester.id).await? { return Err(AppError::Forbidden); }
    if absence.status != "requested" { return Err(AppError::BadRequest("Only requested absences can be approved.".into())); }
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(&mut transaction, absence.user_id, &absence.kind, absence.start_date, absence.end_date).await?;
    if absence.kind == "vacation" {
        let repo_user = app_state.db.users.find_by_id(absence.user_id).await?.ok_or(AppError::NotFound)?;
        let absence_owner = crate::services::users::repo_user_to_auth_user(repo_user);
        validate_vacation_balance(&app_state.pool, &mut transaction, &absence_owner, absence.start_date, absence.end_date, Some(absence_id), true).await?;
    }
    let rows_updated = crate::repository::AbsenceDb::approve_tx(&mut transaction, absence_id, requester.id).await?;
    if rows_updated == 0 { return Err(AppError::Conflict("Absence was already reviewed by someone else.".into())); }
    transaction.commit().await?;
    audit::log(&app_state.pool, requester.id, "approved", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id}))).await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(app_state, &language, absence.user_id, "absence_approved", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(app_state, &language, absence.user_id, "absence_approved", notify_params, absence_id).await;
    }
    Ok(serde_json::json!({"ok":true}))
}

pub async fn reject_absence(app_state: &AppState, requester: &User, absence_id: i64, reason: &str) -> AppResult<serde_json::Value> {
    if !requester.is_lead() { return Err(AppError::Forbidden); }
    if reason.trim().is_empty() { return Err(AppError::BadRequest("Reason required.".into())); }
    if reason.len() > 2000 { return Err(AppError::BadRequest("Reason too long (max 2000).".into())); }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?);
    if absence.user_id == requester.id && !requester.is_admin() { return Err(AppError::Forbidden); }
    if !requester.is_admin() && !crate::repository::AbsenceDb::is_direct_report_for_update(&mut transaction, absence.user_id, requester.id).await? { return Err(AppError::Forbidden); }
    if absence.status != "requested" { return Err(AppError::BadRequest("Only requested absences can be rejected.".into())); }
    let rows_updated = crate::repository::AbsenceDb::reject_tx(&mut transaction, absence_id, requester.id, reason).await?;
    if rows_updated == 0 { return Err(AppError::Conflict("Absence was already reviewed by someone else.".into())); }
    transaction.commit().await?;
    audit::log(&app_state.pool, requester.id, "rejected", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "rejected", "reason": reason}))).await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
        ("reason", reason.to_string()),
    ];
    if absence.user_id != requester.id {
        notify_absence(app_state, &language, absence.user_id, "absence_rejected", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(app_state, &language, absence.user_id, "absence_rejected", notify_params, absence_id).await;
    }
    Ok(serde_json::json!({"ok":true}))
}

pub async fn approve_cancellation_absence(app_state: &AppState, requester: &User, absence_id: i64) -> AppResult<serde_json::Value> {
    if !requester.is_lead() { return Err(AppError::Forbidden); }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?;
    if absence.user_id == requester.id && !requester.is_admin() { return Err(AppError::Forbidden); }
    if !requester.is_admin() && !crate::repository::AbsenceDb::is_direct_report_for_update(&mut transaction, absence.user_id, requester.id).await? { return Err(AppError::Forbidden); }
    if absence.status != "cancellation_pending" { return Err(AppError::BadRequest("Only cancellation-pending absences can have their cancellation approved.".into())); }
    let rows = crate::repository::AbsenceDb::approve_cancellation_tx(&mut transaction, absence_id, requester.id).await?;
    if rows == 0 { return Err(AppError::Conflict("Absence status changed concurrently.".into())); }
    transaction.commit().await?;
    audit::log(&app_state.pool, requester.id, "cancelled", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "cancelled", "reviewed_by": requester.id}))).await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(app_state, &language, absence.user_id, "absence_cancellation_approved", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(app_state, &language, absence.user_id, "absence_cancellation_approved", notify_params, absence_id).await;
    }
    Ok(serde_json::json!({"ok": true}))
}

pub async fn reject_cancellation_absence(app_state: &AppState, requester: &User, absence_id: i64) -> AppResult<serde_json::Value> {
    if !requester.is_lead() { return Err(AppError::Forbidden); }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.db.absences.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?;
    if absence.user_id == requester.id && !requester.is_admin() { return Err(AppError::Forbidden); }
    if !requester.is_admin() && !crate::repository::AbsenceDb::is_direct_report_for_update(&mut transaction, absence.user_id, requester.id).await? { return Err(AppError::Forbidden); }
    if absence.status != "cancellation_pending" { return Err(AppError::BadRequest("Only cancellation-pending absences can have their cancellation rejected.".into())); }
    let rows = crate::repository::AbsenceDb::reject_cancellation_tx(&mut transaction, absence_id, requester.id).await?;
    if rows == 0 { return Err(AppError::Conflict("Absence status changed concurrently.".into())); }
    transaction.commit().await?;
    audit::log(&app_state.pool, requester.id, "cancellation_rejected", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id}))).await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(app_state, &language, absence.user_id, "absence_cancellation_rejected", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(app_state, &language, absence.user_id, "absence_cancellation_rejected", notify_params, absence_id).await;
    }
    Ok(serde_json::json!({"ok": true}))
}

pub async fn revoke_absence(app_state: &AppState, requester: &User, absence_id: i64) -> AppResult<serde_json::Value> {
    if !requester.is_admin() { return Err(AppError::Forbidden); }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?);
    if absence.status != "approved" { return Err(AppError::BadRequest("Only approved absences can be revoked.".into())); }
    crate::repository::AbsenceDb::revoke_tx(&mut transaction, absence_id, requester.id).await?;
    transaction.commit().await?;
    audit::log(&app_state.pool, requester.id, "revoked", "absences", absence_id, serde_json::to_value(&absence).ok(), Some(serde_json::json!({"status": "cancelled", "revoked_by": requester.id}))).await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(app_state, &language, absence.user_id, "absence_revoked", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(app_state, &language, absence.user_id, "absence_revoked", notify_params, absence_id).await;
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
        CarryoverRemainingInput, carryover_remaining_days, parse_expiry_date,
        total_entitlement_with_carryover, vacation_year_context,
        workdays_for_ranges_in_window,
    };

    assert_can_access_user(app_state, requester, target_user_id).await?;
    let repo_user = app_state.db.users.find_by_id(target_user_id).await?.ok_or(AppError::NotFound)?;
    let target_user = crate::services::users::repo_user_to_auth_user(repo_user);
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(|| AppError::BadRequest("Invalid year.".into()))?;
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31).ok_or_else(|| AppError::BadRequest("Invalid year.".into()))?;
    let today = crate::services::settings::app_today(&app_state.pool).await;
    let vacation_absences: Vec<Absence> = app_state.db.absences.vacation_absences_in_year(target_user_id, year_from, year_to).await?.into_iter().map(repo_absence_to_service).collect();
    let mut taken_days = 0.0;
    let mut upcoming_days = 0.0;
    let mut requested_days = 0.0;
    for absence in &vacation_absences {
        let clamped_start = std::cmp::max(absence.start_date, year_from);
        let clamped_end = std::cmp::min(absence.end_date, year_to);
        if absence.status == "approved" {
            if clamped_end <= today {
                taken_days += workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
            } else if clamped_start > today {
                upcoming_days += workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
            } else {
                taken_days += workdays(&app_state.pool, target_user.id, clamped_start, today).await?;
                let tomorrow = today + Duration::days(1);
                if tomorrow <= clamped_end {
                    upcoming_days += workdays(&app_state.pool, target_user.id, tomorrow, clamped_end).await?;
                }
            }
        } else if absence.status == "requested" || absence.status == "cancellation_pending" {
            requested_days += workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
        }
    }
    let expiry_setting = crate::services::settings::load_setting(&app_state.pool, "carryover_expiry_date", "03-31").await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let (effective_entitlement, carryover_days, carryover_expired) = vacation_year_context(&app_state.pool, &target_user, year, today, &expiry_setting).await?;
    let carryover_remaining = carryover_remaining_days(CarryoverRemainingInput {
        pool: &app_state.pool,
        user_id: target_user.id,
        vacation_absences: &vacation_absences,
        year_start: year_from,
        today,
        expiry_date,
        carryover_days,
        carryover_expired,
    }).await?;
    let total_entitlement = total_entitlement_with_carryover(effective_entitlement, carryover_days, carryover_expired);
    let available = if carryover_expired {
        if let Some(expiry) = expiry_date {
            let reserved_ranges: Vec<(NaiveDate, NaiveDate)> = vacation_absences.iter().map(|a| (a.start_date, a.end_date)).collect();
            let pre_window_end = std::cmp::min(expiry, year_to);
            let post_window_start = expiry + Duration::days(1);
            let pre_reserved = if year_from <= pre_window_end { workdays_for_ranges_in_window(&app_state.pool, target_user.id, &reserved_ranges, year_from, pre_window_end).await? } else { 0.0 };
            let post_reserved = if post_window_start <= year_to { workdays_for_ranges_in_window(&app_state.pool, target_user.id, &reserved_ranges, post_window_start, year_to).await? } else { 0.0 };
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
            dark_mode: false,
            overtime_start_balance_min: 0,
            tracks_time,
        }
    }

    fn sample_absence(id: i64, status: &str, kind: &str) -> Absence {
        Absence {
            id,
            user_id: 1,
            kind: kind.to_string(),
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
            previous_start_date: None,
            previous_end_date: None,
            previous_comment: None,
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // validate_absence
    // ──────────────────────────────────────────────────────────────────────

    /// A well-formed absence must pass validation and return the kind.
    #[test]
    fn validate_absence_accepts_valid_input() {
        let input = NewAbsence {
            kind: "vacation".to_string(),
            start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            comment: None,
        };
        assert_eq!(validate_absence(&input).unwrap(), "vacation");
    }

    /// An unrecognised kind must be rejected.
    #[test]
    fn validate_absence_rejects_unknown_kind() {
        let input = NewAbsence {
            kind: "funday".to_string(),
            start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            comment: None,
        };
        assert!(matches!(
            validate_absence(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    /// A comment exceeding 2000 characters must be rejected.
    #[test]
    fn validate_absence_rejects_oversized_comment() {
        let input = NewAbsence {
            kind: "sick".to_string(),
            start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            comment: Some("x".repeat(2001)),
        };
        assert!(matches!(
            validate_absence(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    /// end_date < start_date must be rejected.
    #[test]
    fn validate_absence_rejects_inverted_date_range() {
        let input = NewAbsence {
            kind: "vacation".to_string(),
            start_date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            comment: None,
        };
        assert!(matches!(
            validate_absence(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    /// A range spanning more than 365 days must be rejected.
    #[test]
    fn validate_absence_rejects_range_exceeding_one_year() {
        let input = NewAbsence {
            kind: "general_absence".to_string(),
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2027, 1, 3).unwrap(), // 367 days
            comment: None,
        };
        assert!(matches!(
            validate_absence(&input).unwrap_err(),
            AppError::BadRequest(_)
        ));
    }

    /// All documented allowed kinds must pass `validate_absence`.
    #[test]
    fn validate_absence_accepts_all_allowed_kinds() {
        for kind in crate::repository::absences::ALLOWED_KINDS {
            let input = NewAbsence {
                kind: kind.to_string(),
                start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                end_date: NaiveDate::from_ymd_opt(2026, 6, 3).unwrap(),
                comment: None,
            };
            assert!(
                validate_absence(&input).is_ok(),
                "kind '{kind}' should be allowed"
            );
        }
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
            kind: "sick".to_string(),
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
        assert_eq!(svc.kind, "sick");
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
