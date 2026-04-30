use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{extract::{State, Query}, Json, response::Response, http::header};
use chrono::{Datelike, NaiveDate, Duration};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct MonthQuery { pub user_id: Option<i64>, pub month: String }

fn month_bounds(m: &str) -> AppResult<(NaiveDate, NaiveDate)> {
    let parts: Vec<&str> = m.split('-').collect();
    if parts.len() != 2 { return Err(AppError::BadRequest("month=YYYY-MM".into())); }
    let y: i32 = parts[0].parse().map_err(|_| AppError::BadRequest("year".into()))?;
    let mo: u32 = parts[1].parse().map_err(|_| AppError::BadRequest("month".into()))?;
    let from = NaiveDate::from_ymd_opt(y, mo, 1).ok_or_else(|| AppError::BadRequest("date".into()))?;
    let to = if mo == 12 { NaiveDate::from_ymd_opt(y+1,1,1).unwrap() } else { NaiveDate::from_ymd_opt(y,mo+1,1).unwrap() } - Duration::days(1);
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
    ["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"][d.weekday().num_days_from_monday() as usize]
}

async fn build_month(pool: &sqlx::SqlitePool, user_id: i64, month: &str) -> AppResult<MonthReport> {
    let (from, to) = month_bounds(month)?;
    let user: crate::auth::User = sqlx::query_as("SELECT * FROM users WHERE id=?").bind(user_id).fetch_one(pool).await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;

    let te: Vec<(NaiveDate, String, String, String, String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT z.entry_date, z.start_time, z.end_time, c.name, c.color, z.category_id, z.status, z.comment FROM time_entries z JOIN categories c ON c.id=z.category_id WHERE z.user_id=? AND z.entry_date BETWEEN ? AND ? ORDER BY z.entry_date, z.start_time"
    ).bind(user_id).bind(from).bind(to).fetch_all(pool).await?;

    let abs: Vec<(NaiveDate, NaiveDate, String, bool)> = sqlx::query_as(
        "SELECT start_date, end_date, kind, half_day FROM absences WHERE user_id=? AND status='approved' AND end_date >= ? AND start_date <= ?"
    ).bind(user_id).bind(from).bind(to).fetch_all(pool).await?;

    let h: Vec<(NaiveDate, String)> = sqlx::query_as("SELECT holiday_date, name FROM holidays WHERE holiday_date BETWEEN ? AND ?").bind(from).bind(to).fetch_all(pool).await?;
    let h_map: HashMap<NaiveDate, String> = h.into_iter().collect();

    let mut days: Vec<DayDetail> = vec![];
    let mut target_total = 0i64; let mut actual_total = 0i64;
    let mut cat: HashMap<String, i64> = HashMap::new();
    let mut d = from;
    while d <= to {
        let wd = d.weekday().num_days_from_monday();
        let weekday = wd < 5;
        let holiday = h_map.get(&d).cloned();
        let absence = abs.iter().find(|(s,e,_,_)| d >= *s && d <= *e).map(|(_,_,k,_)| k.clone());
        let target = if weekday && holiday.is_none() { target_per_day_min } else { 0 };
        let mut entries: Vec<EntryDetail> = vec![];
        let mut actual = 0i64;
        for (dd, b, e, cn, cf, _kid, st, km) in &te {
            if *dd != d { continue; }
            let bn = chrono::NaiveTime::parse_from_str(b, "%H:%M").or_else(|_| chrono::NaiveTime::parse_from_str(b, "%H:%M:%S")).unwrap();
            let en = chrono::NaiveTime::parse_from_str(e, "%H:%M").or_else(|_| chrono::NaiveTime::parse_from_str(e, "%H:%M:%S")).unwrap();
            let m = (en-bn).num_minutes();
            if st == "approved" {
                actual += m;
                *cat.entry(cn.clone()).or_insert(0) += m;
            }
            entries.push(EntryDetail { start_time: b.clone(), end_time: e.clone(), category: cn.clone(), color: cf.clone(), minutes: m, status: st.clone(), comment: km.clone() });
        }
        let actual_eff = if absence.is_some() { target } else { actual };
        target_total += target;
        actual_total += actual_eff;
        days.push(DayDetail { date: d, weekday: weekday_en(d).to_string(), entries, actual_min: actual_eff, target_min: target, absence, holiday });
        d = d + Duration::days(1);
    }
    Ok(MonthReport { user_id, month: month.into(), days, target_min: target_total, actual_min: actual_total, diff_min: actual_total - target_total, category_totals: cat })
}

pub async fn month(State(s): State<AppState>, u: User, Query(q): Query<MonthQuery>) -> AppResult<Json<MonthReport>> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() { return Err(AppError::Forbidden); }
    Ok(Json(build_month(&s.pool, uid, &q.month).await?))
}

