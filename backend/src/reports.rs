use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
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

fn month_bounds(month_str: &str) -> AppResult<(NaiveDate, NaiveDate)> {
    let (year_str, month_str) = month_str
        .split_once('-')
        .ok_or_else(|| AppError::BadRequest("month=YYYY-MM".into()))?;
    let year: i32 = year_str
        .parse()
        .map_err(|_| AppError::BadRequest("year".into()))?;
    let month_num: u32 = month_str
        .parse()
        .map_err(|_| AppError::BadRequest("month".into()))?;
    let from = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| AppError::BadRequest("date".into()))?;
    let next = if month_num == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month_num + 1, 1).unwrap()
    };
    Ok((from, next - Duration::days(1)))
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
        Some("sick") => actual.max(target),
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
    let user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;
    let today = chrono::Utc::now().date_naive();

    #[allow(clippy::type_complexity)]
    let te: Vec<(NaiveDate, String, String, String, String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT z.entry_date, z.start_time, z.end_time, c.name, c.color, z.category_id, z.status, z.comment FROM time_entries z JOIN categories c ON c.id=z.category_id WHERE z.user_id=$1 AND z.entry_date BETWEEN $2 AND $3 ORDER BY z.entry_date, z.start_time"
    ).bind(user_id).bind(from).bind(to).fetch_all(pool).await?;

    let abs: Vec<(NaiveDate, NaiveDate, String)> = sqlx::query_as(
        "SELECT start_date, end_date, kind FROM absences WHERE user_id=$1 AND status='approved' AND end_date >= $2 AND start_date <= $3"
    ).bind(user_id).bind(from).bind(to).fetch_all(pool).await?;

    let language = i18n::load_ui_language(pool).await?;

    let h_map: HashMap<NaiveDate, String> = sqlx::query_as::<_, (NaiveDate, String, Option<String>)>(
        "SELECT holiday_date, name, local_name FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(d, name, local_name)| (d, i18n::holiday_display_name(&language, name, local_name)))
    .collect();

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
        let after_today = d >= today;
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
            if st == "rejected" {
                continue;
            }
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
        let actual_eff = if after_today {
            0
        } else {
            credited_actual_minutes(actual, target, absence.as_deref())
        };
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
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<MonthQuery>,
) -> AppResult<Json<MonthReport>> {
    // Default to the requester's own data if no user_id is specified.
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    Ok(Json(
        build_month(&app_state.pool, target_user_id, &query.month).await?,
    ))
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
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CsvQuery>,
) -> AppResult<Response> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    let month = query
        .month
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("month=YYYY-MM".into()))?;
    let report = build_month(&app_state.pool, target_user_id, month).await?;
    csv_response(report, target_user_id, month)
}

#[derive(Deserialize)]
pub struct RangeQuery {
    pub user_id: Option<i64>,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

pub async fn range(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<RangeQuery>,
) -> AppResult<Json<MonthReport>> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    validate_range(query.from, query.to)?;
    let label = format!("{}_to_{}", query.from, query.to);
    let report = build_range(
        &app_state.pool,
        target_user_id,
        query.from,
        query.to,
        &label,
    )
    .await?;
    Ok(Json(report))
}

pub async fn range_csv(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CsvQuery>,
) -> AppResult<Response> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    let from = query
        .from
        .ok_or_else(|| AppError::BadRequest("from is required.".into()))?;
    let to = query
        .to
        .ok_or_else(|| AppError::BadRequest("to is required.".into()))?;
    validate_range(from, to)?;
    let label = format!("{}_to_{}", from, to);
    let report = build_range(&app_state.pool, target_user_id, from, to, &label).await?;
    csv_response(report, target_user_id, &label)
}

