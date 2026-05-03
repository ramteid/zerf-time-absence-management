use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::header,
    response::Response,
    Json,
};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct MonthQuery {
    pub user_id: Option<i64>,
    pub month: String,
}

fn month_bounds(m: &str) -> AppResult<(NaiveDate, NaiveDate)> {
    let parts: Vec<&str> = m.split('-').collect();
    if parts.len() != 2 {
        return Err(AppError::BadRequest("month=YYYY-MM".into()));
    }
    let y: i32 = parts[0]
        .parse()
        .map_err(|_| AppError::BadRequest("year".into()))?;
    let mo: u32 = parts[1]
        .parse()
        .map_err(|_| AppError::BadRequest("month".into()))?;
    let from =
        NaiveDate::from_ymd_opt(y, mo, 1).ok_or_else(|| AppError::BadRequest("date".into()))?;
    let to = if mo == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(y, mo + 1, 1).unwrap()
    } - Duration::days(1);
    Ok((from, to))
}

#[derive(Serialize)]
pub struct DayDetail {
    pub date: NaiveDate,
    pub weekday: String,
    pub entries: Vec<EntryDetail>,
    pub actual_min: i64,
    pub target_min: i64,
    pub absence: Option<String>,
    pub holiday: Option<String>,
}

#[derive(Serialize)]
pub struct EntryDetail {
    pub start_time: String,
    pub end_time: String,
    pub category: String,
    pub color: String,
    pub minutes: i64,
    pub status: String,
    pub comment: Option<String>,
}

#[derive(Serialize)]
pub struct MonthReport {
    pub user_id: i64,
    pub month: String,
    pub days: Vec<DayDetail>,
    pub target_min: i64,
    pub actual_min: i64,
    pub diff_min: i64,
    pub category_totals: HashMap<String, i64>,
}

fn weekday_en(d: NaiveDate) -> &'static str {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ][d.weekday().num_days_from_monday() as usize]
}

async fn build_range(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
    label: &str,
) -> AppResult<MonthReport> {
    let user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;

    #[allow(clippy::type_complexity)]
    let te: Vec<(NaiveDate, String, String, String, String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT z.entry_date, z.start_time, z.end_time, c.name, c.color, z.category_id, z.status, z.comment FROM time_entries z JOIN categories c ON c.id=z.category_id WHERE z.user_id=$1 AND z.entry_date BETWEEN $2 AND $3 ORDER BY z.entry_date, z.start_time"
    ).bind(user_id).bind(from).bind(to).fetch_all(pool).await?;

    let abs: Vec<(NaiveDate, NaiveDate, String)> = sqlx::query_as(
        "SELECT start_date, end_date, kind FROM absences WHERE user_id=$1 AND status='approved' AND end_date >= $2 AND start_date <= $3"
    ).bind(user_id).bind(from).bind(to).fetch_all(pool).await?;

    // Load UI language to decide which holiday name to display
    let ui_lang: String =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'ui_language'")
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| "en".to_string());

    let h: Vec<(NaiveDate, String, Option<String>)> = sqlx::query_as(
        "SELECT holiday_date, name, local_name FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let h: Vec<(NaiveDate, String)> = h
        .into_iter()
        .map(|(d, name, local_name)| {
            let display = if ui_lang != "en" {
                local_name.unwrap_or(name)
            } else {
                name
            };
            (d, display)
        })
        .collect();
    let h_map: HashMap<NaiveDate, String> = h.into_iter().collect();

    let mut days: Vec<DayDetail> = vec![];
    let mut target_total = 0i64;
    let mut actual_total = 0i64;
    let mut cat: HashMap<String, i64> = HashMap::new();
    let mut d = from;
    let is_admin = user.role == "admin";
    while d <= to {
        let wd = d.weekday().num_days_from_monday();
        let weekday = wd < 5;
        let holiday = h_map.get(&d).cloned();
        let absence = abs
            .iter()
            .find(|(s, e, _)| d >= *s && d <= *e)
            .map(|(_, _, k)| k.clone());
        let before_start = d < user.start_date;
        let target = if weekday && holiday.is_none() && !before_start && !is_admin {
            target_per_day_min
        } else {
            0
        };
        let mut entries: Vec<EntryDetail> = vec![];
        let mut actual = 0i64;
        for (dd, b, e, cn, cf, _kid, st, km) in &te {
            if *dd != d {
                continue;
            }
            // Defensive: never panic on malformed time data — surface a 500 with
            // a generic message instead. The DB schema does not constrain the
            // text format, so a corrupted row must not take the process down.
            let bn = parse_report_time(b)?;
            let en = parse_report_time(e)?;
            let m = (en - bn).num_minutes();
            if st == "approved" {
                actual += m;
                *cat.entry(cn.clone()).or_insert(0) += m;
            }
            entries.push(EntryDetail {
                start_time: b.clone(),
                end_time: e.clone(),
                category: cn.clone(),
                color: cf.clone(),
                minutes: m,
                status: st.clone(),
                comment: km.clone(),
            });
        }
        let actual_eff = if absence.is_some() { target } else { actual };
        target_total += target;
        actual_total += actual_eff;
        days.push(DayDetail {
            date: d,
            weekday: weekday_en(d).to_string(),
            entries,
            actual_min: actual_eff,
            target_min: target,
            absence,
            holiday,
        });
        d += Duration::days(1);
    }
    Ok(MonthReport {
        user_id,
        month: label.into(),
        days,
        target_min: target_total,
        actual_min: actual_total,
        diff_min: actual_total - target_total,
        category_totals: cat,
    })
}

