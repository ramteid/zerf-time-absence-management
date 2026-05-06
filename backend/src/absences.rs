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
use sqlx::{Executor, FromRow, Postgres, QueryBuilder};
use std::collections::HashSet;

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::warn!(target:"zerf::absences", "load notification language failed: {e}");
            i18n::Language::default()
        }
    }
}

const ALLOWED_ABSENCE_KINDS: &[&str] = &[
    "vacation",
    "sick",
    "training",
    "special_leave",
    "unpaid",
    "general_absence",
];

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
}

async fn holidays_set(
    pool: &crate::db::DatabasePool,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<HashSet<NaiveDate>> {
    let holiday_rows: Vec<(NaiveDate,)> =
        sqlx::query_as("SELECT holiday_date FROM holidays WHERE holiday_date BETWEEN $1 AND $2")
            .bind(from)
            .bind(to)
            .fetch_all(pool)
            .await?;
    Ok(holiday_rows.into_iter().map(|(date,)| date).collect())
}

pub async fn workdays(
    pool: &crate::db::DatabasePool,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    if to < from {
        return Ok(0.0);
    }
    let holiday_dates = holidays_set(pool, from, to).await?;
    let mut count = 0.0;
    let mut current_date = from;
    while current_date <= to {
        let day_of_week = current_date.weekday().num_days_from_monday();
        if day_of_week < 5 && !holiday_dates.contains(&current_date) {
            count += 1.0;
        }
        current_date += Duration::days(1);
    }
    Ok(count)
}

pub async fn workdays_total(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    kind: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    let absence_ranges: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
        "SELECT start_date, end_date FROM absences WHERE user_id=$1 AND kind=$2 AND status='approved' AND end_date >= $3 AND start_date <= $4"
    ).bind(user_id).bind(kind).bind(from).bind(to).fetch_all(pool).await?;
    let mut total = 0.0;
    for (absence_start, absence_end) in absence_ranges {
        let clamped_start = std::cmp::max(absence_start, from);
        let clamped_end = std::cmp::min(absence_end, to);
        total += workdays(pool, clamped_start, clamped_end).await?;
    }
    Ok(total)
}

#[derive(Deserialize)]
pub struct YearQuery {
    pub year: Option<i32>,
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<YearQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    // Default to the current calendar year when no year is specified.
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());
    let from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let absences = sqlx::query_as::<_, Absence>(
        "SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE user_id=$1 AND end_date >= $2 AND start_date <= $3 ORDER BY start_date DESC"
    ).bind(requester.id).bind(from).bind(to).fetch_all(&app_state.pool).await?;
    Ok(Json(absences))
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
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE TRUE");
    // Team leads only see absences from their direct reports; admins see all.
    if !requester.is_admin() {
        builder
            .push(" AND user_id IN (SELECT id FROM users WHERE approver_id = ")
            .push_bind(requester.id)
            .push(" AND role != 'admin')");
    }
    if let Some(from_date) = query.from {
        builder.push(" AND end_date >= ").push_bind(from_date);
    }
    if let Some(to_date) = query.to {
        builder.push(" AND start_date <= ").push_bind(to_date);
    }
    if let Some(status_filter) = query.status {
        builder.push(" AND status = ").push_bind(status_filter);
    }
    builder.push(" ORDER BY start_date DESC");
    Ok(Json(
        builder
            .build_query_as::<Absence>()
            .fetch_all(&app_state.pool)
            .await?,
    ))
}

#[derive(Deserialize)]
pub struct MonthQuery {
    pub month: String,
}

#[derive(Serialize, FromRow)]
pub struct CalendarEntry {
    pub id: i64,
    pub user_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub kind: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub comment: Option<String>,
    pub status: String,
}

async fn calendar_scope_user_ids(
    pool: &crate::db::DatabasePool,
    requester: &User,
) -> AppResult<Option<Vec<i64>>> {
    if requester.is_admin() {
        return Ok(None);
    }

    let mut user_ids = vec![requester.id];

    if requester.is_lead() {
        let mut reports: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM users WHERE active=TRUE AND approver_id=$1 AND role!='admin'",
        )
        .bind(requester.id)
        .fetch_all(pool)
        .await?;
        user_ids.append(&mut reports);
    } else if let Some(approver_id) = requester.approver_id {
        let mut team: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM users WHERE active=TRUE AND (id=$1 OR (approver_id=$1 AND role!='admin'))",
        )
        .bind(approver_id)
        .fetch_all(pool)
        .await?;
        user_ids.append(&mut team);
    }

    user_ids.sort_unstable();
    user_ids.dedup();
    Ok(Some(user_ids))
}