/// Eine Zeile im Teambericht – ein Datensatz pro aktivem Teammitglied.
#[derive(Serialize)]
pub struct TeamRow {
    pub user_id: i64,
    pub name: String,
    /// Soll-Minuten des Berichtsmonats (ohne Wochenenden, Feiertage, Abwesenheiten, ab heute).
    pub target_min: i64,
    /// Ist-Minuten: genehmigte Zeiteinträge im Berichtsmonat (bis gestern).
    pub actual_min: i64,
    /// Diff = actual - target für den Berichtsmonat.
    pub diff_min: i64,
    /// Genommene Urlaubsarbeitstage im Berichtsmonat (bis gestern).
    pub vacation_days: f64,
    /// Geplante (noch nicht begonnene) Urlaubsarbeitstage im Berichtsmonat (ab heute).
    pub vacation_planned_days: f64,
    /// Kranke Arbeitstage im Berichtsmonat.
    pub sick_days: f64,
    /// Kumulierter Gleitzeitkontostand am Ende des Berichtsmonats (oder bis gestern
    /// bei laufendem Monat).
    pub flextime_balance_min: i64,
    /// True, wenn alle vollständig vergangenen Wochen (Sonntag < heute), die den
    /// Berichtsmonat überschneiden, vollständig eingereicht wurden.
    pub weeks_all_submitted: bool,
}

#[derive(Deserialize)]
pub struct TeamQuery {
    pub month: String,
}

