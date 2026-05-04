use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n::{self, TextKey};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize, Serializer};
use sqlx::{FromRow, Postgres, QueryBuilder};
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
    let r: Vec<(NaiveDate,)> =
        sqlx::query_as("SELECT holiday_date FROM holidays WHERE holiday_date BETWEEN $1 AND $2")
            .bind(from)
            .bind(to)
            .fetch_all(pool)
            .await?;
    Ok(r.into_iter().map(|(d,)| d).collect())
}

pub async fn workdays(
    pool: &crate::db::DatabasePool,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    if to < from {
        return Ok(0.0);
    }
    let h = holidays_set(pool, from, to).await?;
    let mut count = 0.0;
    let mut d = from;
    while d <= to {
        let wd = d.weekday().num_days_from_monday();
        if wd < 5 && !h.contains(&d) {
            count += 1.0;
        }
        d += Duration::days(1);
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
    let r: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
        "SELECT start_date, end_date FROM absences WHERE user_id=$1 AND kind=$2 AND status='approved' AND end_date >= $3 AND start_date <= $4"
    ).bind(user_id).bind(kind).bind(from).bind(to).fetch_all(pool).await?;
    let mut total = 0.0;
    for (s, e) in r {
        let s2 = std::cmp::max(s, from);
        let e2 = std::cmp::min(e, to);
        total += workdays(pool, s2, e2).await?;
    }
    Ok(total)
}

#[derive(Deserialize)]
pub struct YearQuery {
    pub year: Option<i32>,
}

pub async fn list(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<YearQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let r = sqlx::query_as::<_, Absence>(
        "SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE user_id=$1 AND end_date >= $2 AND start_date <= $3 ORDER BY start_date DESC"
    ).bind(u.id).bind(from).bind(to).fetch_all(&s.pool).await?;
    Ok(Json(r))
}

#[derive(Deserialize)]
pub struct AllQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub status: Option<String>,
}

