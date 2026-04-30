use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// Gauss's Easter algorithm
pub fn easter_sunday(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32).unwrap()
}

pub fn holidays_bw(year: i32) -> Vec<(NaiveDate, &'static str)> {
    let o = easter_sunday(year);
    let d = |off: i64| o + chrono::Duration::days(off);
    vec![
        (
            NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
            "New Year's Day",
        ),
        (NaiveDate::from_ymd_opt(year, 1, 6).unwrap(), "Epiphany"),
        (d(-2), "Good Friday"),
        (d(1), "Easter Monday"),
        (NaiveDate::from_ymd_opt(year, 5, 1).unwrap(), "Labour Day"),
        (d(39), "Ascension Day"),
        (d(50), "Whit Monday"),
        (d(60), "Corpus Christi"),
        (
            NaiveDate::from_ymd_opt(year, 10, 3).unwrap(),
            "German Unity Day",
        ),
        (
            NaiveDate::from_ymd_opt(year, 11, 1).unwrap(),
            "All Saints' Day",
        ),
        (
            NaiveDate::from_ymd_opt(year, 12, 25).unwrap(),
            "Christmas Day",
        ),
        (NaiveDate::from_ymd_opt(year, 12, 26).unwrap(), "Boxing Day"),
    ]
}

pub async fn ensure_holidays(pool: &crate::db::DatabasePool, year: i32) -> AppResult<()> {
    for (d, name) in holidays_bw(year) {
        sqlx::query("INSERT INTO holidays(holiday_date, name, year) VALUES ($1, $2, $3) ON CONFLICT (holiday_date) DO NOTHING")
            .bind(d)
            .bind(name)
            .bind(year)
            .execute(pool)
            .await?;
    }
    Ok(())
}

#[derive(FromRow, Serialize)]
pub struct Holiday {
    pub id: i64,
    pub holiday_date: NaiveDate,
    pub name: String,
    pub year: i64,
}

#[derive(Deserialize)]
pub struct YearQuery {
    pub year: Option<i32>,
}

pub async fn list(
    State(s): State<AppState>,
    _u: User,
    Query(q): Query<YearQuery>,
) -> AppResult<Json<Vec<Holiday>>> {
    let year = q.year.unwrap_or_else(|| chrono::Local::now().year());
    let r =
        sqlx::query_as::<_, Holiday>("SELECT * FROM holidays WHERE year=$1 ORDER BY holiday_date")
            .bind(year)
            .fetch_all(&s.pool)
            .await?;
    Ok(Json(r))
}

#[derive(Deserialize)]
pub struct NewHoliday {
    pub holiday_date: NaiveDate,
    pub name: String,
}

pub async fn create(
    State(s): State<AppState>,
    u: User,
    Json(b): Json<NewHoliday>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("INSERT INTO holidays(holiday_date, name, year) VALUES ($1,$2,$3)")
        .bind(b.holiday_date)
        .bind(&b.name)
        .bind(b.holiday_date.year())
        .execute(&s.pool)
        .await
        .map_err(|_| AppError::Conflict("Holiday already exists".into()))?;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn delete(
    State(s): State<AppState>,
    u: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !u.is_admin() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("DELETE FROM holidays WHERE id=$1")
        .bind(id)
        .execute(&s.pool)
        .await?;
    Ok(Json(serde_json::json!({"ok":true})))
}
