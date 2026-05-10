use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize, Serializer};
use sqlx::FromRow;

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::warn!(target:"zerf::absences", "load notification language failed: {e}");
            i18n::Language::default()
        }
    }
}

/// Send one in-app absence notification to a single recipient.
/// `event` is the i18n key prefix; title and body keys are derived as
/// `{event}_title` / `{event}_body`.
async fn notify_absence(
    app_state: &AppState,
    language: &i18n::Language,
    recipient_id: i64,
    event: &str,
    params: Vec<(&'static str, String)>,
    absence_id: i64,
) {
    crate::notifications::create_translated(
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

/// Notify every approver in `recipient_ids` of the same absence event.
async fn notify_approvers(
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

fn repo_absence_to_service(a: crate::repository::Absence) -> Absence {
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

use crate::repository::absences::ALLOWED_KINDS as ALLOWED_ABSENCE_KINDS;

#[derive(FromRow, Serialize, Clone)]
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
    #[sqlx(default)]
    pub review_type: Option<String>,
    #[sqlx(default)]
    pub previous_kind: Option<String>,
    #[sqlx(default)]
    pub previous_start_date: Option<NaiveDate>,
    #[sqlx(default)]
    pub previous_end_date: Option<NaiveDate>,
    #[sqlx(default)]
    pub previous_comment: Option<String>,
}

fn json_opt_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

fn json_opt_date(value: &serde_json::Value, key: &str) -> Option<NaiveDate> {
    let date_str = value.get(key)?.as_str()?;
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

/// Populate `review_type` and `previous_*` fields on an absence pending review,
/// so the UI can show what changed since the original request.
async fn enrich_pending_review_metadata(
    pool: &crate::db::DatabasePool,
    absence: &mut Absence,
) -> AppResult<()> {
    if absence.status == "cancellation_pending" {
        absence.review_type = Some("cancellation".to_string());
        return Ok(());
    }

    // Assume "approval" (first-time request); upgrade to "change" if we find a prior version.
    absence.review_type = Some("approval".to_string());

    let Some(before_data) =
        crate::repository::AbsenceDb::latest_update_before_data(pool, absence.id).await?
    else {
        return Ok(());
    };

    let Ok(before_json) = serde_json::from_str::<serde_json::Value>(&before_data) else {
        return Ok(());
    };

    absence.review_type = Some("change".to_string());
    absence.previous_kind = json_opt_string(&before_json, "kind");
    absence.previous_start_date = json_opt_date(&before_json, "start_date");
    absence.previous_end_date = json_opt_date(&before_json, "end_date");
    absence.previous_comment = json_opt_string(&before_json, "comment");
    Ok(())
}

pub async fn workdays(
    pool: &crate::db::DatabasePool,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone()).workdays(from, to).await
}

pub async fn workdays_total(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    kind: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone()).workdays_total(user_id, kind, from, to).await
}

#[derive(Deserialize)]
pub struct YearQuery {
    pub year: Option<i32>,
}

fn year_bounds(year: i32) -> AppResult<(NaiveDate, NaiveDate)> {
    let from = NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| AppError::BadRequest("Invalid year.".into()))?;
    let to = NaiveDate::from_ymd_opt(year, 12, 31)
        .ok_or_else(|| AppError::BadRequest("Invalid year.".into()))?;
    Ok((from, to))
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<YearQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());
    let (from, to) = year_bounds(year)?;
    let absences = app_state
        .db
        .absences
        .list_for_user(requester.id, from, to)
        .await?;
    Ok(Json(absences.into_iter().map(repo_absence_to_service).collect()))
}

#[derive(Deserialize)]
pub struct AllQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub status: Option<String>,
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<AllQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let absences = app_state
        .db
        .absences
        .list_all(
            requester.is_admin(),
            requester.id,
            query.from,
            query.to,
            query.status.as_deref(),
        )
        .await?;

    let mut mapped: Vec<Absence> = absences.into_iter().map(repo_absence_to_service).collect();
    if query.status.as_deref() == Some("pending_review") {
        for absence in &mut mapped {
            enrich_pending_review_metadata(&app_state.pool, absence).await?;
        }
    }
    Ok(Json(mapped))
}

#[derive(Deserialize)]
pub struct MonthQuery {
    pub month: String,
}

async fn calendar_scope_user_ids(
    app_state: &AppState,
    requester: &User,
) -> AppResult<Option<Vec<i64>>> {
    app_state
        .db
        .absences
        .calendar_scope_user_ids(requester.id, requester.is_admin(), requester.is_lead())
        .await
}