/// Prüft, ob alle vollständig vergangenen Arbeitswochen, die den angegebenen Monat
/// überschneiden, für den Benutzer eingereicht wurden.
///
/// Eine Woche gilt als "vollständig vergangen", wenn ihr Sonntag vor `heute` liegt.
/// Eine Grenzwoche, die zwei Monate überspannt (z.B. Mo 28.04. – So 03.05.), zählt
/// für BEIDE Monate – und es werden alle fünf Arbeitstage der Woche geprüft, nicht
/// nur die Tage des Zielmonats.
///
/// Ein Arbeitstag gilt als eingereicht, wenn
///   - eine genehmigte Abwesenheit den Tag abdeckt, ODER
///   - mindestens ein Zeiteintrag im Status "submitted" oder "approved" vorhanden ist.
async fn all_weeks_submitted_for_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month_start: NaiveDate,
    month_end: NaiveDate,
    user_start_date: NaiveDate,
) -> AppResult<bool> {
    let today = chrono::Utc::now().date_naive();

    // ── Wochengrenzen berechnen ──────────────────────────────────────────────
    // Montag der Woche, in der der erste Monatstag liegt.
    let first_week_monday = {
        let offset = month_start.weekday().num_days_from_monday() as i64;
        month_start - Duration::days(offset)
    };
    // Montag der Woche, in der der letzte Monatstag liegt.
    let last_week_monday = {
        let offset = month_end.weekday().num_days_from_monday() as i64;
        month_end - Duration::days(offset)
    };

    // Alle vollständig vergangenen Wochen sammeln (Sonntag < heute).
    let mut complete_week_mondays: Vec<NaiveDate> = Vec::new();
    let mut w = first_week_monday;
    while w <= last_week_monday {
        let week_sunday = w + Duration::days(6);
        if week_sunday < today {
            complete_week_mondays.push(w);
        }
        w += Duration::days(7);
    }

    // Keine vollständigen Vergangenheitswochen → nichts zu prüfen.
    if complete_week_mondays.is_empty() {
        return Ok(true);
    }

    let check_from = complete_week_mondays[0];
    let check_to = *complete_week_mondays.last().unwrap() + Duration::days(6);

    // ── Feiertage im Prüfzeitraum laden ─────────────────────────────────────
    let holiday_set: std::collections::HashSet<NaiveDate> = {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT holiday_date FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
        )
        .bind(check_from)
        .bind(check_to)
        .fetch_all(pool)
        .await?;
        rows.into_iter().map(|(d,)| d).collect()
    };

    // ── Genehmigte Abwesenheitstage als Set aufbauen ─────────────────────────
    let absence_rows: Vec<(NaiveDate, NaiveDate)> = sqlx::query_as(
        "SELECT start_date, end_date FROM absences \
         WHERE user_id=$1 AND status='approved' AND end_date >= $2 AND start_date <= $3",
    )
    .bind(user_id)
    .bind(check_from)
    .bind(check_to)
    .fetch_all(pool)
    .await?;

    let mut absent_days: std::collections::HashSet<NaiveDate> =
        std::collections::HashSet::new();
    for (abs_start, abs_end) in &absence_rows {
        let mut d = check_from.max(*abs_start);
        let end = check_to.min(*abs_end);
        while d <= end {
            absent_days.insert(d);
            d += Duration::days(1);
        }
    }

    // ── Eingereichte/genehmigte Zeiteinträge laden ──────────────────────────
    let submitted_dates: std::collections::HashSet<NaiveDate> = {
        let rows: Vec<(NaiveDate,)> = sqlx::query_as(
            "SELECT DISTINCT entry_date FROM time_entries \
             WHERE user_id=$1 AND status IN ('submitted','approved') \
             AND entry_date BETWEEN $2 AND $3",
        )
        .bind(user_id)
        .bind(check_from)
        .bind(check_to)
        .fetch_all(pool)
        .await?;
        rows.into_iter().map(|(d,)| d).collect()
    };

    // ── Jede vollständige Woche prüfen ───────────────────────────────────────
    for &week_monday in &complete_week_mondays {
        // Montag bis Freitag (0..5)
        for day_offset in 0..5i64 {
            let day = week_monday + Duration::days(day_offset);

            // Tage vor dem Vertragsstart des Benutzers überspringen.
            if day < user_start_date {
                continue;
            }
            // Feiertage überspringen.
            if holiday_set.contains(&day) {
                continue;
            }
            // Zukünftige Tage überspringen (sollte bei vollständigen Wochen nicht
            // vorkommen, aber defensiv).
            if day >= today {
                continue;
            }

            // Arbeitstag muss durch Abwesenheit ODER eingereichten Eintrag gedeckt sein.
            if !absent_days.contains(&day) && !submitted_dates.contains(&day) {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

pub async fn team(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<TeamQuery>,
) -> AppResult<Json<Vec<TeamRow>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }

    // Admins sehen alle aktiven Benutzer; Teamleitungen nur ihre direkten Berichte.
    let team_members: Vec<crate::auth::User> = if requester.is_admin() {
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE active=TRUE ORDER BY last_name")
            .fetch_all(&app_state.pool)
            .await?
    } else {
        sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE active=TRUE AND approver_id=$1 AND role!='admin' ORDER BY last_name")
            .bind(requester.id)
            .fetch_all(&app_state.pool)
            .await?
    };

    let today = chrono::Utc::now().date_naive();
    let yesterday = today - Duration::days(1);
    let (month_start, month_end) = month_bounds(&query.month)?;

    // Für den Urlaub-Split: "genommen" = bis gestern, "geplant" = ab heute.
    let vacation_taken_end = yesterday.min(month_end);
    let vacation_planned_start = today.max(month_start);

    let mut team_rows = vec![];

    for team_member in team_members {
        // ── Monatsbericht für Soll/Ist/Diff ─────────────────────────────────
        let month_report =
            build_month(&app_state.pool, team_member.id, &query.month).await?;

        // ── Urlaubstage: genommen (bis gestern) ──────────────────────────────
        let vacation_taken = if month_start <= vacation_taken_end {
            crate::absences::workdays_total(
                &app_state.pool,
                team_member.id,
                "vacation",
                month_start,
                vacation_taken_end,
            )
            .await?
        } else {
            0.0
        };

        // ── Urlaubstage: geplant (ab heute bis Monatsende) ───────────────────
        let vacation_planned = if vacation_planned_start <= month_end {
            crate::absences::workdays_total(
                &app_state.pool,
                team_member.id,
                "vacation",
                vacation_planned_start,
                month_end,
            )
            .await?
        } else {
            0.0
        };

        // ── Krankheitstage im vollen Monat ───────────────────────────────────
        let sick_workdays = crate::absences::workdays_total(
            &app_state.pool,
            team_member.id,
            "sick",
            month_start,
            month_end,
        )
        .await?;

        // ── Gleitzeitkontostand am Ende des Berichtsmonats ───────────────────
        // Wir rufen build_overtime_rows_for_year für das Jahr des Berichtsmonats auf
        // und suchen die Zeile dieses Monats. Bei laufendem Monat enthält die letzte
        // Zeile den Stand bis gestern (da build_range today ausschließt).
        let report_year = month_start.year();
        let overtime_rows =
            build_overtime_rows_for_year(&app_state.pool, team_member.id, report_year)
                .await?;
        let month_key = format!("{:04}-{:02}", report_year, month_start.month());
        let flextime_balance_min = overtime_rows
            .iter()
            .find(|r| r.month == month_key)
            .map(|r| r.cumulative_min)
            // Fallback: letzte verfügbare Zeile (für Zukunftsmonate nicht vorhanden).
            .unwrap_or_else(|| {
                overtime_rows
                    .last()
                    .map(|r| r.cumulative_min)
                    .unwrap_or(team_member.overtime_start_balance_min)
            });

        // ── Alle vergangenen Wochen eingereicht? ─────────────────────────────
        let weeks_all_submitted = all_weeks_submitted_for_month(
            &app_state.pool,
            team_member.id,
            month_start,
            month_end,
            team_member.start_date,
        )
        .await?;

        team_rows.push(TeamRow {
            user_id: team_member.id,
            name: format!("{} {}", team_member.first_name, team_member.last_name),
            target_min: month_report.target_min,
            actual_min: month_report.actual_min,
            diff_min: month_report.diff_min,
            vacation_days: vacation_taken,
            vacation_planned_days: vacation_planned,
            sick_days: sick_workdays,
            flextime_balance_min,
            weeks_all_submitted,
        });
    }

    Ok(Json(team_rows))
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
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CategoryQuery>,
) -> AppResult<Json<Vec<CategoryTotal>>> {
    validate_range(query.from, query.to)?;
    let target_user_id = query.user_id;
    if let Some(user_id) = target_user_id {
        // Requesting a specific user: verify access rights.
        assert_can_access_user(&app_state.pool, &requester, user_id).await?;
    } else if !requester.is_lead() {
        // No specific user requested: only leads may see aggregated team data.
        return Err(AppError::Forbidden);
    }
    // Wir schließen nur "rejected"-Einträge aus, damit auch eingereichte (aber noch
    // nicht genehmigte) Buchungen in der Kategorieauswertung erscheinen.
    // Das ist besonders für Admins und Teamleitungen wichtig, deren eigene Einträge
    // u.U. noch nicht genehmigt wurden.
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT c.name, c.color, z.start_time, z.end_time \
         FROM time_entries z \
         JOIN categories c ON c.id=z.category_id \
         WHERE z.status != 'rejected' AND z.entry_date BETWEEN ",
    );
    builder
        .push_bind(query.from)
        .push(" AND ")
        .push_bind(query.to);
    if let Some(user_id) = target_user_id {
        builder.push(" AND z.user_id = ").push_bind(user_id);
    } else if !requester.is_admin() {
        // Non-admin lead with no specific user: restrict to direct reports.
        builder
            .push(" AND z.user_id IN (SELECT id FROM users WHERE approver_id = ")
            .push_bind(requester.id)
            .push(" AND role != 'admin')");
    }
    let rows: Vec<(String, String, String, String)> =
        builder.build_query_as().fetch_all(&app_state.pool).await?;
    let mut category_minutes_map: HashMap<(String, String), i64> = HashMap::new();
    for (category, color, start_time, end_time) in rows {
        let minutes =
            (parse_report_time(&end_time)? - parse_report_time(&start_time)?).num_minutes();
        *category_minutes_map.entry((category, color)).or_insert(0) += minutes;
    }
    let mut sorted_totals: Vec<CategoryTotal> = category_minutes_map
        .into_iter()
        .map(|((category, color), minutes)| CategoryTotal {
            category,
            color,
            minutes,
        })
        .collect();
    sorted_totals.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.category.cmp(&b.category))
    });
    Ok(Json(sorted_totals))
}