pub async fn calendar(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<MonthQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    // Parse the "YYYY-MM" month string into year and month components.
    let (year_str, month_str) = query.month.split_once('-').ok_or_else(|| AppError::BadRequest("month=YYYY-MM required".into()))?;
    let year: i32 = year_str.parse().map_err(|_| AppError::BadRequest("Invalid year".into()))?;
    let month: u32 = month_str.parse().map_err(|_| AppError::BadRequest("Invalid month".into()))?;
    let from = NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(|| AppError::BadRequest("Invalid date".into()))?;
    // Last day of the month: step to first of next month and subtract one day.
    let next_month_first = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    let to = next_month_first - Duration::days(1);
    // Determine which user IDs this requester is allowed to see (scope restriction).
    let scope_user_ids = calendar_scope_user_ids(&app_state.pool, &requester).await?;
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT a.id, a.user_id, u.first_name, u.last_name, a.kind, a.start_date, a.end_date, a.comment, a.status FROM absences a JOIN users u ON u.id=a.user_id WHERE a.status IN ('requested','approved') AND a.end_date >= ",
    );
    builder.push_bind(from);
    builder.push(" AND a.start_date <= ");
    builder.push_bind(to);
    if let Some(user_ids) = scope_user_ids {
        builder.push(" AND a.user_id IN (");
        let mut separated = builder.separated(", ");
        for user_id in user_ids {
            separated.push_bind(user_id);
        }
        separated.push_unseparated(")");
    }
    builder.push(" ORDER BY a.start_date");
    let calendar_entries = builder
        .build_query_as::<CalendarEntry>()
        .fetch_all(&app_state.pool)
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

fn absence_blocks_logged_time(kind: &str) -> bool {
    kind != "sick"
}

async fn ensure_no_logged_time_conflict<'e, E>(
    executor: E,
    user_id: i64,
    kind: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> AppResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    if !absence_blocks_logged_time(kind) {
        return Ok(());
    }

    let existing_entry_day: Option<NaiveDate> = sqlx::query_scalar(
        "SELECT entry_date FROM time_entries WHERE user_id=$1 AND status <> 'rejected' \
         AND entry_date BETWEEN $2 AND $3 ORDER BY entry_date LIMIT 1",
    )
    .bind(user_id)
    .bind(start_date)
    .bind(end_date)
    .fetch_optional(executor)
    .await?;

    if existing_entry_day.is_some() {
        return Err(AppError::BadRequest(
            "Non-sick absences cannot overlap days with logged time. Please remove or reject the time entries first.".into(),
        ));
    }

    Ok(())
}

async fn lock_absence_scope<'e, E>(executor: E, user_id: i64) -> AppResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(user_id)
        .execute(executor)
        .await?;
    Ok(())
}