pub async fn calendar(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<MonthQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    let (year_str, month_str) = query
        .month
        .split_once('-')
        .ok_or_else(|| AppError::BadRequest("month=YYYY-MM required".into()))?;
    let year: i32 = year_str
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid year".into()))?;
    let month: u32 = month_str
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid month".into()))?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| AppError::BadRequest("Invalid date".into()))?;
    let next_month_first = if month == 12 {
        let next_year = year
            .checked_add(1)
            .ok_or_else(|| AppError::BadRequest("Invalid date".into()))?;
        NaiveDate::from_ymd_opt(next_year, 1, 1)
            .ok_or_else(|| AppError::BadRequest("Invalid date".into()))?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
            .ok_or_else(|| AppError::BadRequest("Invalid date".into()))?
    };
    let to = next_month_first - Duration::days(1);
    let scope_user_ids = calendar_scope_user_ids(&app_state, &requester).await?;
    let calendar_entries = app_state
        .db
        .absences
        .calendar_entries(from, to, scope_user_ids.as_deref())
        .await?;
    let requester_is_lead = requester.is_lead();
    // Privacy: only team leads / admins see the actual absence kind. For peers
    // we collapse to a coarse label so that sensitive categories (sick leave —
    // health data under GDPR Art. 9 — training, special leave, unpaid leave)
    // are not disclosed across the team. Vacation stays visible because it is
    // operationally needed to coordinate cover and is not health-related.
    Ok(Json(calendar_entries.into_iter().map(|entry| {
        let is_own_entry = entry.user_id == requester.id;
        let kind_is_visible = requester_is_lead || is_own_entry || entry.kind == "vacation";
        let displayed_kind = if kind_is_visible { entry.kind.clone() } else { "absent".to_string() };
        serde_json::json!({
            "id": entry.id, "user_id": entry.user_id, "name": format!("{} {}", entry.first_name, entry.last_name),
            "kind": displayed_kind,
            "start_date": entry.start_date, "end_date": entry.end_date,
            "status": entry.status,
            "comment": if requester_is_lead || is_own_entry { entry.comment.clone() } else { None }
        })
    }).collect()))
}

#[derive(Deserialize)]
pub struct NewAbsence {
    pub kind: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
}

/// Validate common absence fields and return the kind as a `&str`.
fn validate_absence(input: &NewAbsence) -> AppResult<&str> {
    if !ALLOWED_ABSENCE_KINDS.contains(&input.kind.as_str()) {
        return Err(AppError::BadRequest("Invalid kind".into()));
    }
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

    Ok(&input.kind)
}

fn validate_sick_start_date(kind: &str, start_date: NaiveDate) -> AppResult<()> {
    if kind != "sick" {
        return Ok(());
    }

    let earliest = chrono::Local::now().date_naive() - Duration::days(30);
    if start_date < earliest {
        return Err(AppError::BadRequest(
            "Sick leave cannot be backdated more than 30 days.".into(),
        ));
    }

    Ok(())
}

async fn validate_absence_has_workday(
    pool: &crate::db::DatabasePool,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> AppResult<()> {
    let effective_workdays = workdays(pool, start_date, end_date).await?;
    if effective_workdays <= 0.0 {
        return Err(AppError::BadRequest(
            "Absence must include at least one workday.".into(),
        ));
    }
    Ok(())
}


async fn absence_owner_id(pool: &crate::db::DatabasePool, absence_id: i64) -> AppResult<i64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone()).get_user_id(absence_id).await
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let kind = validate_absence(&body)?;
    validate_sick_start_date(kind, body.start_date)?;
    // Reject absences that start before the user's start_date.
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    validate_absence_has_workday(&app_state.pool, body.start_date, body.end_date).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, requester.id).await?;
    crate::repository::AbsenceDb::assert_no_overlap_tx(&mut transaction, requester.id, body.start_date, body.end_date, None).await?;
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(&mut transaction, requester.id, kind, body.start_date, body.end_date).await?;
    if kind == "vacation" {
        validate_vacation_balance(
            &app_state.pool,
            &mut *transaction,
            &requester,
            body.start_date,
            body.end_date,
            None,
            false,
        )
        .await?;
    }
    // Sick leave is auto-approved only when it has already started (or starts today).
    // Future-dated sick leave requires review like any other request.
    let today_date = chrono::Local::now().date_naive();
    let initial_status = if kind == "sick" && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    let new_absence_id = crate::repository::AbsenceDb::insert_tx(
        &mut transaction, requester.id, kind, body.start_date, body.end_date, body.comment.as_deref(), initial_status,
    ).await?;
    transaction.commit().await?;
    let created_absence = repo_absence_to_service(
        app_state.db.absences.find_by_id(new_absence_id).await?
    );
    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "absences",
        new_absence_id,
        None,
        Some(serde_json::to_value(&created_absence).unwrap()),
    )
    .await;
    if created_absence.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
        notify_approvers(
            &app_state, &language, &approver_ids, "absence_requested",
            vec![
                ("requester_name", requester.full_name()),
                ("kind", i18n::absence_kind_label(&language, &created_absence.kind)),
                ("start_date", i18n::format_date(&language, created_absence.start_date)),
                ("end_date", i18n::format_date(&language, created_absence.end_date)),
            ],
            new_absence_id,
        ).await;
    }
    Ok(Json(created_absence))
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
    Json(body): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let kind = validate_absence(&body)?;
    validate_sick_start_date(kind, body.start_date)?;
    // Reject absences that start before the user's employment start date.
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    validate_absence_has_workday(&app_state.pool, body.start_date, body.end_date).await?;
    let current_owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, current_owner_id).await?;
    let absence_before_update = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?
    );
    if absence_before_update.user_id != requester.id {
        return Err(AppError::Forbidden);
    }
    if absence_before_update.status != "requested" {
        return Err(AppError::BadRequest("Cannot edit.".into()));
    }
    // Sick absences must remain sick: changing kind is never allowed.
    if absence_before_update.kind == "sick" && body.kind != "sick" {
        return Err(AppError::BadRequest(
            "Sick absences cannot change type.".into(),
        ));
    }
    crate::repository::AbsenceDb::assert_no_overlap_tx(&mut transaction, requester.id, body.start_date, body.end_date, Some(absence_id)).await?;
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(&mut transaction, requester.id, kind, body.start_date, body.end_date).await?;
    if kind == "vacation" {
        validate_vacation_balance(
            &app_state.pool,
            &mut *transaction,
            &requester,
            body.start_date,
            body.end_date,
            Some(absence_id),
            false,
        )
        .await?;
    }
    // Sick leave already started today is auto-approved; future-dated requires review.
    let today_date = chrono::Local::now().date_naive();
    let updated_status = if kind == "sick" && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    crate::repository::AbsenceDb::update_fields_tx(
        &mut transaction, absence_id, kind, body.start_date, body.end_date, body.comment.as_deref(), updated_status,
    ).await?;
    transaction.commit().await?;
    let absence_after_update = repo_absence_to_service(
        app_state.db.absences.find_by_id(absence_id).await?
    );
    audit::log(
        &app_state.pool,
        requester.id,
        "updated",
        "absences",
        absence_id,
        Some(serde_json::to_value(&absence_before_update).unwrap()),
        Some(serde_json::to_value(&absence_after_update).unwrap()),
    )
    .await;
    // Notify approvers of the change — they may be reviewing the previous version.
    if absence_after_update.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
        notify_approvers(
            &app_state, &language, &approver_ids, "absence_updated",
            vec![
                ("requester_name", requester.full_name()),
                ("kind", i18n::absence_kind_label(&language, &absence_after_update.kind)),
                ("start_date", i18n::format_date(&language, absence_after_update.start_date)),
                ("end_date", i18n::format_date(&language, absence_after_update.end_date)),
            ],
            absence_id,
        ).await;
    }
    Ok(Json(absence_after_update))
}