#[derive(Serialize)]
pub struct UserCategoryRow {
    pub user_id: i64,
    pub name: String,
    pub categories: Vec<CategoryTotal>,
}

pub async fn team_categories(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CategoryQuery>,
) -> AppResult<Json<Vec<UserCategoryRow>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    validate_range(query.from, query.to)?;

    let mut user_builder = QueryBuilder::<Postgres>::new(
        "SELECT id, first_name, last_name FROM users WHERE active=TRUE",
    );
    if !requester.is_admin() {
        user_builder
            .push(" AND approver_id = ")
            .push_bind(requester.id)
            .push(" AND role != 'admin'");
    }
    user_builder.push(" ORDER BY last_name, first_name");
    let members: Vec<(i64, String, String)> = user_builder
        .build_query_as()
        .fetch_all(&app_state.pool)
        .await?;

    // Wie bei der Einzelauswertung: rejected ausschließen, aber submitted+approved
    // und draft einbeziehen, damit das Teambild vollständig ist.
    let mut entry_builder = QueryBuilder::<Postgres>::new(
        "SELECT z.user_id, c.name, c.color, z.start_time, z.end_time \
         FROM time_entries z \
         JOIN categories c ON c.id=z.category_id \
         WHERE z.status != 'rejected' AND z.entry_date BETWEEN ",
    );
    entry_builder
        .push_bind(query.from)
        .push(" AND ")
        .push_bind(query.to);
    if !requester.is_admin() {
        entry_builder
            .push(" AND z.user_id IN (SELECT id FROM users WHERE approver_id = ")
            .push_bind(requester.id)
            .push(" AND role != 'admin')");
    }
    let rows: Vec<(i64, String, String, String, String)> = entry_builder
        .build_query_as()
        .fetch_all(&app_state.pool)
        .await?;

    let mut user_cat_map: HashMap<i64, HashMap<(String, String), i64>> = HashMap::new();
    for (user_id, category, color, start_time, end_time) in rows {
        let minutes =
            (parse_report_time(&end_time)? - parse_report_time(&start_time)?).num_minutes();
        *user_cat_map
            .entry(user_id)
            .or_default()
            .entry((category, color))
            .or_insert(0) += minutes;
    }

    let result = members
        .into_iter()
        .map(|(uid, first, last)| {
            let mut cats: Vec<CategoryTotal> = user_cat_map
                .remove(&uid)
                .unwrap_or_default()
                .into_iter()
                .map(|((category, color), minutes)| CategoryTotal {
                    category,
                    color,
                    minutes,
                })
                .collect();
            cats.sort_by(|a, b| {
                b.minutes
                    .cmp(&a.minutes)
                    .then_with(|| a.category.cmp(&b.category))
            });
            UserCategoryRow {
                user_id: uid,
                name: format!("{first} {last}"),
                categories: cats,
            }
        })
        .collect();

    Ok(Json(result))
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

