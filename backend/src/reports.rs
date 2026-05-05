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

/// Verify that `requester` is allowed to read data for `target_uid`.
/// Admins may access any user. Non-admin leads may only access their direct
/// reports (users whose `approver_id` matches the lead's id). Every user may
/// always access their own data.
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

fn credited_actual_minutes(actual: i64, target: i64, absence: Option<&str>) -> i64 {
    match absence {
        Some("sick") if actual > 0 => actual,
        Some(_) => target,
        None => actual,
    }
}

async fn build_range(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
    label: &str,
) -> AppResult<MonthReport> {
    let user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;
    let today = chrono::Local::now().date_naive();

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
    while d <= to {
        let wd = d.weekday().num_days_from_monday();
        let weekday = wd < 5;
        let holiday = h_map.get(&d).cloned();
        let absence = abs
            .iter()
            .find(|(s, e, _)| d >= *s && d <= *e)
            .map(|(_, _, k)| k.clone());
        let before_start = d < user.start_date;
        let after_today = d > today;
        let target = if weekday && holiday.is_none() && !before_start && !after_today {
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
        let actual_eff = credited_actual_minutes(actual, target, absence.as_deref());
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
    assert_can_access_user(&s.pool, &u, uid).await?;
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
            "Date range must not exceed 365 days.".into(),
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
        tracing::error!(target: "zerf::reports", "CSV export failed: {error}");
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
        tracing::error!(target: "zerf::reports", "CSV export finalize failed: {error}");
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
    assert_can_access_user(&s.pool, &u, uid).await?;
    let month = q
        .month
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("month=YYYY-MM".into()))?;
    let r = build_month(&s.pool, uid, month).await?;
    csv_response(r, uid, month)
}

#[derive(Deserialize)]
pub struct RangeQuery {
    pub user_id: Option<i64>,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

pub async fn range(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<RangeQuery>,
) -> AppResult<Json<MonthReport>> {
    let uid = q.user_id.unwrap_or(u.id);
    assert_can_access_user(&s.pool, &u, uid).await?;
    validate_range(q.from, q.to)?;
    let label = format!("{}_to_{}", q.from, q.to);
    let r = build_range(&s.pool, uid, q.from, q.to, &label).await?;
    Ok(Json(r))
}

pub async fn range_csv(
    State(s): State<AppState>,
    u: User,
    Query(q): Query<CsvQuery>,
) -> AppResult<Response> {
    let uid = q.user_id.unwrap_or(u.id);
    assert_can_access_user(&s.pool, &u, uid).await?;
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
    // Admins see all active users; non-admin leads see only their direct reports.
    let users: Vec<crate::auth::User> = if u.is_admin() {
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE active=TRUE ORDER BY last_name")
            .fetch_all(&s.pool)
            .await?
    } else {
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE active=TRUE AND approver_id=$1 AND role!='admin' ORDER BY last_name")
            .bind(u.id)
            .fetch_all(&s.pool)
            .await?
    };
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
    validate_range(q.from, q.to)?;
    let uid = q.user_id;
    if let Some(id) = uid {
        assert_can_access_user(&s.pool, &u, id).await?;
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
    } else if !u.is_admin() {
        // Non-admin lead with no specific user: restrict to direct reports.
        builder
            .push(" AND z.user_id IN (SELECT id FROM users WHERE approver_id = ")
            .push_bind(u.id)
            .push(" AND role != 'admin')");
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
    assert_can_access_user(&s.pool, &u, uid).await?;
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let now = chrono::Local::now();
    let current_year = now.year();
    // Cap the loop so future months (with zero actuals but full targets) do not
    // produce large artificial deficits in the cumulative balance.
    let max_month: u32 = if year < current_year {
        12
    } else if year == current_year {
        now.month()
    } else {
        // Future year - nothing has been worked yet.
        return Ok(Json(vec![]));
    };
    // Determine the user's start_date and overtime start balance.
    let (user_start, overtime_start_balance_min): (NaiveDate, i64) =
        sqlx::query_as("SELECT start_date, overtime_start_balance_min FROM users WHERE id=$1")
            .bind(uid)
            .fetch_one(&s.pool)
            .await?;
    let start_month = if user_start.year() == year {
        user_start.month()
    } else if user_start.year() > year {
        // User hasn't started yet in this year.
        return Ok(Json(vec![]));
    } else {
        1
    };
    let mut out = vec![];
    let mut cum = overtime_start_balance_min;
    for prior_year in user_start.year()..year {
        let prior_start_month = if prior_year == user_start.year() {
            user_start.month()
        } else {
            1
        };
        for prior_month in prior_start_month..=12 {
            let mstr = format!("{:04}-{:02}", prior_year, prior_month);
            let r = build_month(&s.pool, uid, &mstr).await?;
            cum += r.diff_min;
        }
    }
    for m in start_month..=max_month {
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
    assert_can_access_user(&s.pool, &u, uid).await?;
    if q.from > q.to {
        return Err(AppError::BadRequest("from must not be after to.".into()));
    }
    if (q.to - q.from).num_days() > 366 {
        return Err(AppError::BadRequest(
            "Date range must not exceed 366 days.".into(),
        ));
    }

    let user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, annual_leave_days, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(uid)
        .fetch_one(&s.pool)
        .await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;

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

    let today = chrono::Local::now().date_naive();
    let mut out = vec![];
    let mut cum = if loop_start < user.start_date {
        0
    } else {
        user.overtime_start_balance_min
    };
    let mut d = loop_start;
    while d <= q.to {
        if d == user.start_date && loop_start < user.start_date {
            cum += user.overtime_start_balance_min;
        }
        let wd = d.weekday().num_days_from_monday();
        let weekday = wd < 5;
        let holiday = h_map.get(&d).cloned();
        let absence = abs
            .iter()
            .find(|(s, e, _)| d >= *s && d <= *e)
            .map(|(_, _, k)| k.clone());
        let before_start = d < user.start_date;
        let after_today = d > today;
        let target = if weekday && holiday.is_none() && !before_start && !after_today {
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
        let actual_eff = credited_actual_minutes(actual, target, absence.as_deref());
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