pub async fn cancel(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    if owner_id != requester.id {
        return Err(AppError::Forbidden);
    }
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?
    );
    if absence.user_id != requester.id {
        return Err(AppError::Forbidden);
    }

    // Pre-load notification context — identical for both cancel paths.
    let language = notification_language(&app_state.pool).await;
    let approver_ids = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
    let approver_params = vec![
        ("requester_name", requester.full_name()),
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];

    match absence.status.as_str() {
        // Not yet reviewed: cancel immediately and notify approvers.
        "requested" => {
            crate::repository::AbsenceDb::cancel_requested_tx(&mut transaction, absence_id).await?;
            transaction.commit().await?;
            audit::log(
                &app_state.pool,
                requester.id,
                "cancelled",
                "absences",
                absence_id,
                Some(serde_json::to_value(&absence).unwrap()),
                Some(serde_json::json!({"status": "cancelled"})),
            )
            .await;
            notify_approvers(&app_state, &language, &approver_ids, "absence_cancelled", approver_params, absence_id).await;
            Ok(Json(serde_json::json!({"ok": true})))
        }
        // Approved absence: request cancellation approval from approvers.
        "approved" => {
            let rows = crate::repository::AbsenceDb::request_cancellation_tx(
                &mut *transaction,
                absence_id,
            )
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
                Some(serde_json::to_value(&absence).unwrap()),
                Some(serde_json::json!({"status": "cancellation_pending"})),
            )
            .await;
            notify_approvers(&app_state, &language, &approver_ids, "absence_cancellation_requested", approver_params, absence_id).await;
            Ok(Json(serde_json::json!({"ok": true, "pending": true})))
        }
        _ => Err(AppError::BadRequest(
            "Only requested or approved absences can be cancelled.".into(),
        )),
    }
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?
    );
    // A lead may not approve their own absence; admins may.
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
    if !requester.is_admin()
        && !crate::repository::AbsenceDb::is_direct_report_for_update(
            &mut *transaction, absence.user_id, requester.id,
        ).await?
    {
        return Err(AppError::Forbidden);
    }
    if absence.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be approved.".into(),
        ));
    }
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(&mut transaction, absence.user_id, &absence.kind, absence.start_date, absence.end_date).await?;
    // Re-validate vacation balance at approval time — between request and approval
    // another vacation may have been approved or the entitlement may have changed.
    if absence.kind == "vacation" {
        let repo_user = app_state.db.users.find_by_id(absence.user_id).await?.ok_or(AppError::NotFound)?;
        let absence_owner = crate::users::repo_user_to_auth_user(repo_user);
        validate_vacation_balance(
            &app_state.pool,
            &mut *transaction,
            &absence_owner,
            absence.start_date,
            absence.end_date,
            Some(absence_id),
            true,
        )
        .await?;
    }
    // Optimistic lock: status='requested' guard in the UPDATE catches concurrent reviews.
    let rows_updated = crate::repository::AbsenceDb::approve_tx(
        &mut *transaction, absence_id, requester.id,
    ).await?;
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    let before_json = serde_json::to_value(&absence).unwrap();
    let after_json = serde_json::json!({"status": "approved", "reviewed_by": requester.id});
    audit::log(&app_state.pool, requester.id, "approved", "absences", absence_id, Some(before_json.clone()), Some(after_json.clone())).await;
    if absence.user_id != requester.id {
        // Also record in the absence owner's audit trail.
        audit::log(&app_state.pool, absence.user_id, "approved", "absences", absence_id, Some(before_json), Some(after_json)).await;
    }
    let language = notification_language(&app_state.pool).await;
    notify_absence(&app_state, &language, absence.user_id, "absence_approved",
        vec![
            ("kind", i18n::absence_kind_label(&language, &absence.kind)),
            ("start_date", i18n::format_date(&language, absence.start_date)),
            ("end_date", i18n::format_date(&language, absence.end_date)),
        ],
        absence_id,
    ).await;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