async fn build_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month: &str,
) -> AppResult<MonthReport> {
    let (from, to) = month_bounds(month)?;
    build_range(pool, user_id, from, to, month).await
}

pub async fn month(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<MonthQuery>,
) -> AppResult<Json<MonthReport>> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(build_month(&s.pool, uid, &q.month).await?))
}

#[derive(Deserialize)]
pub struct CsvQuery {
    pub user_id: Option<i64>,
    pub month: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

fn validate_range(from: NaiveDate, to: NaiveDate) -> AppResult<()> {
    if from > to {
        return Err(AppError::BadRequest("from must not be after to.".into()));
    }
    if (to - from).num_days() > 365 {
        return Err(AppError::BadRequest(
            "Date range must not exceed 366 days.".into(),
        ));
    }
    Ok(())
}

fn csv_response(r: MonthReport, uid: i64, file_label: &str) -> AppResult<Response> {
    // CSV formula-injection guard: prefix any cell that begins with =, +, -, @ or
    // a tab/CR with a leading single-quote so spreadsheets treat it as text.
    fn safe(s: &str) -> String {
        if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            format!("'{}", s)
        } else {
            s.to_string()
        }
    }
    fn csv_err(error: csv::Error) -> AppError {
        tracing::error!(target: "kitazeit::reports", "CSV export failed: {error}");
        AppError::Internal("CSV export failed.".into())
    }
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "Date", "Weekday", "Start", "End", "Category", "Minutes", "Status", "Comment", "Absence",
        "Holiday",
    ])
    .map_err(csv_err)?;
    for t in &r.days {
        if t.entries.is_empty() {
            wtr.write_record([
                t.date.to_string(),
                t.weekday.clone(),
                "".into(),
                "".into(),
                "".into(),
                "0".into(),
                "".into(),
                "".into(),
                safe(&t.absence.clone().unwrap_or_default()),
                safe(&t.holiday.clone().unwrap_or_default()),
            ])
            .map_err(csv_err)?;
        } else {
            for e in &t.entries {
                wtr.write_record([
                    t.date.to_string(),
                    t.weekday.clone(),
                    e.start_time.clone(),
                    e.end_time.clone(),
                    safe(&e.category),
                    e.minutes.to_string(),
                    e.status.clone(),
                    safe(&e.comment.clone().unwrap_or_default()),
                    safe(&t.absence.clone().unwrap_or_default()),
                    safe(&t.holiday.clone().unwrap_or_default()),
                ])
                .map_err(csv_err)?;
            }
        }
    }
    wtr.write_record([
        "",
        "Total",
        "",
        "",
        "",
        &r.actual_min.to_string(),
        "",
        "",
        "",
        "",
    ])
    .map_err(csv_err)?;
    let data = wtr.into_inner().map_err(|error| {
        tracing::error!(target: "kitazeit::reports", "CSV export finalize failed: {error}");
        AppError::Internal("CSV export failed.".into())
    })?;
    let mut resp = Response::new(axum::body::Body::from(data));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        "text/csv; charset=utf-8".parse().unwrap(),
    );
    let safe_label: String = file_label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(30)
        .collect();
    let cd = format!(
        "attachment; filename=\"report-user-{}-{}.csv\"",
        uid, safe_label
    );
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&cd).unwrap_or_else(|_| {
            axum::http::HeaderValue::from_static("attachment; filename=\"report.csv\"")
        }),
    );
    Ok(resp)
}