async fn absence_owner_id(pool: &crate::db::DatabasePool, absence_id: i64) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT user_id FROM absences WHERE id=$1")
            .bind(absence_id)
            .fetch_one(pool)
            .await?,
    )
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
    // Use an advisory lock on the user_id to serialize absence creation per
    // user, preventing the TOCTOU race where two concurrent requests both pass
    // the overlap check before either insert commits.
    let mut tx = app_state.pool.begin().await?;
    lock_absence_scope(&mut *tx, requester.id).await?;
    let overlap_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM absences WHERE user_id=$1 AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
    ).bind(requester.id).bind(body.start_date).bind(body.end_date).fetch_one(&mut *tx).await?;
    if overlap_count > 0 {
        return Err(AppError::Conflict("Overlap with existing absence.".into()));
    }
    ensure_no_logged_time_conflict(&mut *tx, requester.id, kind, body.start_date, body.end_date).await?;
    // Validate vacation balance: user cannot request more vacation than available.
    if kind == "vacation" {
        validate_vacation_balance(&app_state.pool, &mut *tx, &requester, body.start_date, body.end_date, None).await?;
    }
    // Sick leave is auto-approved only when it has already started (or starts today).
    // Future-dated sick leave requires review like any other request.
    let today_date = chrono::Local::now().date_naive();
    let initial_status = if kind == "sick" && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    let new_absence_id: i64 = sqlx::query_scalar("INSERT INTO absences(user_id, kind, start_date, end_date, comment, status) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(requester.id).bind(kind).bind(body.start_date).bind(body.end_date).bind(&body.comment).bind(initial_status)
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let created_absence: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(new_absence_id)
        .fetch_one(&app_state.pool)
        .await?;
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
        // Notify approvers that a new absence request needs review.
        let requester_full_name = format!("{} {}", requester.first_name, requester.last_name);
        let approver_ids = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
        let language = notification_language(&app_state.pool).await;
        for approver_id in approver_ids {
            crate::notifications::create_translated(
                &app_state,
                &language,
                approver_id,
                "absence_requested",
                "absence_requested_title",
                "absence_requested_body",
                vec![
                    ("requester_name", requester_full_name.clone()),
                    ("start_date", i18n::format_date(&language, created_absence.start_date)),
                    ("end_date", i18n::format_date(&language, created_absence.end_date)),
                ],
                Some("absences"),
                Some(new_absence_id),
            )
            .await;
        }
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
    let current_owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut tx = app_state.pool.begin().await?;
    lock_absence_scope(&mut *tx, current_owner_id).await?;
    let absence_before_update: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;
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
    // Re-check overlap with *other* absences of the same user (under advisory
    // lock to prevent TOCTOU race).
    let overlap_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM absences WHERE id != $1 AND user_id=$2 AND status IN ('requested','approved') AND end_date >= $3 AND start_date <= $4",
    )
    .bind(absence_id).bind(requester.id).bind(body.start_date).bind(body.end_date)
    .fetch_one(&mut *tx).await?;
    if overlap_count > 0 {
        return Err(AppError::Conflict("Overlap with existing absence.".into()));
    }
    ensure_no_logged_time_conflict(&mut *tx, requester.id, kind, body.start_date, body.end_date).await?;
    // Validate vacation balance (excluding the current absence being edited).
    if kind == "vacation" {
        validate_vacation_balance(&app_state.pool, &mut *tx, &requester, body.start_date, body.end_date, Some(absence_id)).await?;
    }
    // Sick leave already started today is auto-approved; future-dated requires review.
    let today_date = chrono::Local::now().date_naive();
    let updated_status = if kind == "sick" && body.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    sqlx::query(
        "UPDATE absences SET kind=$1, start_date=$2, end_date=$3, comment=$4, status=$5, reviewed_by=NULL, reviewed_at=NULL, rejection_reason=NULL WHERE id=$6",
    )
    .bind(kind)
    .bind(body.start_date)
    .bind(body.end_date)
    .bind(&body.comment)
    .bind(updated_status)
    .bind(absence_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let absence_after_update: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(absence_id)
        .fetch_one(&app_state.pool)
        .await?;
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
    Ok(Json(absence_after_update))
}

fn can_self_cancel(absence: &Absence) -> bool {
    absence.status == "requested"
        || (absence.kind == "sick" && absence.status == "approved" && absence.reviewed_by.is_none())
}