pub async fn reject(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
    Json(body): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    if body.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    // Mirror the 2000-char limit applied to absence comments.
    if body.reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long (max 2000).".into()));
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?
    );
    // A lead may not reject their own absence; admins may.
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
    if !requester.is_admin()
        && !crate::repository::AbsenceDb::is_direct_report_for_update(
            &mut *transaction, absence.user_id, requester.id,
        ).await?
    {
        return Err(AppError::Forbidden);
    }
    if absence.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be rejected.".into(),
        ));
    }
    // Optimistic lock: status='requested' guard in the UPDATE catches concurrent reviews.
    let rows_updated = crate::repository::AbsenceDb::reject_tx(
        &mut *transaction, absence_id, requester.id, &body.reason,
    ).await?;
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "absences",
        absence_id,
        Some(serde_json::to_value(&absence).unwrap()),
        Some(serde_json::json!({"status": "rejected", "reason": body.reason})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    notify_absence(&app_state, &language, absence.user_id, "absence_rejected",
        vec![
            ("kind", i18n::absence_kind_label(&language, &absence.kind)),
            ("start_date", i18n::format_date(&language, absence.start_date)),
            ("end_date", i18n::format_date(&language, absence.end_date)),
            ("reason", body.reason.clone()),
        ],
        absence_id,
    ).await;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn approve_cancellation(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?;
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() && !crate::repository::AbsenceDb::is_direct_report_for_update(&mut *transaction, absence.user_id, requester.id).await? {
        return Err(AppError::Forbidden);
    }
    if absence.status != "cancellation_pending" {
        return Err(AppError::BadRequest(
            "Only cancellation-pending absences can have their cancellation approved.".into(),
        ));
    }
    let rows = crate::repository::AbsenceDb::approve_cancellation_tx(
        &mut *transaction,
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
    audit::log(
        &app_state.pool,
        requester.id,
        "cancelled",
        "absences",
        absence_id,
        Some(serde_json::to_value(&absence).unwrap()),
        Some(serde_json::json!({"status": "cancelled", "reviewed_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    notify_absence(&app_state, &language, absence.user_id, "absence_cancellation_approved",
        vec![
            ("kind", i18n::absence_kind_label(&language, &absence.kind)),
            ("start_date", i18n::format_date(&language, absence.start_date)),
            ("end_date", i18n::format_date(&language, absence.end_date)),
        ],
        absence_id,
    ).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn reject_cancellation(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?;
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    if !requester.is_admin() && !crate::repository::AbsenceDb::is_direct_report_for_update(&mut *transaction, absence.user_id, requester.id).await? {
        return Err(AppError::Forbidden);
    }
    if absence.status != "cancellation_pending" {
        return Err(AppError::BadRequest(
            "Only cancellation-pending absences can have their cancellation rejected.".into(),
        ));
    }
    let rows = crate::repository::AbsenceDb::reject_cancellation_tx(
        &mut *transaction,
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
    audit::log(
        &app_state.pool,
        requester.id,
        "cancellation_rejected",
        "absences",
        absence_id,
        Some(serde_json::to_value(&absence).unwrap()),
        Some(serde_json::json!({"status": "approved", "reviewed_by": requester.id})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    notify_absence(&app_state, &language, absence.user_id, "absence_cancellation_rejected",
        vec![
            ("kind", i18n::absence_kind_label(&language, &absence.kind)),
            ("start_date", i18n::format_date(&language, absence.start_date)),
            ("end_date", i18n::format_date(&language, absence.end_date)),
        ],
        absence_id,
    ).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Admin-only: revoke an already-approved absence (e.g. mistaken approval).
/// Transitions the absence to 'cancelled' with an audit trail.
pub async fn revoke(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, owner_id).await?;
    let absence = repo_absence_to_service(
        crate::repository::AbsenceDb::find_for_update(&mut *transaction, absence_id).await?
    );
    if absence.status != "approved" {
        return Err(AppError::BadRequest(
            "Only approved absences can be revoked.".into(),
        ));
    }
    crate::repository::AbsenceDb::revoke_tx(&mut *transaction, absence_id, requester.id).await?;
    transaction.commit().await?;
    audit::log(
        &app_state.pool,
        requester.id,
        "revoked",
        "absences",
        absence_id,
        Some(serde_json::to_value(&absence).unwrap()),
        Some(serde_json::json!({"status": "cancelled", "revoked_by": requester.id})),
    )
    .await;
    if absence.user_id != requester.id {
        // Notify the absence owner that their absence was revoked by an admin.
        let language = notification_language(&app_state.pool).await;
        notify_absence(&app_state, &language, absence.user_id, "absence_revoked",
            vec![
                ("kind", i18n::absence_kind_label(&language, &absence.kind)),
                ("start_date", i18n::format_date(&language, absence.start_date)),
                ("end_date", i18n::format_date(&language, absence.end_date)),
            ],
            absence_id,
        ).await;
    }
    Ok(Json(serde_json::json!({"ok":true})))
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
    /// Carryover from previous year (0 if none or already expired).
    pub carryover_days: i64,
    /// How many carryover days are still remaining (not yet taken).
    #[serde(serialize_with = "serialize_day_count")]
    pub carryover_remaining: f64,
    /// The date (ISO) when carryover expires, if applicable.
    pub carryover_expiry: Option<String>,
    /// Whether the carryover has already expired.
    pub carryover_expired: bool,
}

#[derive(Deserialize)]
pub struct BalanceQuery {
    pub year: Option<i32>,
}

async fn assert_can_access_user(
    app_state: &AppState,
    requester: &User,
    target_uid: i64,
) -> AppResult<()> {
    if requester.id == target_uid || requester.is_admin() {
        return Ok(());
    }
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let is_report = app_state.db.users.is_direct_report(target_uid, requester.id).await?;
    if !is_report {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Helper: resolve the effective annual leave entitlement for a user in a given year.
/// Uses the user_annual_leave table (lazy-creates row on first access).
async fn effective_annual_days(
    pool: &crate::db::DatabasePool,
    user: &crate::auth::User,
    year: i32,
) -> AppResult<i64> {
    crate::users::get_leave_days(pool, user.id, year).await
}

/// Parse the carryover expiry date setting (MM-DD) into a NaiveDate for the given year.
fn parse_expiry_date(setting: &str, year: i32) -> Option<NaiveDate> {
    let (month_str, day_str) = setting.split_once('-')?;
    let month: u32 = month_str.parse().ok()?;
    let configured_day: u32 = day_str.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }

    let next_month_start = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    let max_day = (next_month_start - Duration::days(1)).day();
    let effective_day = configured_day.min(max_day);
    NaiveDate::from_ymd_opt(year, month, effective_day)
}

/// Pro-rate annual leave entitlement for a user who started mid-year.
/// Returns the full entitlement if the user was active the entire year,
/// or zero if they hadn't started yet in `year`.
fn pro_rate_entitlement(user_start_date: NaiveDate, year: i32, entitled: i64) -> i64 {
    let year_start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    if user_start_date > year_end {
        0
    } else if user_start_date > year_start {
        let months_remaining = (13 - user_start_date.month()) as f64;
        ((entitled as f64) * months_remaining / 12.0).ceil() as i64
    } else {
        entitled
    }
}

/// Clamp an arbitrary date range to an inclusive year window.
/// Returns `None` when there is no overlap.
fn clamp_range_to_window(
    start_date: NaiveDate,
    end_date: NaiveDate,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Option<(NaiveDate, NaiveDate)> {
    let clamped_start = std::cmp::max(start_date, window_start);
    let clamped_end = std::cmp::min(end_date, window_end);
    (clamped_start <= clamped_end).then_some((clamped_start, clamped_end))
}

/// Sum workdays for a list of date ranges after clamping each range to the
/// provided inclusive window.
async fn workdays_for_ranges_in_window(
    pool: &crate::db::DatabasePool,
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> AppResult<f64> {
    let mut total = 0.0;
    for (start_date, end_date) in ranges {
        if let Some((clamped_start, clamped_end)) =
            clamp_range_to_window(*start_date, *end_date, window_start, window_end)
        {
            total += workdays(pool, clamped_start, clamped_end).await?;
        }
    }
    Ok(total)
}

/// Build a year-level entitlement context:
/// - `effective_entitlement`: this year's entitlement after user-start pro-rating
/// - `carryover_days`: previous-year unused vacation days
/// - `carryover_expired`: whether previous-year carryover can still be used now
///
/// Policy encoded here:
/// carryover is derived from previous-year entitlement minus previous-year
/// approved/cancellation-pending vacation usage.
async fn vacation_year_context(
    pool: &crate::db::DatabasePool,
    user: &crate::auth::User,
    year: i32,
    today: NaiveDate,
    expiry_setting: &str,
) -> AppResult<(i64, i64, bool)> {
    let entitled = effective_annual_days(pool, user, year).await?;
    let effective_entitlement = pro_rate_entitlement(user.start_date, year, entitled);

    let prev_year = year - 1;
    let prev_entitled = effective_annual_days(pool, user, prev_year).await?;
    let prev_effective = pro_rate_entitlement(user.start_date, prev_year, prev_entitled);
    let prev_year_start = NaiveDate::from_ymd_opt(prev_year, 1, 1).unwrap();
    let prev_year_end = NaiveDate::from_ymd_opt(prev_year, 12, 31).unwrap();
    let prev_taken =
        workdays_total(pool, user.id, "vacation", prev_year_start, prev_year_end).await?;
    let carryover_days = std::cmp::max(0, prev_effective - prev_taken.round() as i64);

    let expiry_date = parse_expiry_date(expiry_setting, year);
    let carryover_expired = expiry_date.map(|d| today > d).unwrap_or(false);
    Ok((effective_entitlement, carryover_days, carryover_expired))
}

/// Total budget usable in a year according to carryover policy.
fn total_entitlement_with_carryover(
    effective_entitlement: i64,
    carryover_days: i64,
    carryover_expired: bool,
) -> f64 {
    if carryover_expired {
        effective_entitlement as f64
    } else {
        effective_entitlement as f64 + carryover_days as f64
    }
}

/// Compute how much carryover remains in the queried year.
///
/// Intent:
/// - carryover is consumed by approved days taken in the queried year
/// - when an expiry date exists, only approved days up to min(expiry, today)
///   consume carryover
/// - without expiry date, all already-taken approved days consume carryover
async fn carryover_remaining_days(
    pool: &crate::db::DatabasePool,
    vacation_absences: &[Absence],
    year_start: NaiveDate,
    today: NaiveDate,
    expiry_date: Option<NaiveDate>,
    carryover_days: i64,
    carryover_expired: bool,
) -> AppResult<f64> {
    if carryover_expired || carryover_days == 0 {
        return Ok(0.0);
    }

    let approved_or_pending_ranges: Vec<(NaiveDate, NaiveDate)> = vacation_absences
        .iter()
        .filter(|absence| {
            absence.status == "approved" || absence.status == "cancellation_pending"
        })
        .map(|absence| (absence.start_date, absence.end_date))
        .collect();
    let consumed = if let Some(expiry) = expiry_date {
        let cutoff = std::cmp::min(expiry, today);
        if cutoff < year_start {
            0.0
        } else {
            workdays_for_ranges_in_window(pool, &approved_or_pending_ranges, year_start, cutoff).await?
        }
    } else {
        workdays_for_ranges_in_window(pool, &approved_or_pending_ranges, year_start, today).await?
    };

    Ok((carryover_days as f64 - consumed).max(0.0))
}

/// Validate that a vacation absence does not exceed the user's remaining entitlement
/// for the affected year(s). `exclude_id` allows excluding the current absence when
/// editing (pass `None` when creating).
async fn validate_vacation_balance(
    pool: &crate::db::DatabasePool,
    tx: &mut sqlx::PgConnection,
    user: &crate::auth::User,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_id: Option<i64>,
    count_new_for_carryover_source: bool,
) -> AppResult<()> {
    use crate::repository::AbsenceDb;

    // Carryover policy matrix (date-driven, not request-driven):
    // 1) vacation_day <= expiry_date: may consume carryover + annual entitlement
    // 2) vacation_day >  expiry_date: may consume annual entitlement only
    // 3) cross-year requests are validated per year with the same split logic
    // 4) carryover source for next year comes from current-year approved/pending-approved usage

    let year = start_date.year();
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = chrono::Local::now().date_naive();
    let expiry_setting =
        crate::settings::load_setting(pool, "carryover_expiry_date", "03-31").await?;
    let (effective_entitlement, carryover_days, _carryover_expired) =
        vacation_year_context(pool, user, year, today, &expiry_setting).await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let total_entitlement = effective_entitlement as f64 + carryover_days as f64;

    // Sum existing vacation usage (requested + approved) in this year, excluding `exclude_id`.
    let existing_ranges =
        AbsenceDb::vacation_ranges_in_year_tx(&mut *tx, user.id, year_from, year_to, exclude_id)
            .await?;
    let used_days = workdays_for_ranges_in_window(pool, &existing_ranges, year_from, year_to).await?;
    // Clamp the new absence to this year and check whether adding it would exceed the budget.
    let new_days = if let Some((new_start, new_end)) =
        clamp_range_to_window(start_date, end_date, year_from, year_to)
    {
        workdays(pool, new_start, new_end).await?
    } else {
        0.0
    };
    if used_days + new_days > total_entitlement {
        return Err(AppError::BadRequest(
            "Not enough remaining vacation days.".into(),
        ));
    }

    // Enforce carryover expiry by absence date, not request/approval date.
    // Days strictly after the configured expiry must be covered by current-year
    // entitlement only; carryover can cover only days on/before expiry.
    if let Some(expiry) = expiry_date {
        let pre_window_end = std::cmp::min(expiry, year_to);
        let post_window_start = expiry + Duration::days(1);

        let pre_existing_days = if year_from <= pre_window_end {
            workdays_for_ranges_in_window(pool, &existing_ranges, year_from, pre_window_end).await?
        } else {
            0.0
        };
        let pre_new_days = if year_from <= pre_window_end {
            if let Some((pre_new_start, pre_new_end)) =
                clamp_range_to_window(start_date, end_date, year_from, pre_window_end)
            {
                workdays(pool, pre_new_start, pre_new_end).await?
            } else {
                0.0
            }
        } else {
            0.0
        };

        let post_existing_days = if post_window_start <= year_to {
            workdays_for_ranges_in_window(pool, &existing_ranges, post_window_start, year_to)
                .await?
        } else {
            0.0
        };
        let post_new_days = if post_window_start <= year_to {
            if let Some((post_new_start, post_new_end)) =
                clamp_range_to_window(start_date, end_date, post_window_start, year_to)
            {
                workdays(pool, post_new_start, post_new_end).await?
            } else {
                0.0
            }
        } else {
            0.0
        };

        let pre_total = pre_existing_days + pre_new_days;
        let post_total = post_existing_days + post_new_days;
        let carryover_budget = carryover_days as f64;
        let base_budget = effective_entitlement as f64;
        let base_consumed_before_or_on_expiry = (pre_total - carryover_budget).max(0.0);
        let base_remaining_after_expiry = (base_budget - base_consumed_before_or_on_expiry).max(0.0);

        if post_total > base_remaining_after_expiry {
            return Err(AppError::BadRequest(
                "Not enough remaining vacation days.".into(),
            ));
        }
    }

    // When the absence spans New Year's Day, validate the end year's budget separately.
    // The current year's unused entitlement becomes next year's carryover.
    let end_year = end_date.year();
    if end_year != year {
        let end_year_from = NaiveDate::from_ymd_opt(end_year, 1, 1).unwrap();
        let end_year_to = NaiveDate::from_ymd_opt(end_year, 12, 31).unwrap();

        let end_year_entitled = effective_annual_days(pool, user, end_year).await?;
        let end_year_effective =
            pro_rate_entitlement(user.start_date, end_year, end_year_entitled);

        // Carryover source is status-based: only approved / cancellation_pending
        // vacation reduces next year's carryover. Requested days reserve current-year
        // availability but do not reduce the carryover source.
        let end_year_expiry_date = parse_expiry_date(&expiry_setting, end_year);
        let current_year_approved_usage = workdays_total(pool, user.id, "vacation", year_from, year_to).await?;
        let current_year_new_approved = if count_new_for_carryover_source {
            if let Some((current_year_new_start, current_year_new_end)) =
                clamp_range_to_window(start_date, end_date, year_from, year_to)
            {
                workdays(pool, current_year_new_start, current_year_new_end).await?
            } else {
                0.0
            }
        } else {
            0.0
        };
        let current_year_total_usage = current_year_approved_usage + current_year_new_approved;
        let current_year_carryover = std::cmp::max(
            0,
            effective_entitlement - current_year_total_usage.round() as i64,
        );
        // Do not collapse carryover based on today's date here. We validate by
        // vacation day date (pre/post expiry split below), not by request day.
        let end_year_total = end_year_effective as f64 + current_year_carryover as f64;

        let end_year_existing = AbsenceDb::vacation_ranges_in_year_tx(
            &mut *tx,
            user.id,
            end_year_from,
            end_year_to,
            exclude_id,
        )
        .await?;
        let end_year_used =
            workdays_for_ranges_in_window(pool, &end_year_existing, end_year_from, end_year_to)
                .await?;
        let end_new_days = if let Some((end_new_start, end_new_end)) =
            clamp_range_to_window(start_date, end_date, end_year_from, end_year_to)
        {
            workdays(pool, end_new_start, end_new_end).await?
        } else {
            0.0
        };
        if end_year_used + end_new_days > end_year_total {
            return Err(AppError::BadRequest(
                "Not enough remaining vacation days.".into(),
            ));
        }

        // Apply the same post-expiry rule to the end year: days strictly after
        // expiry must be covered by end-year entitlement only.
        if let Some(end_year_expiry) = end_year_expiry_date {
            let end_pre_window_end = std::cmp::min(end_year_expiry, end_year_to);
            let end_post_window_start = end_year_expiry + Duration::days(1);

            let end_pre_existing_days = if end_year_from <= end_pre_window_end {
                workdays_for_ranges_in_window(
                    pool,
                    &end_year_existing,
                    end_year_from,
                    end_pre_window_end,
                )
                .await?
            } else {
                0.0
            };
            let end_pre_new_days = if end_year_from <= end_pre_window_end {
                if let Some((end_pre_new_start, end_pre_new_end)) = clamp_range_to_window(
                    start_date,
                    end_date,
                    end_year_from,
                    end_pre_window_end,
                ) {
                    workdays(pool, end_pre_new_start, end_pre_new_end).await?
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let end_post_existing_days = if end_post_window_start <= end_year_to {
                workdays_for_ranges_in_window(
                    pool,
                    &end_year_existing,
                    end_post_window_start,
                    end_year_to,
                )
                .await?
            } else {
                0.0
            };
            let end_post_new_days = if end_post_window_start <= end_year_to {
                if let Some((end_post_new_start, end_post_new_end)) = clamp_range_to_window(
                    start_date,
                    end_date,
                    end_post_window_start,
                    end_year_to,
                ) {
                    workdays(pool, end_post_new_start, end_post_new_end).await?
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let end_pre_total = end_pre_existing_days + end_pre_new_days;
            let end_post_total = end_post_existing_days + end_post_new_days;
            let end_carryover_budget = current_year_carryover as f64;
            let end_base_budget = end_year_effective as f64;
            let end_base_consumed_before_or_on_expiry =
                (end_pre_total - end_carryover_budget).max(0.0);
            let end_base_remaining_after_expiry =
                (end_base_budget - end_base_consumed_before_or_on_expiry).max(0.0);

            if end_post_total > end_base_remaining_after_expiry {
                return Err(AppError::BadRequest(
                    "Not enough remaining vacation days.".into(),
                ));
            }
        }
    }
    Ok(())
}

pub async fn balance(
    State(app_state): State<AppState>,
    requester: User,
    Path(target_user_id): Path<i64>,
    Query(query): Query<BalanceQuery>,
) -> AppResult<Json<LeaveBalance>> {
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
    // Default to the current year if none was provided.
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());
    let repo_user = app_state.db.users.find_by_id(target_user_id).await?
        .ok_or(AppError::NotFound)?;
    let target_user = crate::users::repo_user_to_auth_user(repo_user);
    let (year_from, year_to) = year_bounds(year)?;
    let today = chrono::Local::now().date_naive();
    // Load all vacation absences (requested + approved) in the given year.
    let vacation_absences: Vec<Absence> = app_state
        .db
        .absences
        .vacation_absences_in_year(target_user_id, year_from, year_to)
        .await?
        .into_iter()
        .map(repo_absence_to_service)
        .collect();
    // Categorize each vacation absence into taken, upcoming, or requested buckets.
    // cancellation_pending is treated as requested because those days are still
    // reserved until a lead/admin approves the cancellation.
    let mut taken_days = 0.0;
    let mut upcoming_days = 0.0;
    let mut requested_days = 0.0;
    for absence in &vacation_absences {
        let clamped_start = std::cmp::max(absence.start_date, year_from);
        let clamped_end = std::cmp::min(absence.end_date, year_to);
        if absence.status == "approved" {
            if clamped_end < today {
                // Absence is entirely in the past.
                taken_days += workdays(&app_state.pool, clamped_start, clamped_end).await?;
            } else if clamped_start >= today {
                // Absence is entirely in the future.
                upcoming_days += workdays(&app_state.pool, clamped_start, clamped_end).await?;
            } else {
                // Absence spans today: count today as already taken and only keep
                // days strictly after today in the upcoming bucket.
                taken_days += workdays(&app_state.pool, clamped_start, today).await?;
                let tomorrow = today + Duration::days(1);
                if tomorrow <= clamped_end {
                    upcoming_days += workdays(&app_state.pool, tomorrow, clamped_end).await?;
                }
            }
        } else if absence.status == "requested" || absence.status == "cancellation_pending" {
            requested_days += workdays(&app_state.pool, clamped_start, clamped_end).await?;
        }
    }

    // -- Carryover policy context for this year --
    let expiry_setting =
        crate::settings::load_setting(&app_state.pool, "carryover_expiry_date", "03-31").await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let (effective_entitlement, carryover_days, carryover_expired) = vacation_year_context(
        &app_state.pool,
        &target_user,
        year,
        today,
        &expiry_setting,
    )
    .await?;
    let carryover_remaining = carryover_remaining_days(
        &app_state.pool,
        &vacation_absences,
        year_from,
        today,
        expiry_date,
        carryover_days,
        carryover_expired,
    )
    .await?;

    // Total available is an annual frame value (entitlement + active carryover),
    // then reduced by taken/upcoming/requested days. It is intentionally not a
    // date-window-specific "bookable after expiry" value.
    let total_entitlement =
        total_entitlement_with_carryover(effective_entitlement, carryover_days, carryover_expired);
    let available = total_entitlement - taken_days - upcoming_days - requested_days;

    Ok(Json(LeaveBalance {
        annual_entitlement: effective_entitlement,
        already_taken: taken_days,
        approved_upcoming: upcoming_days,
        requested: requested_days,
        available,
        carryover_days,
        carryover_remaining,
        carryover_expiry: expiry_date.map(|d| d.to_string()),
        carryover_expired,
    }))
}