pub async fn month_csv(State(s): State<AppState>, u: User, Query(q): Query<MonthQuery>) -> AppResult<Response> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() { return Err(AppError::Forbidden); }
    let r = build_month(&s.pool, uid, &q.month).await?;
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record(&["Date","Weekday","Start","End","Category","Minutes","Status","Comment","Absence","Holiday"]).ok();
    for t in &r.days {
        if t.entries.is_empty() {
            wtr.write_record(&[
                t.date.to_string(), t.weekday.clone(), "".into(), "".into(), "".into(), "0".into(), "".into(), "".into(),
                t.absence.clone().unwrap_or_default(), t.holiday.clone().unwrap_or_default()
            ]).ok();
        } else {
            for e in &t.entries {
                wtr.write_record(&[
                    t.date.to_string(), t.weekday.clone(), e.start_time.clone(), e.end_time.clone(), e.category.clone(),
                    e.minutes.to_string(), e.status.clone(), e.comment.clone().unwrap_or_default(),
                    t.absence.clone().unwrap_or_default(), t.holiday.clone().unwrap_or_default()
                ]).ok();
            }
        }
    }
    wtr.write_record(&["", "Total", "", "", "", &r.actual_min.to_string(), "", "", "", ""]).ok();
    let data = wtr.into_inner().unwrap();
    let mut resp = Response::new(axum::body::Body::from(data));
    resp.headers_mut().insert(header::CONTENT_TYPE, "text/csv; charset=utf-8".parse().unwrap());
    resp.headers_mut().insert(header::CONTENT_DISPOSITION, format!("attachment; filename=\"report-{}-{}.csv\"", uid, q.month).parse().unwrap());
    Ok(resp)
}

#[derive(Serialize)]
pub struct TeamRow { pub user_id: i64, pub name: String, pub target_min: i64, pub actual_min: i64, pub diff_min: i64, pub vacation_days: f64, pub sick_days: f64 }

#[derive(Deserialize)]
pub struct TeamQuery { pub month: String }

pub async fn team(State(s): State<AppState>, u: User, Query(q): Query<TeamQuery>) -> AppResult<Json<Vec<TeamRow>>> {
    if !u.is_lead() { return Err(AppError::Forbidden); }
    let users: Vec<crate::auth::User> = sqlx::query_as("SELECT * FROM users WHERE active=1 ORDER BY last_name").fetch_all(&s.pool).await?;
    let mut out = vec![];
    let (from, to) = month_bounds(&q.month)?;
    for usr in users {
        let r = build_month(&s.pool, usr.id, &q.month).await?;
        let vac = crate::absences::workdays_total(&s.pool, usr.id, "vacation", from, to).await?;
        let sick = crate::absences::workdays_total(&s.pool, usr.id, "sick", from, to).await?;
        out.push(TeamRow { user_id: usr.id, name: format!("{} {}", usr.first_name, usr.last_name), target_min: r.target_min, actual_min: r.actual_min, diff_min: r.diff_min, vacation_days: vac, sick_days: sick });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CategoryQuery { pub from: NaiveDate, pub to: NaiveDate, pub user_id: Option<i64> }

pub async fn categories(State(s): State<AppState>, u: User, Query(q): Query<CategoryQuery>) -> AppResult<Json<Vec<serde_json::Value>>> {
    let uid = q.user_id;
    if let Some(id) = uid { if id != u.id && !u.is_lead() { return Err(AppError::Forbidden); } }
    else if !u.is_lead() { return Err(AppError::Forbidden); }
    let mut sql = String::from("SELECT c.name, c.color, COALESCE(SUM((strftime('%s', '2000-01-01 ' || z.end_time) - strftime('%s', '2000-01-01 ' || z.start_time))/60),0) AS m FROM time_entries z JOIN categories c ON c.id=z.category_id WHERE z.status='approved' AND z.entry_date BETWEEN ? AND ?");
    if uid.is_some() { sql += " AND z.user_id = ?"; }
    sql += " GROUP BY c.id ORDER BY m DESC";
    let mut qx = sqlx::query_as::<_, (String, String, i64)>(&sql).bind(q.from).bind(q.to);
    if let Some(id) = uid { qx = qx.bind(id); }
    let rows = qx.fetch_all(&s.pool).await?;
    Ok(Json(rows.into_iter().map(|(n,c,m)| serde_json::json!({"category": n, "color": c, "minutes": m})).collect()))
}

#[derive(Deserialize)]
pub struct OvertimeQuery { pub user_id: Option<i64>, pub year: Option<i32> }

#[derive(Serialize)]
pub struct MonthRow { pub month: String, pub target_min: i64, pub actual_min: i64, pub diff_min: i64, pub cumulative_min: i64 }

pub async fn overtime(State(s): State<AppState>, u: User, Query(q): Query<OvertimeQuery>) -> AppResult<Json<Vec<MonthRow>>> {
    let uid = q.user_id.unwrap_or(u.id);
    if uid != u.id && !u.is_lead() { return Err(AppError::Forbidden); }
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let mut out = vec![]; let mut cum = 0i64;
    for m in 1..=12u32 {
        let mstr = format!("{:04}-{:02}", year, m);
        let r = build_month(&s.pool, uid, &mstr).await?;
        cum += r.diff_min;
        out.push(MonthRow { month: mstr, target_min: r.target_min, actual_min: r.actual_min, diff_min: r.diff_min, cumulative_min: cum });
    }
    Ok(Json(out))
}