pub async fn list_all(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<AllQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut builder = QueryBuilder::<Postgres>::new("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE TRUE");
    // Team leads only see absences from their direct reports; admins see all.
    if !u.is_admin() {
        builder.push(" AND user_id IN (SELECT id FROM users WHERE approver_id = ").push_bind(u.id).push(")");
    }
    if let Some(v) = q.from {
        builder.push(" AND end_date >= ").push_bind(v);
    }
    if let Some(v) = q.to {
        builder.push(" AND start_date <= ").push_bind(v);
    }
    if let Some(v) = q.status {
        builder.push(" AND status = ").push_bind(v);
    }
    builder.push(" ORDER BY start_date DESC");
    Ok(Json(
        builder
            .build_query_as::<Absence>()
            .fetch_all(&s.pool)
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

pub async fn calendar(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<MonthQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    let parts: Vec<&str> = q.month.split('-').collect();
    if parts.len() != 2 {
        return Err(AppError::BadRequest("month=YYYY-MM required".into()));
    }
    let year: i32 = parts[0]
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid year".into()))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid month".into()))?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| AppError::BadRequest("Invalid date".into()))?;
    let to = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    } - Duration::days(1);
    let rows = sqlx::query_as::<_, CalendarEntry>(
        "SELECT a.id, a.user_id, u.first_name, u.last_name, a.kind, a.start_date, a.end_date, a.comment, a.status FROM absences a JOIN users u ON u.id=a.user_id WHERE a.status IN ('requested','approved') AND a.end_date >= $1 AND a.start_date <= $2 ORDER BY a.start_date"
    ).bind(from).bind(to).fetch_all(&s.pool).await?;
    let lead = u.is_lead();
    // Privacy: only team leads / admins see the actual absence kind. For peers
    // we collapse to a coarse label so that sensitive categories (sick leave —
    // health data under GDPR Art. 9 — training, special leave, unpaid leave)
    // are not disclosed across the team. Vacation stays visible because it is
    // operationally needed to coordinate cover and is not health-related.
    Ok(Json(rows.into_iter().map(|e| {
        let own = e.user_id == u.id;
        let kind_visible = lead || own || e.kind == "vacation";
        let kind_out = if kind_visible { e.kind.clone() } else { "absent".to_string() };
        serde_json::json!({
            "id": e.id, "user_id": e.user_id, "name": format!("{} {}", e.first_name, e.last_name),
            "kind": kind_out,
            "start_date": e.start_date, "end_date": e.end_date,
            "status": e.status,
            "comment": if lead || own { e.comment.clone() } else { None }
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

struct NormalizedAbsence<'a> {
    kind: &'a str,
}

fn normalize_absence(input: &NewAbsence) -> AppResult<NormalizedAbsence<'_>> {
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

    Ok(NormalizedAbsence { kind: &input.kind })
}


pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let normalized = normalize_absence(&b)?;
    // Reject absences that start before the user's start_date.
    if b.start_date < u.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    // Sick leave may not be backdated more than 30 days on initial creation.
    // Updates to an existing record are not subject to this limit.
    if normalized.kind == "sick" {
        let earliest = chrono::Local::now().date_naive() - Duration::days(30);
        if b.start_date < earliest {
            return Err(AppError::BadRequest(
                "Sick leave cannot be backdated more than 30 days.".into(),
            ));
        }
    }
    // Use an advisory lock on the user_id to serialize absence creation per
    // user, preventing the TOCTOU race where two concurrent requests both pass
    // the overlap check before either insert commits.
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(u.id)
        .execute(&mut *tx)
        .await?;
    let overlap: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM absences WHERE user_id=$1 AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
    ).bind(u.id).bind(b.start_date).bind(b.end_date).fetch_one(&mut *tx).await?;
    if overlap > 0 {
        return Err(AppError::Conflict("Overlap with existing absence.".into()));
    }
    // Sick leave is auto-approved only when it has already started (or starts today).
    // Future-dated sick leave requires review like any other request.
    let today_date = chrono::Local::now().date_naive();
    let status = if normalized.kind == "sick" && b.start_date <= today_date {
        "approved"
    } else {
        "requested"
    };
    let id: i64 = sqlx::query_scalar("INSERT INTO absences(user_id, kind, start_date, end_date, comment, status) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(u.id).bind(normalized.kind).bind(b.start_date).bind(b.end_date).bind(&b.comment).bind(status)
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let a: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "created",
        "absences",
        id,
        None,
        Some(serde_json::to_value(&a).unwrap()),
    )
    .await;
    Ok(Json(a))
}

pub async fn update(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let normalized = normalize_absence(&b)?;
    // Reject absences that start before the user's start_date.
    if b.start_date < u.start_date {
        return Err(AppError::BadRequest(
            "Absence start date is before user start date.".into(),
        ));
    }
    let prev: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if prev.user_id != u.id {
        return Err(AppError::Forbidden);
    }
    let allowed = prev.status == "requested" || (prev.kind == "sick" && prev.status == "approved");
    if !allowed {
        return Err(AppError::BadRequest("Cannot edit.".into()));
    }
    // Sick absences must remain sick: changing kind is never allowed.
    if prev.kind == "sick" && b.kind != "sick" {
        return Err(AppError::BadRequest(
            "Sick absences cannot change type.".into(),
        ));
    }
    if prev.status == "approved" && b.kind != prev.kind {
        return Err(AppError::BadRequest(
            "Approved absences cannot change type.".into(),
        ));
    }
    // Re-check overlap with *other* absences of the same user (under advisory
    // lock to prevent TOCTOU race).
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(u.id)
        .execute(&mut *tx)
        .await?;
    let overlap: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM absences WHERE id != $1 AND user_id=$2 AND status IN ('requested','approved') AND end_date >= $3 AND start_date <= $4",
    )
    .bind(id).bind(u.id).bind(b.start_date).bind(b.end_date)
    .fetch_one(&mut *tx).await?;
    if overlap > 0 {
        return Err(AppError::Conflict("Overlap with existing absence.".into()));
    }
    let (status, reviewed_by, reviewed_at, rejection_reason) = if prev.status == "requested" {
        let today_date = chrono::Local::now().date_naive();
        let new_status = if normalized.kind == "sick" && b.start_date <= today_date {
            "approved"
        } else {
            "requested"
        };
        (new_status, None, None, None)
    } else {
        (
            prev.status.as_str(),
            prev.reviewed_by,
            prev.reviewed_at,
            prev.rejection_reason.clone(),
        )
    };
    sqlx::query(
        "UPDATE absences SET kind=$1, start_date=$2, end_date=$3, comment=$4, status=$5, reviewed_by=$6, reviewed_at=$7, rejection_reason=$8 WHERE id=$9",
    )
    .bind(normalized.kind)
    .bind(b.start_date)
    .bind(b.end_date)
    .bind(&b.comment)
    .bind(status)
    .bind(reviewed_by)
    .bind(reviewed_at)
    .bind(rejection_reason)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let next: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "updated",
        "absences",
        id,
        Some(serde_json::to_value(&prev).unwrap()),
        Some(serde_json::to_value(&next).unwrap()),
    )
    .await;
    Ok(Json(next))
}

