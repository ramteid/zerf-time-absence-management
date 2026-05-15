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
    crate::notifications::load_language(pool).await
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

/// Send one in-app-only absence notification (no email). Used when the
/// requester is also the absence owner, e.g. an admin self-approving.
async fn notify_absence_inapp_only(
    app_state: &AppState,
    language: &i18n::Language,
    recipient_id: i64,
    event: &str,
    params: Vec<(&'static str, String)>,
    absence_id: i64,
) {
    crate::notifications::create_translated_inapp_only(
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

fn absence_period_params(
    language: &i18n::Language,
    requester: &User,
    absence: &Absence,
) -> Vec<(&'static str, String)> {
    vec![
        ("requester_name", requester.full_name()),
        ("kind", i18n::absence_kind_label(language, &absence.kind)),
        (
            "start_date",
            i18n::format_date(language, absence.start_date),
        ),
        ("end_date", i18n::format_date(language, absence.end_date)),
    ]
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

/// Populate `review_type` and `previous_*` fields using a pre-fetched map of
/// absence_id to latest audit before_data, avoiding per-row DB queries.
fn enrich_absence_with_metadata(
    absence: &mut Absence,
    before_data_map: &std::collections::HashMap<i64, String>,
) {
    if absence.status == "cancellation_pending" {
        absence.review_type = Some("cancellation".to_string());
        return;
    }

    // Assume "approval" (first-time request); upgrade to "change" if we find a prior version.
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

/// Count contract workdays in a date range for a specific user.
/// Respects the user's workdays_per_week configuration (1-7 days per week).
/// Excludes public holidays.
///
/// Used throughout the absence/vacation logic to calculate:
///   - Vacation days used
///   - Leave balance deductions
///   - Carryover calculations
pub async fn workdays(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone())
        .workdays_for_user(user_id, from, to)
        .await
}

pub async fn workdays_total(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    kind: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone())
        .workdays_total(user_id, kind, from, to)
        .await
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
    let year = match query.year {
        Some(value) => value,
        None => crate::settings::app_current_year(&app_state.pool).await,
    };
    let (from, to) = year_bounds(year)?;
    let absences = app_state
        .db
        .absences
        .list_for_user(requester.id, from, to)
        .await?;
    Ok(Json(
        absences.into_iter().map(repo_absence_to_service).collect(),
    ))
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
        let ids: Vec<i64> = mapped.iter().map(|a| a.id).collect();
        let before_data_map =
            crate::repository::AbsenceDb::latest_update_before_data_batch(&app_state.pool, &ids)
                .await?;
        for absence in &mut mapped {
            enrich_absence_with_metadata(absence, &before_data_map);
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
    if (input.end_date - input.start_date).num_days() >= 365 {
        return Err(AppError::BadRequest(
            "Absence range exceeds one year.".into(),
        ));
    }

    Ok(&input.kind)
}

fn validate_sick_start_date(kind: &str, start_date: NaiveDate, today: NaiveDate) -> AppResult<()> {
    if kind != "sick" {
        return Ok(());
    }

    let earliest = today - Duration::days(30);
    if start_date < earliest {
        return Err(AppError::BadRequest(
            "Sick leave cannot be backdated more than 30 days.".into(),
        ));
    }

    Ok(())
}

/// Check whether the date range contains at least one effective workday:
/// a day that is both a contract workday (per workdays_per_week) and not a
/// public holiday. The doc requires "not weekend-only, not holiday-only".
fn has_effective_workday(
    start_date: NaiveDate,
    end_date: NaiveDate,
    workdays_per_week: i16,
    holidays: &std::collections::HashSet<NaiveDate>,
) -> bool {
    let mut day = start_date;
    while day <= end_date {
        let is_contract_day = day.weekday().num_days_from_monday() < workdays_per_week as u32;
        if is_contract_day && !holidays.contains(&day) {
            return true;
        }
        day += Duration::days(1);
    }
    false
}

/// Validate that the absence range includes at least one effective workday
/// (not weekend-only, not holiday-only) as required by the user guide.
async fn validate_absence_has_workday(
    pool: &crate::db::DatabasePool,
    workdays_per_week: i16,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> AppResult<()> {
    let holidays = crate::repository::HolidayDb::new(pool.clone())
        .get_dates_in_range(start_date, end_date)
        .await?;
    if !has_effective_workday(start_date, end_date, workdays_per_week, &holidays) {
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
    let today_date = crate::settings::app_today(&app_state.pool).await;
    let kind = validate_absence(&body)?;
    validate_sick_start_date(kind, body.start_date, today_date)?;
    // Reject absences that start before the user's start_date.
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    if !crate::roles::is_assistant_role(&requester.role) {
        validate_absence_has_workday(
            &app_state.pool,
            requester.workdays_per_week,
            body.start_date,
            body.end_date,
        )
        .await?;
    }
    let mut transaction = app_state.pool.begin().await?;
    crate::repository::AbsenceDb::lock_user_scope_tx(&mut transaction, requester.id).await?;
    crate::repository::AbsenceDb::assert_no_overlap_tx(
        &mut transaction,
        requester.id,
        body.start_date,
        body.end_date,
        None,
    )
    .await?;
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(
        &mut transaction,
        requester.id,
        kind,
        body.start_date,
        body.end_date,
    )
    .await?;
    if kind == "vacation" {
        validate_vacation_balance(
            &app_state.pool,
            &mut transaction,
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
    let initial_status = if kind == "sick" && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    let new_absence_id = crate::repository::AbsenceDb::insert_tx(
        &mut transaction,
        requester.id,
        kind,
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
            crate::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
        notify_approvers(
            &app_state,
            &language,
            &approver_ids,
            "absence_requested",
            absence_period_params(&language, &requester, &created_absence),
            new_absence_id,
        )
        .await;
    } else if created_absence.kind == "sick" && created_absence.status == "approved" {
        let language = notification_language(&app_state.pool).await;
        let mut approver_ids =
            crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
        if !requester.is_admin() {
            approver_ids.retain(|recipient_id| *recipient_id != requester.id);
        }
        notify_approvers(
            &app_state,
            &language,
            &approver_ids,
            "absence_auto_approved_notice",
            absence_period_params(&language, &requester, &created_absence),
            new_absence_id,
        )
        .await;
    }
    Ok(Json(created_absence))
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
    Json(body): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let today_date = crate::settings::app_today(&app_state.pool).await;
    let kind = validate_absence(&body)?;
    validate_sick_start_date(kind, body.start_date, today_date)?;
    // Reject absences that start before the user's employment start date.
    if body.start_date < requester.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    if !crate::roles::is_assistant_role(&requester.role) {
        validate_absence_has_workday(
            &app_state.pool,
            requester.workdays_per_week,
            body.start_date,
            body.end_date,
        )
        .await?;
    }
    let current_owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
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
    // Sick absences must remain sick: changing kind is never allowed.
    if absence_before_update.kind == "sick" && body.kind != "sick" {
        return Err(AppError::BadRequest(
            "Sick absences cannot change type.".into(),
        ));
    }
    if absence_before_update.kind != "sick" && body.kind == "sick" {
        return Err(AppError::BadRequest(
            "Create a separate sick leave request instead of converting another absence type."
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
    crate::repository::AbsenceDb::ensure_no_time_conflict_tx(
        &mut transaction,
        requester.id,
        kind,
        body.start_date,
        body.end_date,
    )
    .await?;
    if kind == "vacation" {
        validate_vacation_balance(
            &app_state.pool,
            &mut transaction,
            &requester,
            body.start_date,
            body.end_date,
            Some(absence_id),
            false,
        )
        .await?;
    }
    // Sick leave already started today is auto-approved; future-dated requires review.
    let updated_status = if kind == "sick" && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    crate::repository::AbsenceDb::update_fields_tx(
        &mut transaction,
        absence_id,
        kind,
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
    // Notify approvers of the change — they may be reviewing the previous version.
    if absence_after_update.status == "requested" {
        let language = notification_language(&app_state.pool).await;
        let approver_ids =
            crate::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
        notify_approvers(
            &app_state,
            &language,
            &approver_ids,
            "absence_updated",
            absence_period_params(&language, &requester, &absence_after_update),
            absence_id,
        )
        .await;
    } else if absence_after_update.kind == "sick" && absence_after_update.status == "approved" {
        let language = notification_language(&app_state.pool).await;
        let mut approver_ids =
            crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
        if !requester.is_admin() {
            approver_ids.retain(|recipient_id| *recipient_id != requester.id);
        }
        notify_approvers(
            &app_state,
            &language,
            &approver_ids,
            "absence_auto_approved_notice",
            absence_period_params(&language, &requester, &absence_after_update),
            absence_id,
        )
        .await;
    }
    Ok(Json(absence_after_update))
}

pub async fn cancel(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut transaction = app_state.pool.begin().await?;
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
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        (
            "start_date",
            i18n::format_date(&language, absence.start_date),
        ),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];

    match absence.status.as_str() {
        // Not yet reviewed: cancel immediately and notify approvers.
        // Use non-failing recipient lookup — cancellation of a requested absence
        // is a withdrawal, not a routed approval, so missing approvers must not
        // block the operation.
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
                crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
            notify_approvers(
                &app_state,
                &language,
                &approver_ids,
                "absence_cancelled",
                approver_params,
                absence_id,
            )
            .await;
            Ok(Json(serde_json::json!({"ok": true})))
        }
        // Approved absence: request cancellation approval from approvers.
        // Requires at least one active approver because this needs a review decision.
        "approved" => {
            let approver_ids =
                crate::auth::required_approval_recipient_ids(&app_state.pool, &requester).await?;
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
                &app_state,
                &language,
                &approver_ids,
                "absence_cancellation_requested",
                approver_params,
                absence_id,
            )
            .await;
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
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    // A lead may not approve their own absence; admins may.
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
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
        &absence.kind,
        absence.start_date,
        absence.end_date,
    )
    .await?;
    // Re-validate vacation balance at approval time — between request and approval
    // another vacation may have been approved or the entitlement may have changed.
    if absence.kind == "vacation" {
        let repo_user = app_state
            .db
            .users
            .find_by_id(absence.user_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let absence_owner = crate::users::repo_user_to_auth_user(repo_user);
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
    // Optimistic lock: status='requested' guard in the UPDATE catches concurrent reviews.
    let rows_updated =
        crate::repository::AbsenceDb::approve_tx(&mut transaction, absence_id, requester.id)
            .await?;
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    transaction.commit().await?;
    let before_json = serde_json::to_value(&absence).ok();
    let after_json = serde_json::json!({"status": "approved", "reviewed_by": requester.id});
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "absences",
        absence_id,
        before_json,
        Some(after_json),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(&app_state, &language, absence.user_id, "absence_approved", notify_params, absence_id).await;
    } else {
        // Self-approval by admin: in-app only, no email.
        notify_absence_inapp_only(&app_state, &language, absence.user_id, "absence_approved", notify_params, absence_id).await;
    }
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
        crate::repository::AbsenceDb::find_for_update(&mut transaction, absence_id).await?,
    );
    // A lead may not reject their own absence; admins may.
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
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
    // Optimistic lock: status='requested' guard in the UPDATE catches concurrent reviews.
    let rows_updated = crate::repository::AbsenceDb::reject_tx(
        &mut transaction,
        absence_id,
        requester.id,
        &body.reason,
    )
    .await?;
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
        serde_json::to_value(&absence).ok(),
        Some(serde_json::json!({"status": "rejected", "reason": body.reason})),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
        ("reason", body.reason.clone()),
    ];
    if absence.user_id != requester.id {
        notify_absence(&app_state, &language, absence.user_id, "absence_rejected", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(&app_state, &language, absence.user_id, "absence_rejected", notify_params, absence_id).await;
    }
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
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(&app_state, &language, absence.user_id, "absence_cancellation_approved", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(&app_state, &language, absence.user_id, "absence_cancellation_approved", notify_params, absence_id).await;
    }
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
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(&app_state, &language, absence.user_id, "absence_cancellation_rejected", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(&app_state, &language, absence.user_id, "absence_cancellation_rejected", notify_params, absence_id).await;
    }
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
    // Notify the absence owner that their absence was revoked by an admin.
    let language = notification_language(&app_state.pool).await;
    let notify_params = vec![
        ("kind", i18n::absence_kind_label(&language, &absence.kind)),
        ("start_date", i18n::format_date(&language, absence.start_date)),
        ("end_date", i18n::format_date(&language, absence.end_date)),
    ];
    if absence.user_id != requester.id {
        notify_absence(&app_state, &language, absence.user_id, "absence_revoked", notify_params, absence_id).await;
    } else {
        notify_absence_inapp_only(&app_state, &language, absence.user_id, "absence_revoked", notify_params, absence_id).await;
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
    let is_report = app_state
        .db
        .users
        .is_direct_report(target_uid, requester.id)
        .await?;
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

async fn annual_days_or_default(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    year: i32,
    default_days: i64,
) -> AppResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT days FROM user_annual_leave WHERE user_id=$1 AND year=$2",
    )
    .bind(user_id)
    .bind(year)
    .fetch_optional(pool)
    .await?
    .unwrap_or(default_days))
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
    user_id: i64,
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> AppResult<f64> {
    let mut total = 0.0;
    for (start_date, end_date) in ranges {
        if let Some((clamped_start, clamped_end)) =
            clamp_range_to_window(*start_date, *end_date, window_start, window_end)
        {
            total += workdays(pool, user_id, clamped_start, clamped_end).await?;
        }
    }
    Ok(total)
}

/// Build a year-level entitlement context:
/// - `effective_entitlement`: this year's entitlement after user-start pro-rating
/// - `carryover_days`: previous-year unused base entitlement
/// - `carryover_expired`: whether previous-year carryover can still be used now
///
/// Policy encoded here:
/// carryover is derived from each previous year's base entitlement after
/// approved usage has consumed any active incoming carryover first.
/// Cancellation-pending days from previous years are intentionally excluded:
/// while a cancellation is undecided we treat it optimistically (favoring the
/// user) so new-year carryover is not suppressed by a request that may yet be
/// cancelled. If the cancellation is later rejected the day reverts to approved
/// and the carryover recomputes downward on the next read.
async fn vacation_year_context(
    pool: &crate::db::DatabasePool,
    user: &crate::auth::User,
    year: i32,
    today: NaiveDate,
    expiry_setting: &str,
) -> AppResult<(i64, i64, bool)> {
    let entitled = effective_annual_days(pool, user, year).await?;
    let effective_entitlement = pro_rate_entitlement(user.start_date, year, entitled);
    let carryover_days = carryover_days_into_year(pool, user, year, expiry_setting).await?;

    let expiry_date = parse_expiry_date(expiry_setting, year);
    let carryover_expired = expiry_date.map(|d| today > d).unwrap_or(false);
    Ok((effective_entitlement, carryover_days, carryover_expired))
}

async fn carryover_days_into_year(
    pool: &crate::db::DatabasePool,
    user: &crate::auth::User,
    year: i32,
    expiry_setting: &str,
) -> AppResult<i64> {
    if year <= user.start_date.year() {
        return Ok(0);
    }

    let absence_db = crate::repository::AbsenceDb::new(pool.clone());
    let default_days = crate::repository::UserDb::new(pool.clone())
        .get_default_leave_days()
        .await?;
    let mut incoming_carryover = 0;

    for source_year in user.start_date.year()..year {
        let entitled = annual_days_or_default(pool, user.id, source_year, default_days).await?;
        let effective_entitlement = pro_rate_entitlement(user.start_date, source_year, entitled);
        let year_from = NaiveDate::from_ymd_opt(source_year, 1, 1).unwrap();
        let year_to = NaiveDate::from_ymd_opt(source_year, 12, 31).unwrap();
        let expiry_date = parse_expiry_date(expiry_setting, source_year);

        let base_usage = if let Some(expiry) = expiry_date {
            let pre_window_end = std::cmp::min(expiry, year_to);
            let post_window_start = expiry + Duration::days(1);
            let pre_usage = if year_from <= pre_window_end {
                absence_db
                    .workdays_total_filtered(
                        user.id,
                        "vacation",
                        year_from,
                        pre_window_end,
                        &["approved"],
                    )
                    .await?
            } else {
                0.0
            };
            let post_usage = if post_window_start <= year_to {
                absence_db
                    .workdays_total_filtered(
                        user.id,
                        "vacation",
                        post_window_start,
                        year_to,
                        &["approved"],
                    )
                    .await?
            } else {
                0.0
            };
            post_usage + (pre_usage - incoming_carryover as f64).max(0.0)
        } else {
            let total_usage = absence_db
                .workdays_total_filtered(user.id, "vacation", year_from, year_to, &["approved"])
                .await?;
            (total_usage - incoming_carryover as f64).max(0.0)
        };

        incoming_carryover = std::cmp::max(0, effective_entitlement - base_usage.round() as i64);
    }

    Ok(incoming_carryover)
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

fn total_entitlement_for_dated_vacation(
    effective_entitlement: i64,
    carryover_days: i64,
    expiry_date: Option<NaiveDate>,
    carryover_expired: bool,
) -> f64 {
    if expiry_date.is_some() {
        // When there is a carryover expiry system, the gross year budget is always
        // effective + carryover regardless of whether today is past the expiry date.
        // The fine-grained pre/post-expiry check in validate_vacation_balance enforces
        // that post-expiry days may only draw from the base entitlement; this total is
        // a conservative upper bound used only for the fast-path budget check.
        effective_entitlement as f64 + carryover_days as f64
    } else {
        total_entitlement_with_carryover(effective_entitlement, carryover_days, carryover_expired)
    }
}

const VACATION_DAY_EPSILON: f64 = 0.000_001;

fn exceeds_vacation_budget(required_days: f64, budget_days: f64) -> bool {
    required_days - budget_days > VACATION_DAY_EPSILON
}

async fn approved_vacation_ranges_in_year_tx(
    tx: &mut sqlx::PgConnection,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
    exclude_id: Option<i64>,
) -> AppResult<Vec<(NaiveDate, NaiveDate)>> {
    if let Some(exclude_id) = exclude_id {
        Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
            "SELECT start_date, end_date FROM absences \
             WHERE id != $1 AND user_id=$2 AND kind='vacation' \
             AND status='approved' \
             AND end_date >= $3 AND start_date <= $4",
        )
        .bind(exclude_id)
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(tx)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, (NaiveDate, NaiveDate)>(
            "SELECT start_date, end_date FROM absences \
             WHERE user_id=$1 AND kind='vacation' \
             AND status='approved' \
             AND end_date >= $2 AND start_date <= $3",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .fetch_all(tx)
        .await?)
    }
}

#[allow(clippy::too_many_arguments)]
async fn carryover_from_year_into_next_year(
    pool: &crate::db::DatabasePool,
    tx: &mut sqlx::PgConnection,
    user_id: i64,
    year_from: NaiveDate,
    year_to: NaiveDate,
    effective_entitlement: i64,
    carryover_days: i64,
    expiry_date: Option<NaiveDate>,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_id: Option<i64>,
    count_new_for_carryover_source: bool,
) -> AppResult<i64> {
    let mut approved_ranges =
        approved_vacation_ranges_in_year_tx(tx, user_id, year_from, year_to, exclude_id).await?;
    if count_new_for_carryover_source {
        if let Some((new_start, new_end)) =
            clamp_range_to_window(start_date, end_date, year_from, year_to)
        {
            approved_ranges.push((new_start, new_end));
        }
    }

    let base_usage = if let Some(expiry) = expiry_date {
        let pre_window_end = std::cmp::min(expiry, year_to);
        let post_window_start = expiry + Duration::days(1);
        let pre_usage = if year_from <= pre_window_end {
            workdays_for_ranges_in_window(
                pool,
                user_id,
                &approved_ranges,
                year_from,
                pre_window_end,
            )
            .await?
        } else {
            0.0
        };
        let post_usage = if post_window_start <= year_to {
            workdays_for_ranges_in_window(
                pool,
                user_id,
                &approved_ranges,
                post_window_start,
                year_to,
            )
            .await?
        } else {
            0.0
        };
        post_usage + (pre_usage - carryover_days as f64).max(0.0)
    } else {
        let total_usage =
            workdays_for_ranges_in_window(pool, user_id, &approved_ranges, year_from, year_to)
                .await?;
        (total_usage - carryover_days as f64).max(0.0)
    };

    Ok(std::cmp::max(
        0,
        effective_entitlement - base_usage.round() as i64,
    ))
}

/// Compute how much carryover remains in the queried year.
///
/// Intent:
/// - carryover is consumed by approved days taken in the queried year
/// - when an expiry date exists, only approved days up to min(expiry, today)
///   consume carryover
/// - without expiry date, all already-taken approved days consume carryover
struct CarryoverRemainingInput<'a> {
    pool: &'a crate::db::DatabasePool,
    user_id: i64,
    vacation_absences: &'a [Absence],
    year_start: NaiveDate,
    today: NaiveDate,
    expiry_date: Option<NaiveDate>,
    carryover_days: i64,
    carryover_expired: bool,
}

async fn carryover_remaining_days(input: CarryoverRemainingInput<'_>) -> AppResult<f64> {
    let CarryoverRemainingInput {
        pool,
        user_id,
        vacation_absences,
        year_start,
        today,
        expiry_date,
        carryover_days,
        carryover_expired,
    } = input;

    if carryover_expired || carryover_days == 0 {
        return Ok(0.0);
    }

    let approved_or_pending_ranges: Vec<(NaiveDate, NaiveDate)> = vacation_absences
        .iter()
        .filter(|absence| absence.status == "approved" || absence.status == "cancellation_pending")
        .map(|absence| (absence.start_date, absence.end_date))
        .collect();
    let consumed = if let Some(expiry) = expiry_date {
        let cutoff = std::cmp::min(expiry, today);
        if cutoff < year_start {
            0.0
        } else {
            workdays_for_ranges_in_window(
                pool,
                user_id,
                &approved_or_pending_ranges,
                year_start,
                cutoff,
            )
            .await?
        }
    } else {
        workdays_for_ranges_in_window(
            pool,
            user_id,
            &approved_or_pending_ranges,
            year_start,
            today,
        )
        .await?
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

    // Validate that the user has sufficient vacation balance. Vacation days
    // are counted as contract workdays only, not calendar days.
    // Carryover policy matrix (date-driven, not request-driven):
    // 1) vacation_day <= expiry_date: may consume carryover + annual entitlement
    // 2) vacation_day >  expiry_date: may consume annual entitlement only
    // 3) cross-year requests are validated per year with the same split logic
    // 4) carryover source for next year comes from current-year base entitlement
    //    after approved usage has consumed any active carryover first.

    let year = start_date.year();
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = crate::settings::app_today(pool).await;
    let expiry_setting =
        crate::settings::load_setting(pool, "carryover_expiry_date", "03-31").await?;
    let (effective_entitlement, carryover_days, carryover_expired) =
        vacation_year_context(pool, user, year, today, &expiry_setting).await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let total_year_budget = total_entitlement_for_dated_vacation(
        effective_entitlement,
        carryover_days,
        expiry_date,
        carryover_expired,
    );

    // Sum existing vacation usage (requested + approved) in this year, excluding `exclude_id`.
    let existing_ranges =
        AbsenceDb::vacation_ranges_in_year_tx(&mut *tx, user.id, year_from, year_to, exclude_id)
            .await?;
    let used_days =
        workdays_for_ranges_in_window(pool, user.id, &existing_ranges, year_from, year_to).await?;
    // Clamp the new absence to this year and check whether adding it would exceed the budget.
    let new_days = if let Some((new_start, new_end)) =
        clamp_range_to_window(start_date, end_date, year_from, year_to)
    {
        workdays(pool, user.id, new_start, new_end).await?
    } else {
        0.0
    };
    if exceeds_vacation_budget(used_days + new_days, total_year_budget) {
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
            workdays_for_ranges_in_window(
                pool,
                user.id,
                &existing_ranges,
                year_from,
                pre_window_end,
            )
            .await?
        } else {
            0.0
        };
        let pre_new_days = if year_from <= pre_window_end {
            if let Some((pre_new_start, pre_new_end)) =
                clamp_range_to_window(start_date, end_date, year_from, pre_window_end)
            {
                workdays(pool, user.id, pre_new_start, pre_new_end).await?
            } else {
                0.0
            }
        } else {
            0.0
        };

        let post_existing_days = if post_window_start <= year_to {
            workdays_for_ranges_in_window(
                pool,
                user.id,
                &existing_ranges,
                post_window_start,
                year_to,
            )
            .await?
        } else {
            0.0
        };
        let post_new_days = if post_window_start <= year_to {
            if let Some((post_new_start, post_new_end)) =
                clamp_range_to_window(start_date, end_date, post_window_start, year_to)
            {
                workdays(pool, user.id, post_new_start, post_new_end).await?
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
        let base_remaining_after_expiry =
            (base_budget - base_consumed_before_or_on_expiry).max(0.0);

        if exceeds_vacation_budget(post_total, base_remaining_after_expiry) {
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
        let end_year_effective = pro_rate_entitlement(user.start_date, end_year, end_year_entitled);

        // Carryover source uses approved-only usage. Cancellation-pending and
        // requested days do not reduce next year's carryover; they only reserve
        // current-year availability. This is consistent with vacation_year_context.
        let end_year_expiry_date = parse_expiry_date(&expiry_setting, end_year);
        let current_year_carryover = carryover_from_year_into_next_year(
            pool,
            tx,
            user.id,
            year_from,
            year_to,
            effective_entitlement,
            carryover_days,
            expiry_date,
            start_date,
            end_date,
            exclude_id,
            count_new_for_carryover_source,
        )
        .await?;
        let end_year_carryover_expired = end_year_expiry_date
            .map(|expiry| today > expiry)
            .unwrap_or(false);
        let end_year_total = total_entitlement_for_dated_vacation(
            end_year_effective,
            current_year_carryover,
            end_year_expiry_date,
            end_year_carryover_expired,
        );

        let end_year_existing = AbsenceDb::vacation_ranges_in_year_tx(
            &mut *tx,
            user.id,
            end_year_from,
            end_year_to,
            exclude_id,
        )
        .await?;
        let end_year_used = workdays_for_ranges_in_window(
            pool,
            user.id,
            &end_year_existing,
            end_year_from,
            end_year_to,
        )
        .await?;
        let end_new_days = if let Some((end_new_start, end_new_end)) =
            clamp_range_to_window(start_date, end_date, end_year_from, end_year_to)
        {
            workdays(pool, user.id, end_new_start, end_new_end).await?
        } else {
            0.0
        };
        if exceeds_vacation_budget(end_year_used + end_new_days, end_year_total) {
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
                    user.id,
                    &end_year_existing,
                    end_year_from,
                    end_pre_window_end,
                )
                .await?
            } else {
                0.0
            };
            let end_pre_new_days = if end_year_from <= end_pre_window_end {
                if let Some((end_pre_new_start, end_pre_new_end)) =
                    clamp_range_to_window(start_date, end_date, end_year_from, end_pre_window_end)
                {
                    workdays(pool, user.id, end_pre_new_start, end_pre_new_end).await?
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let end_post_existing_days = if end_post_window_start <= end_year_to {
                workdays_for_ranges_in_window(
                    pool,
                    user.id,
                    &end_year_existing,
                    end_post_window_start,
                    end_year_to,
                )
                .await?
            } else {
                0.0
            };
            let end_post_new_days = if end_post_window_start <= end_year_to {
                if let Some((end_post_new_start, end_post_new_end)) =
                    clamp_range_to_window(start_date, end_date, end_post_window_start, end_year_to)
                {
                    workdays(pool, user.id, end_post_new_start, end_post_new_end).await?
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

            if exceeds_vacation_budget(end_post_total, end_base_remaining_after_expiry) {
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
    let year = match query.year {
        Some(value) => value,
        None => crate::settings::app_current_year(&app_state.pool).await,
    };
    let repo_user = app_state
        .db
        .users
        .find_by_id(target_user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let target_user = crate::users::repo_user_to_auth_user(repo_user);
    let (year_from, year_to) = year_bounds(year)?;
    let today = crate::settings::app_today(&app_state.pool).await;
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
                taken_days +=
                    workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
            } else if clamped_start > today {
                // Absence is entirely in the future.
                upcoming_days +=
                    workdays(&app_state.pool, target_user.id, clamped_start, clamped_end).await?;
            } else {
                // Absence spans today: count today as already taken and only keep
                // days strictly after today in the upcoming bucket.
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

    // -- Carryover policy context for this year --
    let expiry_setting =
        crate::settings::load_setting(&app_state.pool, "carryover_expiry_date", "03-31").await?;
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

    // Total available is an annual frame value (entitlement + active carryover),
    // then reduced by taken/upcoming/requested days. It is intentionally not a
    // date-window-specific "bookable after expiry" value.
    let total_entitlement =
        total_entitlement_with_carryover(effective_entitlement, carryover_days, carryover_expired);
    let available = if carryover_expired {
        if let Some(expiry) = expiry_date {
            let reserved_ranges: Vec<(NaiveDate, NaiveDate)> = vacation_absences
                .iter()
                .map(|absence| (absence.start_date, absence.end_date))
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