pub async fn month_csv(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<CsvQuery>,
) -> AppResult<Response> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let month = q
        .month
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("month=YYYY-MM".into()))?;
    let r = build_month(&s.pool, uid, month).await?;
    csv_response(r, uid, month)
}

pub async fn range_csv(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<CsvQuery>,
) -> AppResult<Response> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let from = q
        .from
        .ok_or_else(|| AppError::BadRequest("from is required.".into()))?;
    let to =
        q.to.ok_or_else(|| AppError::BadRequest("to is required.".into()))?;
    validate_range(from, to)?;
    let label = format!("{}_to_{}", from, to);
    let r = build_range(&s.pool, uid, from, to, &label).await?;
    csv_response(r, uid, &label)
}

#[derive(Serialize)]
pub struct TeamRow {
    pub user_id: i64,
    pub name: String,
    pub target_min: i64,
    pub actual_min: i64,
    pub diff_min: i64,
    pub vacation_days: f64,
    pub sick_days: f64,
}

#[derive(Deserialize)]
pub struct TeamQuery {
    pub month: String,
}

pub async fn team(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<TeamQuery>,
) -> AppResult<Json<Vec<TeamRow>>> {
    if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let users: Vec<crate::auth::User> =
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE active=TRUE ORDER BY last_name")
            .fetch_all(&s.pool)
            .await?;
    let mut out = vec![];
    let (from, to) = month_bounds(&q.month)?;
    for usr in users {
        let r = build_month(&s.pool, usr.id, &q.month).await?;
        let vac = crate::absences::workdays_total(&s.pool, usr.id, "vacation", from, to).await?;
        let sick = crate::absences::workdays_total(&s.pool, usr.id, "sick", from, to).await?;
        out.push(TeamRow {
            user_id: usr.id,
            name: format!("{} {}", usr.first_name, usr.last_name),
            target_min: r.target_min,
            actual_min: r.actual_min,
            diff_min: r.diff_min,
            vacation_days: vac,
            sick_days: sick,
        });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CategoryQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub user_id: Option<i64>,
}

#[derive(Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub color: String,
    pub minutes: i64,
}

fn parse_report_time(raw: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(raw, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(raw, "%H:%M:%S"))
        .map_err(|_| AppError::Internal("Invalid time value stored in database.".into()))
}

pub async fn categories(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<CategoryQuery>,
) -> AppResult<Json<Vec<CategoryTotal>>> {
    let uid = q.user_id;
    if let Some(id) = uid {
        if id != u.id && !u.is_lead() {
            return Err(AppError::Forbidden);
        }
    } else if !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT c.name, c.color, z.start_time, z.end_time \
         FROM time_entries z \
         JOIN categories c ON c.id=z.category_id \
         WHERE z.status IN ('draft','submitted','approved') AND z.entry_date BETWEEN ",
    );
    builder.push_bind(q.from).push(" AND ").push_bind(q.to);
    if let Some(id) = uid {
        builder.push(" AND z.user_id = ").push_bind(id);
    }
    let rows: Vec<(String, String, String, String)> =
        builder.build_query_as().fetch_all(&s.pool).await?;
    let mut totals: HashMap<(String, String), i64> = HashMap::new();
    for (category, color, start_time, end_time) in rows {
        let minutes =
            (parse_report_time(&end_time)? - parse_report_time(&start_time)?).num_minutes();
        *totals.entry((category, color)).or_insert(0) += minutes;
    }
    let mut out: Vec<CategoryTotal> = totals
        .into_iter()
        .map(|((category, color), minutes)| CategoryTotal {
            category,
            color,
            minutes,
        })
        .collect();
    out.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.category.cmp(&b.category))
    });
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct OvertimeQuery {
    pub user_id: Option<i64>,
    pub year: Option<i32>,
}

#[derive(Serialize)]
pub struct MonthRow {
    pub month: String,
    pub target_min: i64,
    pub actual_min: i64,
    pub diff_min: i64,
    pub cumulative_min: i64,
}