async fn build_overtime_rows_for_year(
    pool: &crate::db::DatabasePool,
    target_user_id: i64,
    year: i32,
) -> AppResult<Vec<MonthRow>> {
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
        return Ok(vec![]);
    };

    // Determine the user's start_date and overtime start balance.
    let (user_start_date, overtime_start_balance_min): (NaiveDate, i64) =
        sqlx::query_as("SELECT start_date, overtime_start_balance_min FROM users WHERE id=$1")
            .bind(target_user_id)
            .fetch_one(pool)
            .await?;

    let first_month_in_year = if user_start_date.year() == year {
        user_start_date.month()
    } else if user_start_date.year() > year {
        // User hasn't started yet in this year: nothing to show.
        return Ok(vec![]);
    } else {
        1
    };

    let mut month_rows = vec![];
    // Accumulate all prior-year months to seed the running overtime balance.
    let mut cumulative_min = overtime_start_balance_min;
    for prior_year in user_start_date.year()..year {
        let prior_year_first_month = if prior_year == user_start_date.year() {
            user_start_date.month()
        } else {
            1
        };
        for prior_month in prior_year_first_month..=12 {
            let month_label = format!("{:04}-{:02}", prior_year, prior_month);
            let month_report = build_month(pool, target_user_id, &month_label).await?;
            cumulative_min += month_report.diff_min;
        }
    }

    for month_num in first_month_in_year..=max_month {
        let month_label = format!("{:04}-{:02}", year, month_num);
        let month_report = build_month(pool, target_user_id, &month_label).await?;
        cumulative_min += month_report.diff_min;
        month_rows.push(MonthRow {
            month: month_label,
            target_min: month_report.target_min,
            actual_min: month_report.actual_min,
            diff_min: month_report.diff_min,
            cumulative_min,
        });
    }

    Ok(month_rows)
}