pub async fn cancel(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let a: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if a.user_id != u.id {
        return Err(AppError::Forbidden);
    }
    if a.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be cancelled.".into(),
        ));
    }
    sqlx::query("UPDATE absences SET status='cancelled' WHERE id=$1")
        .bind(id)
        .execute(&s.pool)
        .await?;
    audit::log(
        &s.pool,
        u.id,
        "cancelled",
        "absences",
        id,
        Some(serde_json::to_value(&a).unwrap()),
        Some(serde_json::json!({"status": "cancelled"})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn approve(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut tx = s.pool.begin().await?;
    let a: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    // A lead may not approve their own absence; admins may.
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
    if !u.is_admin() {
        let is_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 FOR UPDATE",
        )
        .bind(a.user_id)
        .bind(u.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    if a.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be approved.".into(),
        ));
    }
    let updated = sqlx::query(
        "UPDATE absences SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2 AND status='requested'",
    )
    .bind(u.id)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    tx.commit().await?;
    let before = serde_json::to_value(&a).unwrap();
    let after = serde_json::json!({"status": "approved", "reviewed_by": u.id});
    audit::log(
        &s.pool,
        u.id,
        "approved",
        "absences",
        id,
        Some(before.clone()),
        Some(after.clone()),
    )
    .await;
    if a.user_id != u.id {
        audit::log(
            &s.pool,
            a.user_id,
            "approved",
            "absences",
            id,
            Some(before),
            Some(after),
        )
        .await;
        let language = notification_language(&s.pool).await;
        crate::notifications::create_translated(
            &s,
            language,
            a.user_id,
            "absence_approved",
            TextKey::AbsenceApprovedTitle,
            TextKey::AbsenceApprovedBody,
            vec![
                ("start_date", i18n::format_date(language, a.start_date)),
                ("end_date", i18n::format_date(language, a.end_date)),
            ],
            Some("absences"),
            Some(id),
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
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
    Json(b): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    if b.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    let mut tx = s.pool.begin().await?;
    let a: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    // A lead may not reject their own absence; admins may.
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    // Non-admin leads may only act on absences of their direct reports.
    if !u.is_admin() {
        let is_report: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2 FOR UPDATE",
        )
        .bind(a.user_id)
        .bind(u.id)
        .fetch_optional(&mut *tx)
        .await?;
        if is_report.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    if a.status != "requested" {
        return Err(AppError::BadRequest(
            "Only requested absences can be rejected.".into(),
        ));
    }
    let updated = sqlx::query(
        "UPDATE absences SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3 AND status='requested'",
    )
    .bind(u.id)
    .bind(&b.reason)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::Conflict(
            "Absence was already reviewed by someone else.".into(),
        ));
    }
    tx.commit().await?;
    audit::log(
        &s.pool,
        u.id,
        "rejected",
        "absences",
        id,
        Some(serde_json::to_value(&a).unwrap()),
        Some(serde_json::json!({"status": "rejected", "reason": b.reason})),
    )
    .await;
    if a.user_id != u.id {
        let language = notification_language(&s.pool).await;
        crate::notifications::create_translated(
            &s,
            language,
            a.user_id,
            "absence_rejected",
            TextKey::AbsenceRejectedTitle,
            TextKey::AbsenceRejectedBody,
            vec![
                ("start_date", i18n::format_date(language, a.start_date)),
                ("end_date", i18n::format_date(language, a.end_date)),
                ("reason", b.reason.clone()),
            ],
            Some("absences"),
            Some(id),
        )
        .await;
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

/// Admin-only: revoke an already-approved absence (e.g. mistaken approval).
/// Transitions the absence to 'cancelled' with an audit trail.
pub async fn revoke(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    let a: Absence = sqlx::query_as("SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if a.status != "approved" {
        return Err(AppError::BadRequest(
            "Only approved absences can be revoked.".into(),
        ));
    }
    sqlx::query("UPDATE absences SET status='cancelled', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2")
        .bind(u.id).bind(id).execute(&s.pool).await?;
    audit::log(
        &s.pool,
        u.id,
        "revoked",
        "absences",
        id,
        Some(serde_json::to_value(&a).unwrap()),
        Some(serde_json::json!({"status": "cancelled", "revoked_by": u.id})),
    )
    .await;
    if a.user_id != u.id {
        let language = notification_language(&s.pool).await;
        crate::notifications::create_translated(
            &s,
            language,
            a.user_id,
            "absence_revoked",
            TextKey::AbsenceRevokedTitle,
            TextKey::AbsenceRevokedBody,
            vec![
                ("start_date", i18n::format_date(language, a.start_date)),
                ("end_date", i18n::format_date(language, a.end_date)),
            ],
            Some("absences"),
            Some(id),
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
        "SELECT TRUE FROM users WHERE id = $1 AND approver_id = $2",
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

pub async fn balance(
    State(s): State<AppState>,
    u: User,
    Path(uid): Path<i64>,
    Query(q): Query<BalanceQuery>,
) -> AppResult<Json<LeaveBalance>> {
    assert_can_access_user(&s.pool, &u, uid).await?;
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let target: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(uid)
        .fetch_one(&s.pool)
        .await?;
    let from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = chrono::Local::now().date_naive();
    let vacations = sqlx::query_as::<_, Absence>(
        "SELECT id, user_id, kind, start_date, end_date, comment, status, reviewed_by, reviewed_at, rejection_reason, created_at FROM absences WHERE user_id=$1 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
    ).bind(uid).bind(from).bind(to).fetch_all(&s.pool).await?;
    let mut taken = 0.0;
    let mut upcoming = 0.0;
    let mut requested = 0.0;
    for a in &vacations {
        let s2 = std::cmp::max(a.start_date, from);
        let e2 = std::cmp::min(a.end_date, to);
        if a.status == "approved" {
            // Split at today: the portion in the past counts as taken,
            // the portion from today onward counts as upcoming.
            if e2 < today {
                taken += workdays(&s.pool, s2, e2).await?;
            } else if s2 >= today {
                upcoming += workdays(&s.pool, s2, e2).await?;
            } else {
                let yesterday = today - Duration::days(1);
                taken += workdays(&s.pool, s2, yesterday).await?;
                upcoming += workdays(&s.pool, today, e2).await?;
            }
        } else if a.status == "requested" {
            requested += workdays(&s.pool, s2, e2).await?;
        }
    }
    let entitled = target.annual_leave_days;
    // Pro-rate entitlement for mid-year starts: if the user's start_date is
    // within the queried year, they are only entitled to the fraction of the
    // year they were employed (rounded up to be generous).
    let year_start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let effective_entitlement = if target.start_date > year_start && target.start_date <= year_end {
        // Months remaining (inclusive of start month), standard German pro-rata
        // calculation: full months from start_date.month through December.
        let months_remaining = (13 - target.start_date.month()) as f64;
        ((entitled as f64) * months_remaining / 12.0).ceil() as i64
    } else {
        entitled
    };
    Ok(Json(LeaveBalance {
        annual_entitlement: effective_entitlement,
        already_taken: taken,
        approved_upcoming: upcoming,
        requested,
        available: effective_entitlement as f64 - taken - upcoming - requested,
    }))
}
