use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};
use std::collections::HashSet;

#[derive(FromRow, Serialize, Clone)]
pub struct Absence {
    pub id: i64,
    pub user_id: i64,
    pub kind: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub half_day: bool,
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
    half_day: bool,
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
    if half_day && from == to && count == 1.0 {
        count = 0.5;
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
    let r: Vec<(NaiveDate, NaiveDate, bool)> = sqlx::query_as(
        "SELECT start_date, end_date, half_day FROM absences WHERE user_id=$1 AND kind=$2 AND status='approved' AND end_date >= $3 AND start_date <= $4"
    ).bind(user_id).bind(kind).bind(from).bind(to).fetch_all(pool).await?;
    let mut total = 0.0;
    for (s, e, h) in r {
        let s2 = std::cmp::max(s, from);
        let e2 = std::cmp::min(e, to);
        total += workdays(pool, s2, e2, h).await?;
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
        "SELECT * FROM absences WHERE user_id=$1 AND end_date >= $2 AND start_date <= $3 ORDER BY start_date DESC"
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
    let mut builder = QueryBuilder::<Postgres>::new("SELECT * FROM absences WHERE TRUE");
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
    pub half_day: bool,
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
        "SELECT a.id, a.user_id, u.first_name, u.last_name, a.kind, a.start_date, a.end_date, a.half_day, a.comment, a.status FROM absences a JOIN users u ON u.id=a.user_id WHERE a.status IN ('requested','approved') AND a.end_date >= $1 AND a.start_date <= $2 ORDER BY a.start_date"
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
            "start_date": e.start_date, "end_date": e.end_date, "half_day": e.half_day,
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
    pub half_day: Option<bool>,
    pub comment: Option<String>,
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    if ![
        "vacation",
        "sick",
        "training",
        "special_leave",
        "unpaid",
        "general_absence",
    ]
    .contains(&b.kind.as_str())
    {
        return Err(AppError::BadRequest("Invalid kind".into()));
    }
    if let Some(c) = &b.comment {
        if c.len() > 2000 {
            return Err(AppError::BadRequest("Comment too long (max 2000).".into()));
        }
    }
    // Reject absurdly long ranges to bound work in `workdays_total`.
    if (b.end_date - b.start_date).num_days() > 366 {
        return Err(AppError::BadRequest(
            "Absence range exceeds one year.".into(),
        ));
    }
    if b.end_date < b.start_date {
        return Err(AppError::BadRequest(
            "end_date must be >= start_date.".into(),
        ));
    }
    let overlap: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM absences WHERE user_id=$1 AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
    ).bind(u.id).bind(b.start_date).bind(b.end_date).fetch_one(&s.pool).await?;
    if overlap > 0 {
        return Err(AppError::Conflict("Overlap with existing absence.".into()));
    }

    let half = b.half_day.unwrap_or(false) && b.kind == "vacation" && b.start_date == b.end_date;
    let status = if b.kind == "sick" {
        "approved"
    } else {
        "requested"
    };
    let id: i64 = sqlx::query_scalar("INSERT INTO absences(user_id, kind, start_date, end_date, half_day, comment, status) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
        .bind(u.id).bind(&b.kind).bind(b.start_date).bind(b.end_date).bind(half).bind(&b.comment).bind(status)
        .fetch_one(&s.pool).await?;
    let a: Absence = sqlx::query_as("SELECT * FROM absences WHERE id=$1")
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
    let prev: Absence = sqlx::query_as("SELECT * FROM absences WHERE id=$1")
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
    if b.end_date < b.start_date {
        return Err(AppError::BadRequest(
            "end_date must be >= start_date.".into(),
        ));
    }
    // Re-check overlap with *other* absences of the same user.
    let overlap: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM absences WHERE id != $1 AND user_id=$2 AND status IN ('requested','approved') AND end_date >= $3 AND start_date <= $4",
    )
    .bind(id).bind(u.id).bind(b.start_date).bind(b.end_date)
    .fetch_one(&s.pool).await?;
    if overlap > 0 {
        return Err(AppError::Conflict("Overlap with existing absence.".into()));
    }
    sqlx::query(
        "UPDATE absences SET start_date=$1, end_date=$2, half_day=$3, comment=$4 WHERE id=$5",
    )
    .bind(b.start_date)
    .bind(b.end_date)
    .bind(b.half_day.unwrap_or(false))
    .bind(&b.comment)
    .bind(id)
    .execute(&s.pool)
    .await?;
    let next: Absence = sqlx::query_as("SELECT * FROM absences WHERE id=$1")
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
    let a: Absence = sqlx::query_as("SELECT * FROM absences WHERE id=$1")
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
    audit::log(&s.pool, u.id, "cancelled", "absences", id, None, None).await;
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
    let a: Absence = sqlx::query_as("SELECT * FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE absences SET status='approved', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP WHERE id=$2")
        .bind(u.id).bind(id).execute(&s.pool).await?;
    audit::log(&s.pool, u.id, "approved", "absences", id, None, None).await;
    if a.user_id != u.id {
        audit::log(&s.pool, a.user_id, "approved", "absences", id, None, None).await;
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
    let a: Absence = sqlx::query_as("SELECT * FROM absences WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if a.user_id == u.id && !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE absences SET status='rejected', reviewed_by=$1, reviewed_at=CURRENT_TIMESTAMP, rejection_reason=$2 WHERE id=$3")
        .bind(u.id).bind(&b.reason).bind(id).execute(&s.pool).await?;
    audit::log(
        &s.pool,
        u.id,
        "rejected",
        "absences",
        id,
        None,
        Some(serde_json::json!({"reason": b.reason})),
    )
    .await;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Serialize)]
pub struct LeaveBalance {
    pub annual_entitlement: f64,
    pub already_taken: f64,
    pub approved_upcoming: f64,
    pub requested: f64,
    pub available: f64,
}

#[derive(Deserialize)]
pub struct BalanceQuery {
    pub year: Option<i32>,
}

pub async fn balance(
    State(s): State<AppState>,
    u: User,
    Path(uid): Path<i64>,
    Query(q): Query<BalanceQuery>,
) -> AppResult<Json<LeaveBalance>> {
    if u.id != uid && !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let target: crate::auth::User = sqlx::query_as("SELECT * FROM users WHERE id=$1")
        .bind(uid)
        .fetch_one(&s.pool)
        .await?;
    let from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = chrono::Local::now().date_naive();
    let vacations = sqlx::query_as::<_, Absence>(
        "SELECT * FROM absences WHERE user_id=$1 AND kind='vacation' AND status IN ('requested','approved') AND end_date >= $2 AND start_date <= $3"
    ).bind(uid).bind(from).bind(to).fetch_all(&s.pool).await?;
    let mut taken = 0.0;
    let mut upcoming = 0.0;
    let mut requested = 0.0;
    for a in &vacations {
        let s2 = std::cmp::max(a.start_date, from);
        let e2 = std::cmp::min(a.end_date, to);
        let days = workdays(&s.pool, s2, e2, a.half_day).await?;
        if a.status == "approved" {
            if a.end_date < today {
                taken += days;
            } else {
                upcoming += days;
            }
        } else if a.status == "requested" {
            requested += days;
        }
    }
    let entitled = if target.role == "admin" {
        0.0
    } else {
        target.annual_leave_days as f64
    };
    Ok(Json(LeaveBalance {
        annual_entitlement: entitled,
        already_taken: taken,
        approved_upcoming: upcoming,
        requested,
        available: entitled - taken - upcoming - requested,
    }))
}