async fn cumulative_at_month_end(
    pool: &crate::db::DatabasePool,
    target_user_id: i64,
    year: i32,
    month: u32,
    user_start_date: NaiveDate,
    overtime_start_balance_min: i64,
) -> AppResult<i64> {
    if year < user_start_date.year()
        || (year == user_start_date.year() && month < user_start_date.month())
    {
        return Ok(overtime_start_balance_min);
    }

    let now = chrono::Local::now();
    let current_year = now.year();
    let current_month = now.month();

    let rows = build_overtime_rows_for_year(pool, target_user_id, year.min(current_year)).await?;
    if rows.is_empty() {
        return Ok(overtime_start_balance_min);
    }

    if year > current_year || (year == current_year && month > current_month) {
        return Ok(rows
            .last()
            .map(|row| row.cumulative_min)
            .unwrap_or(overtime_start_balance_min));
    }

    let key = format!("{:04}-{:02}", year, month);
    if let Some(row) = rows.iter().find(|row| row.month == key) {
        return Ok(row.cumulative_min);
    }

    Ok(overtime_start_balance_min)
}

pub async fn overtime(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<OvertimeQuery>,
) -> AppResult<Json<Vec<MonthRow>>> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());
    Ok(Json(
        build_overtime_rows_for_year(&app_state.pool, target_user_id, year).await?,
    ))
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
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<FlextimeQuery>,
) -> AppResult<Json<Vec<FlextimeDay>>> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state.pool, &requester, target_user_id).await?;
    if query.from > query.to {
        return Err(AppError::BadRequest("from must not be after to.".into()));
    }
    if (query.to - query.from).num_days() > 366 {
        return Err(AppError::BadRequest(
            "Date range must not exceed 366 days.".into(),
        ));
    }

    let user: crate::auth::User = sqlx::query_as("SELECT id, email, password_hash, first_name, last_name, role, weekly_hours, start_date, active, must_change_password, created_at, approver_id, allow_reopen_without_approval, dark_mode, overtime_start_balance_min FROM users WHERE id=$1")
        .bind(target_user_id)
        .fetch_one(&app_state.pool)
        .await?;
    let target_per_day_min = (user.weekly_hours / 5.0 * 60.0) as i64;

    // Seed cumulative at query.from-1 via month-level overtime plus a small
    // partial-month report, so per-day flextime processing stays within the
    // requested output range.
    let mut cumulative_min = if query.from < user.start_date {
        0
    } else {
        user.overtime_start_balance_min
    };
    if query.from > user.start_date {
        let day_before_from = query.from - Duration::days(1);
        let month_start = NaiveDate::from_ymd_opt(
            day_before_from.year(),
            day_before_from.month(),
            1,
        )
        .ok_or_else(|| AppError::BadRequest("date".into()))?;

        let cumulative_before_month = if month_start <= user.start_date {
            user.overtime_start_balance_min
        } else {
            let previous_month_end = month_start - Duration::days(1);
            cumulative_at_month_end(
                &app_state.pool,
                target_user_id,
                previous_month_end.year(),
                previous_month_end.month(),
                user.start_date,
                user.overtime_start_balance_min,
            )
            .await?
        };

        let seed_from = std::cmp::max(month_start, user.start_date);
        if seed_from <= day_before_from {
            let month_seed_report = build_range(
                &app_state.pool,
                target_user_id,
                seed_from,
                day_before_from,
                "seed",
            )
            .await?;
            cumulative_min = cumulative_before_month + month_seed_report.diff_min;
        } else {
            cumulative_min = cumulative_before_month;
        }
    }

    let time_entries_raw: Vec<(NaiveDate, String, String, String)> = sqlx::query_as(
        "SELECT entry_date, start_time, end_time, status \
         FROM time_entries WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3",
    )
    .bind(target_user_id)
    .bind(query.from)
    .bind(query.to)
    .fetch_all(&app_state.pool)
    .await?;

    let mut approved_minutes_by_day: HashMap<NaiveDate, i64> = HashMap::new();
    for (entry_date, start_time, end_time, status) in time_entries_raw {
        if status != "approved" {
            continue;
        }
        let minutes =
            (parse_report_time(&end_time)? - parse_report_time(&start_time)?).num_minutes();
        *approved_minutes_by_day.entry(entry_date).or_insert(0) += minutes;
    }

    let approved_absences: Vec<(NaiveDate, NaiveDate, String)> = sqlx::query_as(
        "SELECT start_date, end_date, kind FROM absences \
         WHERE user_id=$1 AND status='approved' AND end_date >= $2 AND start_date <= $3",
    )
    .bind(target_user_id)
    .bind(query.from)
    .bind(query.to)
    .fetch_all(&app_state.pool)
    .await?;

    let mut absence_by_day: HashMap<NaiveDate, String> = HashMap::new();
    for (absence_start, absence_end, absence_kind) in approved_absences {
        let mut day = std::cmp::max(absence_start, query.from);
        let end = std::cmp::min(absence_end, query.to);
        while day <= end {
            absence_by_day
                .entry(day)
                .or_insert_with(|| absence_kind.clone());
            day += Duration::days(1);
        }
    }

    let language = i18n::load_ui_language(&app_state.pool).await?;

    let holiday_map: HashMap<NaiveDate, String> = sqlx::query_as::<
        _,
        (NaiveDate, String, Option<String>),
    >(
        "SELECT holiday_date, name, local_name FROM holidays WHERE holiday_date BETWEEN $1 AND $2",
    )
    .bind(query.from)
    .bind(query.to)
    .fetch_all(&app_state.pool)
    .await?
    .into_iter()
    .map(|(date, name, local_name)| {
        (
            date,
            i18n::holiday_display_name(&language, name, local_name),
        )
    })
    .collect();

    let today = chrono::Utc::now().date_naive();
    let mut flextime_days = vec![];
    let mut current_date = query.from;
    while current_date <= query.to {
        // Inject the configured overtime start balance on the user's first day
        // when the requested range begins before that date.
        if current_date == user.start_date && query.from < user.start_date {
            cumulative_min += user.overtime_start_balance_min;
        }
        let day_of_week_num = current_date.weekday().num_days_from_monday();
        let is_weekday = day_of_week_num < 5;
        let holiday = holiday_map.get(&current_date).cloned();
        let absence = absence_by_day.get(&current_date).cloned();
        let before_start = current_date < user.start_date;
        let after_today = current_date >= today;
        let target = if is_weekday && holiday.is_none() && !before_start && !after_today {
            target_per_day_min
        } else {
            0
        };
        let actual = if after_today {
            0
        } else {
            approved_minutes_by_day.get(&current_date).copied().unwrap_or(0)
        };
        let credited_actual_min = credited_actual_minutes(actual, target, absence.as_deref());
        let day_diff_min = credited_actual_min - target;
        cumulative_min += day_diff_min;
        flextime_days.push(FlextimeDay {
            date: current_date,
            actual_min: credited_actual_min,
            target_min: target,
            diff_min: day_diff_min,
            cumulative_min,
            absence,
            holiday,
        });
        current_date += Duration::days(1);
    }
    Ok(Json(flextime_days))
}