pub async fn cancel(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut tx = app_state.pool.begin().await?;
    lock_absence_scope(&mut *tx, owner_id).await?;
    // Lock the absence row to prevent concurrent cancellations.
    let absence: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;
    if absence.user_id != requester.id {
        return Err(AppError::Forbidden);
    }
    if !can_self_cancel(&absence) {
        return Err(AppError::BadRequest(
            "Only requested or auto-approved sick absences can be self-cancelled.".into(),
        ));
    }
    sqlx::query("UPDATE absences SET status='cancelled' WHERE id=$1")
        .bind(absence_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
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
    Ok(Json(serde_json::json!({"ok":true})))
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
    let mut tx = app_state.pool.begin().await?;
    lock_absence_scope(&mut *tx, owner_id).await?;
    // Lock the absence row to prevent concurrent approvals.
    let absence: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;
    // A lead may not approve their own absence; admins may.
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
    if !requester.is_admin() {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(absence.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    if absence.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be approved.".into(),
        ));
    }
    ensure_no_logged_time_conflict(&mut *tx, absence.user_id, &absence.kind, absence.start_date, absence.end_date).await?;
    // Use optimistic locking: check that status is still 'requested' in the UPDATE.
    let rows_updated = sqlx::query(
        "UPDATE absences SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='requested'",
    )
    .bind(requester.id)
    .bind(absence_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    tx.commit().await?;
    let before_json = serde_json::to_value(&absence).unwrap();
    let after_json = serde_json::json!({"status": "approved", "reviewed_by": requester.id});
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "absences",
        absence_id,
        Some(before_json.clone()),
        Some(after_json.clone()),
    )
    .await;
    if absence.user_id != requester.id {
        // Also record in the absence owner's audit trail.
        audit::log(
            &app_state.pool,
            absence.user_id,
            "approved",
            "absences",
            absence_id,
            Some(before_json),
            Some(after_json),
        )
        .await;
        // Notify the absence owner that their absence was approved.
        let language = notification_language(&app_state.pool).await;
        crate::notifications::create_translated(
            &app_state,
            &language,
            absence.user_id,
            "absence_approved",
            "absence_approved_title",
            "absence_approved_body",
            vec![
                ("start_date", i18n::format_date(&language, absence.start_date)),
                ("end_date", i18n::format_date(&language, absence.end_date)),
            ],
            Some("absences"),
            Some(absence_id),
        )
        .await;
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
    let owner_id = absence_owner_id(&app_state.pool, absence_id).await?;
    let mut tx = app_state.pool.begin().await?;
    lock_absence_scope(&mut *tx, owner_id).await?;
    // Lock the absence row to prevent concurrent rejections.
    let absence: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;
    // A lead may not reject their own absence; admins may.
    if absence.user_id == requester.id && !requester.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
    if !requester.is_admin() {
        let is_direct_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin' FOR UPDATE",
        )
        .bind(absence.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_direct_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    if absence.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be rejected.".into(),
        ));
    }
    // Use optimistic locking: check that status is still 'requested' in the UPDATE.
    let rows_updated = sqlx::query(
        "UPDATE absences SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3 AND status='requested'",
    )
    .bind(requester.id)
    .bind(&body.reason)
    .bind(absence_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows_updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    tx.commit().await?;
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
    if absence.user_id != requester.id {
        // Notify the absence owner that their absence was rejected.
        let language = notification_language(&app_state.pool).await;
        crate::notifications::create_translated(
            &app_state,
            &language,
            absence.user_id,
            "absence_rejected",
            "absence_rejected_title",
            "absence_rejected_body",
            vec![
                ("start_date", i18n::format_date(&language, absence.start_date)),
                ("end_date", i18n::format_date(&language, absence.end_date)),
                ("reason", body.reason.clone()),
            ],
            Some("absences"),
            Some(absence_id),
        )
        .await;
    }
    Ok(Json(serde_json::json!({"ok":true})))
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
    let mut tx = app_state.pool.begin().await?;
    lock_absence_scope(&mut *tx, owner_id).await?;
    // Lock the absence row to prevent concurrent revocations.
    let absence: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(absence_id)
        .fetch_one(&mut *tx)
        .await?;
    if absence.status != "approved" {
        return Err(AppError::BadRequest(
            "Only approved absences can be revoked.".into(),
        ));
    }
    sqlx::query("UPDATE absences SET status='cancelled', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2")
        .bind(requester.id)
        .bind(absence_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
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
        crate::notifications::create_translated(
            &app_state,
            &language,
            absence.user_id,
            "absence_revoked",
            "absence_revoked_title",
            "absence_revoked_body",
            vec![
                ("start_date", i18n::format_date(&language, absence.start_date)),
                ("end_date", i18n::format_date(&language, absence.end_date)),
            ],
            Some("absences"),
            Some(absence_id),
        )
        .await;
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
    pool: &crate::db::DatabasePool,
    requester: &User,
    target_uid: i64,
) -> AppResult<()> {
    if requester.id == target_uid || requester.is_admin() {
        return Ok(());
    }
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let is_report: Option<bool> = sqlx::query_scalar(
        "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 AND role != 'admin'",
    )
    .bind(target_uid)
    .bind(requester.id)
    .fetch_optional(pool)
    .await?;
    if is_report.is_none() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Helper: resolve the effective annual leave entitlement for a user in a given year.
/// Checks `user_annual_leave_overrides` first, falls back to `users.annual_leave_days`.
async fn effective_annual_days(
    pool: &crate::db::DatabasePool,
    user: &crate::auth::User,
    year: i32,
) -> AppResult<i64> {
    let override_days: Option<i64> = sqlx::query_scalar(
        "SELECT days FROM user_annual_leave_overrides WHERE user_id=$1 AND year=$2",
    )
    .bind(user.id)
    .bind(year)
    .fetch_optional(pool)
    .await?;
    Ok(override_days.unwrap_or(user.annual_leave_days))
}

/// Parse the carryover expiry date setting (MM-DD) into a NaiveDate for the given year.
fn parse_expiry_date(setting: &str, year: i32) -> Option<NaiveDate> {
    let (month_str, day_str) = setting.split_once('-')?;
    let month: u32 = month_str.parse().ok()?;
    let day: u32 = day_str.parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
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
) -> AppResult<()> {
    let year = start_date.year();
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = chrono::Local::now().date_naive();

    // Resolve effective entitlement for the start year (pro-rated if mid-year start).
    let entitled = effective_annual_days(pool, user, year).await?;
    let effective_entitlement = pro_rate_entitlement(user.start_date, year, entitled);

    // Determine carryover from the previous year: entitlement minus days actually taken.
    let expiry_setting = crate::settings::load_setting(pool, "carryover_expiry_date", "03-31").await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let carryover_expired = expiry_date.map(|d| today > d).unwrap_or(false);
    let prev_year = year - 1;
    let prev_entitled = effective_annual_days(pool, user, prev_year).await?;
    let prev_effective = pro_rate_entitlement(user.start_date, prev_year, prev_entitled);
    let prev_year_start = NaiveDate::from_ymd_opt(prev_year, 1, 1).unwrap();
    let prev_year_end = NaiveDate::from_ymd_opt(prev_year, 12, 31).unwrap();
    let prev_taken = workdays_total(pool, user.id, "vacation", prev_year_start, prev_year_end).await?;
    // Carryover is the unused portion of last year's entitlement (never negative).
    let carryover_days = std::cmp::max(0, prev_effective - prev_taken.ceil() as i64);

    // Total budget = this year's entitlement + unexpired carryover.
    let total_entitlement = if carryover_expired {
        effective_entitlement as f64
    } else {
        effective_entitlement as f64 + carryover_days as f64
    };

    // Sum existing vacation usage (requested + approved) in this year, excluding `exclude_id`.
    let existing_ranges: Vec<(NaiveDate, NaiveDate)> = if let Some(excl) = exclude_id {
        sqlx::query_as(
            "SELECT start_date, end_date FROM absences WHERE id != $1 AND user_id=$2 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $3 AND start_date <= $4"
        ).bind(excl).bind(user.id).bind(year_from).bind(year_to).fetch_all(&mut *tx).await?
    } else {
        sqlx::query_as(
            "SELECT start_date, end_date FROM absences WHERE user_id=$1 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
        ).bind(user.id).bind(year_from).bind(year_to).fetch_all(&mut *tx).await?
    };
    let mut used_days = 0.0;
    for (s, e) in &existing_ranges {
        // Clamp each existing absence to the current year boundary before counting workdays.
        used_days += workdays(pool, std::cmp::max(*s, year_from), std::cmp::min(*e, year_to)).await?;
    }
    // Clamp the new absence to this year and check whether adding it would exceed the budget.
    let new_start = std::cmp::max(start_date, year_from);
    let new_end = std::cmp::min(end_date, year_to);
    let new_days = workdays(pool, new_start, new_end).await?;
    if used_days + new_days > total_entitlement {
        return Err(AppError::BadRequest("Not enough remaining vacation days.".into()));
    }

    // When the absence spans New Year's Day, validate the end year's budget separately.
    // The current year's unused entitlement becomes next year's carryover.
    let end_year = end_date.year();
    if end_year != year {
        let end_year_from = NaiveDate::from_ymd_opt(end_year, 1, 1).unwrap();
        let end_year_to = NaiveDate::from_ymd_opt(end_year, 12, 31).unwrap();

        let end_year_entitled = effective_annual_days(pool, user, end_year).await?;
        let end_year_effective = pro_rate_entitlement(user.start_date, end_year, end_year_entitled);

        // Carryover into the end year = current year's entitlement minus current year's usage.
        let end_year_expiry_date = parse_expiry_date(&expiry_setting, end_year);
        let end_year_carryover_expired = end_year_expiry_date.map(|d| today > d).unwrap_or(false);
        let current_year_taken = workdays_total(pool, user.id, "vacation", year_from, year_to).await?;
        let current_year_carryover = std::cmp::max(0, effective_entitlement - current_year_taken.ceil() as i64);
        let end_year_total = if end_year_carryover_expired {
            end_year_effective as f64
        } else {
            end_year_effective as f64 + current_year_carryover as f64
        };

        let end_year_existing: Vec<(NaiveDate, NaiveDate)> = if let Some(excl) = exclude_id {
            sqlx::query_as(
                "SELECT start_date, end_date FROM absences WHERE id != $1 AND user_id=$2 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $3 AND start_date <= $4"
            ).bind(excl).bind(user.id).bind(end_year_from).bind(end_year_to).fetch_all(&mut *tx).await?
        } else {
            sqlx::query_as(
                "SELECT start_date, end_date FROM absences WHERE user_id=$1 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
            ).bind(user.id).bind(end_year_from).bind(end_year_to).fetch_all(&mut *tx).await?
        };
        let mut end_year_used = 0.0;
        for (s, e) in &end_year_existing {
            end_year_used += workdays(pool, std::cmp::max(*s, end_year_from), std::cmp::min(*e, end_year_to)).await?;
        }
        let end_new_start = std::cmp::max(start_date, end_year_from);
        let end_new_end = std::cmp::min(end_date, end_year_to);
        let end_new_days = workdays(pool, end_new_start, end_new_end).await?;
        if end_year_used + end_new_days > end_year_total {
            return Err(AppError::BadRequest("Not enough remaining vacation days.".into()));
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
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    // Default to the current year if none was provided.
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());
    let target_user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(target_user_id)
        .fetch_one(&app_state.pool)
        .await?;
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = chrono::Local::now().date_naive();
    // Load all vacation absences (requested + approved) in the given year.
    let vacation_absences = sqlx::query_as::<_, Absence>(
        "SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE user_id=$1 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
    ).bind(target_user_id).bind(year_from).bind(year_to).fetch_all(&app_state.pool).await?;
    // Categorize each vacation absence into taken, upcoming, or requested buckets.
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
                // Absence spans today: split into taken and upcoming.
                let yesterday = today - Duration::days(1);
                taken_days += workdays(&app_state.pool, clamped_start, yesterday).await?;
                upcoming_days += workdays(&app_state.pool, today, clamped_end).await?;
            }
        } else if absence.status == "requested" {
            requested_days += workdays(&app_state.pool, clamped_start, clamped_end).await?;
        }
    }

    // Resolve per-year entitlement (override or default), pro-rated for mid-year starts.
    let entitled = effective_annual_days(&app_state.pool, &target_user, year).await?;
    let effective_entitlement = pro_rate_entitlement(target_user.start_date, year, entitled);

    // -- Carryover from previous year --
    let expiry_setting =
        crate::settings::load_setting(&app_state.pool, "carryover_expiry_date", "03-31").await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let carryover_expired = expiry_date.map(|d| today > d).unwrap_or(false);

    // Previous year entitlement minus previous year's actually-taken vacation days.
    let prev_year = year - 1;
    let prev_year_entitled = effective_annual_days(&app_state.pool, &target_user, prev_year).await?;
    let prev_year_effective = pro_rate_entitlement(target_user.start_date, prev_year, prev_year_entitled);
    let prev_year_start = NaiveDate::from_ymd_opt(prev_year, 1, 1).unwrap();
    let prev_year_end = NaiveDate::from_ymd_opt(prev_year, 12, 31).unwrap();
    let prev_year_taken =
        workdays_total(&app_state.pool, target_user_id, "vacation", prev_year_start, prev_year_end).await?;
    let carryover_days = std::cmp::max(0, prev_year_effective - prev_year_taken.ceil() as i64);

    // Calculate how much of the carryover has been consumed this year.
    // Carryover is consumed first: vacation taken this year eats into carryover
    // before the current year's entitlement. But only if carryover hasn't expired.
    // If expired, carryover_remaining = 0.
    let carryover_remaining = if carryover_expired || carryover_days == 0 {
        0.0
    } else {
        // Vacation taken this year (approved, already past) consumes carryover first.
        // Must be taken (not just requested) before expiry to count.
        let taken_before_expiry = if let Some(expiry) = expiry_date {
            // Count approved vacation days taken within [jan1, expiry_date].
            let cutoff = std::cmp::min(expiry, today);
            if cutoff >= year_from {
                let mut sum = 0.0;
                for absence in &vacation_absences {
                    if absence.status != "approved" {
                        continue;
                    }
                    let clamped_start = std::cmp::max(absence.start_date, year_from);
                    let clamped_end = std::cmp::min(absence.end_date, cutoff);
                    if clamped_end >= clamped_start {
                        sum += workdays(&app_state.pool, clamped_start, clamped_end).await?;
                    }
                }
                sum
            } else {
                0.0
            }
        } else {
            // No expiry date configured: all taken days consume carryover.
            taken_days
        };
        (carryover_days as f64 - taken_before_expiry).max(0.0)
    };

    // Total available = current year entitlement + active carryover - all used/pending.
    let total_entitlement = if carryover_expired {
        effective_entitlement as f64
    } else {
        effective_entitlement as f64 + carryover_days as f64
    };
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