pub async fn overtime(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<OvertimeQuery>,
) -> AppResult<Json<Vec<MonthRow>>> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let mut out = vec![];
    let mut cum = 0i64;
    for m in 1..=12u32 {
        let mstr = format!("{:04}-{:02}", year, m);
        let r = build_month(&s.pool, uid, &mstr).await?;
        cum += r.diff_min;
        out.push(MonthRow {
            month: mstr,
            target_min: r.target_min,
            actual_min: r.actual_min,
            diff_min: r.diff_min,
            cumulative_min: cum,
        });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct FlextimeQuery {
    pub user_id: Option<i64>,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

#[derive(Serialize)]
pub struct FlextimeDay {
    pub date: NaiveDate,
    pub actual_min: i64,
    pub target_min: i64,
    pub diff_min: i64,
    pub cumulative_min: i64,
    pub absence: Option<String>,
    pub holiday: Option<String>,
}

pub async fn flextime(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<FlextimeQuery>,
) -> AppResult<Json<Vec<FlextimeDay>>> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() {
        return Err(AppError::Forbidden);
    }
    if q.from > q.to {
        return Err(AppError::BadRequest("from must not be after to.".into()));
    }
    if (q.to - q.from).num_days() > 366 {
        return Err(AppError::BadRequest(
            "Date range must not exceed 366 days.".into(),
        ));
    }

    let user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval FROM users WHERE id=$1")
        .bind(uid)
        .fetch_one(&s.pool)
        .await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;
    let is_admin = user.role == "admin";

    // Start accumulating from the user's first day so the running balance at
    // q.from already reflects all prior over/under-time.
    let loop_start = user.start_date.min(q.from);

    let te: Vec<(NaiveDate, String, String, String)> = sqlx::query_as(
        "SELECT entry_date, start_time, end_time, status \
         FROM time_entries WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3",
    )
    .bind(uid)
    .bind(loop_start)
    .bind(q.to)
    .fetch_all(&s.pool)
    .await?;

    let abs: Vec<(NaiveDate, NaiveDate, String)> = sqlx::query_as(
        "SELECT start_date, end_date, kind FROM absences \
         WHERE user_id=$1 AND status='approved' AND end_date >= $2 AND start_date <= $3",
    )
    .bind(uid)
    .bind(loop_start)
    .bind(q.to)
    .fetch_all(&s.pool)
    .await?;

    let ui_lang: String =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'ui_language'")
            .fetch_optional(&s.pool)
            .await?
            .unwrap_or_else(|| "en".to_string());

    let h: Vec<(NaiveDate, String, Option<String>)> = sqlx::query_as(
        "SELECT holiday_date, name, local_name FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
    )
    .bind(loop_start)
    .bind(q.to)
    .fetch_all(&s.pool)
    .await?;
    let h_map: HashMap<NaiveDate, String> = h
        .into_iter()
        .map(|(d, name, local_name)| {
            let display = if ui_lang != "en" {
                local_name.unwrap_or(name)
            } else {
                name
            };
            (d, display)
        })
        .collect();

    let mut out = vec![];
    let mut cum = 0i64;
    let mut d = loop_start;
    while d <= q.to {
        let wd = d.weekday().num_days_from_monday();
        let weekday = wd < 5;
        let holiday = h_map.get(&d).cloned();
        let absence = abs
            .iter()
            .find(|(s, e, _)| d >= *s && d <= *e)
            .map(|(_, _, k)| k.clone());
        let before_start = d < user.start_date;
        let target = if weekday && holiday.is_none() && !before_start && !is_admin {
            target_per_day_min
        } else {
            0
        };
        let mut actual = 0i64;
        for (dd, b, e, st) in &te {
            if *dd != d {
                continue;
            }
            if st == "approved" {
                let bn = parse_report_time(b)?;
                let en = parse_report_time(e)?;
                actual += (en - bn).num_minutes();
            }
        }
        let actual_eff = if absence.is_some() { target } else { actual };
        let diff = actual_eff - target;
        cum += diff;
        // Only emit days within the requested display range
        if d >= q.from {
            out.push(FlextimeDay {
                date: d,
                actual_min: actual_eff,
                target_min: target,
                diff_min: diff,
                cumulative_min: cum,
                absence,
                holiday,
            });
        }
        d += Duration::days(1);
    }
    Ok(Json(out))
}
