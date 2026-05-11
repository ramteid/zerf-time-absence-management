use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::time_calc;
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

const FLEXTIME_REDUCTION_KIND: &str = "flextime_reduction";

fn absence_removes_target(kind: &str) -> bool {
    kind != FLEXTIME_REDUCTION_KIND
}

fn reporting_today() -> NaiveDate {
    time_calc::today_local()
}

/// Verify that `requester` is allowed to read data for `target_uid`.
/// Admins may access any user. Non-admin leads may only access their direct
/// reports (users whose `approver_id` matches the lead's id). Every user may
/// always access their own data.
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
    pub counts_as_work: bool,
    pub status: String,
    pub comment: Option<String>,
}

fn entry_counts_as_work(counts_as_work: bool, status: &str) -> bool {
    counts_as_work && status != "rejected"
}

#[derive(Serialize)]
pub struct MonthReport {
    pub user_id: i64,
    pub month: String,
    pub days: Vec<DayDetail>,
    pub target_min: i64,
    pub actual_min: i64,
    pub diff_min: i64,
    /// Submitted + approved entries (excludes draft/rejected).
    pub submitted_min: i64,
    /// Full-month target without the "capped at today" restriction.
    pub full_month_target_min: i64,
    pub category_totals: HashMap<String, i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weeks_all_submitted: Option<bool>,
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

/// Determine if a date is a contract workday based on user's workdays_per_week.
/// Contract workdays are the first N days of the ISO week.
/// ISO weekday: 0=Monday, 1=Tuesday, ..., 6=Sunday
/// Examples:
///   - workdays_per_week=5: Mon-Fri are contract days
///   - workdays_per_week=4: Mon-Thu are contract days
///   - workdays_per_week=6: Mon-Sat are contract days
fn is_contract_workday(date: NaiveDate, workdays_per_week: i16) -> bool {
    // ISO weekday 0=Monday, 6=Sunday. Contract workdays are first N days of week.
    // Examples: workdays_per_week=5 → Mon-Fri OK, Sun-Sat not OK
    //          workdays_per_week=4 → Mon-Thu OK, Fri-Sun not OK
    date.weekday().num_days_from_monday() < workdays_per_week as u32
}

/// Calculate the daily target work minutes based on user's weekly hours and workdays_per_week.
/// Formula: (weekly_hours / workdays_per_week) * 60 minutes
/// Examples:
///   - 40 hours/week, 5 days: 8 hours/day = 480 minutes/day
///   - 40 hours/week, 4 days: 10 hours/day = 600 minutes/day
///   - 32 hours/week, 4 days: 8 hours/day = 480 minutes/day
fn target_minutes_per_day(weekly_hours: f64, workdays_per_week: i16) -> i64 {
    // Calculate daily target = (weekly_hours / workdays_per_week) * 60 minutes
    // Examples: 40h/5days=8h/day=480min, 40h/4days=10h/day=600min
    (weekly_hours / f64::from(workdays_per_week) * 60.0).round() as i64
}

async fn build_range(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
    label: &str,
) -> AppResult<MonthReport> {
    let repo_user = crate::repository::UserDb::new(pool.clone())
        .find_by_id(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let user = crate::users::repo_user_to_auth_user(repo_user);
    let target_per_day_min = target_minutes_per_day(user.weekly_hours, user.workdays_per_week);
    let today = reporting_today();

    let reports_db = crate::repository::ReportDb::new(pool.clone());

    #[allow(clippy::type_complexity)]
    let time_entry_rows: Vec<(
        NaiveDate,
        String,
        String,
        String,
        String,
        i64,
        bool,
        String,
        Option<String>,
    )> = reports_db.time_entry_rows(user_id, from, to).await?;
    // Pre-group by date so per-day lookups are O(1) instead of scanning all rows.
    let entries_by_date = group_entries_by_date(time_entry_rows);

    let approved_absence_rows: Vec<(NaiveDate, NaiveDate, String)> =
        reports_db.approved_absence_rows(user_id, from, to).await?;

    let language = i18n::load_ui_language(pool).await?;

    let holiday_raw = reports_db.holiday_rows(from, to).await?;
    let holiday_map: HashMap<NaiveDate, String> = holiday_raw
        .into_iter()
        .map(|(holiday_date, name, local_name)| {
            (
                holiday_date,
                i18n::holiday_display_name(&language, name, local_name),
            )
        })
        .collect();

    let mut days: Vec<DayDetail> = vec![];
    let mut target_total = 0i64;
    let mut actual_total = 0i64;
    let mut submitted_total = 0i64;
    let mut full_month_target_total = 0i64;
    let mut category_minutes_by_name: HashMap<String, i64> = HashMap::new();
    let mut current_date = from;
    while current_date <= to {
        let holiday = holiday_map.get(&current_date).cloned();
        let absence = approved_absence_rows
            .iter()
            .find(|(abs_start, abs_end, _)| current_date >= *abs_start && current_date <= *abs_end)
            .map(|(_, _, kind)| kind.clone());
        let before_start = current_date < user.start_date;
        let after_today = current_date > today;

        // A day has a work target when it is a weekday within the user's contract,
        // not covered by a holiday or absence, and not in the future.
        let absence_blocks_target = absence
            .as_deref()
            .map(absence_removes_target)
            .unwrap_or(false);
        let is_workday = is_contract_workday(current_date, user.workdays_per_week)
            && holiday.is_none()
            && !absence_blocks_target
            && !before_start;
        let target = if is_workday && !after_today { target_per_day_min } else { 0 };
        // full_month_target counts all contract workdays without the "capped at today" cutoff.
        let full_target = if is_workday { target_per_day_min } else { 0 };

        let mut entries: Vec<EntryDetail> = vec![];
        let mut actual = 0i64;
        let mut submitted = 0i64;
        // Skip entry processing entirely for inactive/future days.
        if !before_start && !after_today {
            for (
                start_time,
                end_time,
                category_name,
                category_color,
                _cat_id,
                counts_as_work,
                status,
                comment,
            )
                in entries_by_date.get(&current_date).into_iter().flatten()
            {
                if status == "rejected" {
                    continue;
                }
                // Defensive: surface a 500 on malformed time strings rather than panicking.
                // The DB schema does not constrain the text format.
                let entry_minutes =
                    (parse_report_time(end_time)? - parse_report_time(start_time)?).num_minutes();
                // Only approved entries count towards actual hours and the monthly diff.
                if *counts_as_work && status == "approved" {
                    actual += entry_minutes;
                }
                // submitted_min includes submitted + approved (everything the employee filed).
                if *counts_as_work && (status == "approved" || status == "submitted") {
                    submitted += entry_minutes;
                }
                // Category totals include every non-rejected entry.
                if entry_counts_as_work(*counts_as_work, status) {
                    *category_minutes_by_name.entry(category_name.clone()).or_insert(0) +=
                        entry_minutes;
                }
                entries.push(EntryDetail {
                    start_time: start_time.clone(),
                    end_time: end_time.clone(),
                    category: category_name.clone(),
                    color: category_color.clone(),
                    minutes: entry_minutes,
                    counts_as_work: *counts_as_work,
                    status: status.clone(),
                    comment: comment.clone(),
                });
            }
        }

        target_total += target;
        actual_total += actual;
        submitted_total += submitted;
        full_month_target_total += full_target;
        days.push(DayDetail {
            date: current_date,
            weekday: weekday_en(current_date).to_string(),
            entries,
            actual_min: actual,
            target_min: target,
            absence,
            holiday,
        });
        current_date += Duration::days(1);
    }
    Ok(MonthReport {
        user_id,
        month: label.into(),
        days,
        target_min: target_total,
        actual_min: actual_total,
        diff_min: actual_total - target_total,
        submitted_min: submitted_total,
        full_month_target_min: full_month_target_total,
        category_totals: category_minutes_by_name,
        weeks_all_submitted: None,
    })
}

async fn build_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month: &str,
) -> AppResult<MonthReport> {
    let (from, to) = month_bounds(month)?;
    let repo_user = crate::repository::UserDb::new(pool.clone())
        .find_by_id(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let user_start_date = repo_user.start_date;
    let workdays_per_week = repo_user.workdays_per_week;
    let mut report = build_range(pool, user_id, from, to, month).await?;
    report.weeks_all_submitted =
        Some(
            all_weeks_submitted_for_month(
                pool,
                user_id,
                from,
                to,
                user_start_date,
                workdays_per_week,
            )
            .await?,
        );
    Ok(report)
}

async fn build_month_without_submission_status(
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
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
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
    if (to - from).num_days() > 366 {
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
        tracing::error!(target: "zerf::reports", "CSV export failed: {error}");
        AppError::Internal("CSV export failed.".into())
    }
    let mut csv_writer = csv::Writer::from_writer(vec![]);
    csv_writer
        .write_record([
            "Date", "Weekday", "Start", "End", "Category", "Minutes", "Status", "Comment",
            "Absence", "Holiday",
        ])
        .map_err(csv_err)?;
    let mut csv_total_min = 0i64;
    for day in &r.days {
        if day.entries.is_empty() {
            csv_writer
                .write_record([
                    day.date.to_string(),
                    day.weekday.clone(),
                    "".into(),
                    "".into(),
                    "".into(),
                    "0".into(),
                    "".into(),
                    "".into(),
                    safe(&day.absence.clone().unwrap_or_default()),
                    safe(&day.holiday.clone().unwrap_or_default()),
                ])
                .map_err(csv_err)?;
        } else {
            for entry in &day.entries {
                if entry.counts_as_work {
                    csv_total_min += entry.minutes;
                }
                csv_writer
                    .write_record([
                        day.date.to_string(),
                        day.weekday.clone(),
                        entry.start_time.clone(),
                        entry.end_time.clone(),
                        safe(&entry.category),
                        entry.minutes.to_string(),
                        entry.status.clone(),
                        safe(&entry.comment.clone().unwrap_or_default()),
                        safe(&day.absence.clone().unwrap_or_default()),
                        safe(&day.holiday.clone().unwrap_or_default()),
                    ])
                    .map_err(csv_err)?;
            }
        }
    }
    csv_writer
        .write_record([
            "",
            "Total",
            "",
            "",
            "",
            &csv_total_min.to_string(),
            "",
            "",
            "",
            "",
        ])
        .map_err(csv_err)?;
    let csv_bytes = csv_writer.into_inner().map_err(|error| {
        tracing::error!(target: "zerf::reports", "CSV export finalize failed: {error}");
        AppError::Internal("CSV export failed.".into())
    })?;
    // Prepend the UTF-8 BOM so that Excel auto-detects the encoding and correctly
    // splits fields into columns regardless of the system locale.
    let mut data = Vec::with_capacity(3 + csv_bytes.len());
    data.extend_from_slice(b"\xEF\xBB\xBF");
    data.extend_from_slice(&csv_bytes);
    let mut response = Response::new(axum::body::Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "text/csv; charset=utf-8".parse().unwrap(),
    );
    let safe_label: String = file_label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(30)
        .collect();
    let content_disposition = format!(
        "attachment; filename=\"report-user-{}-{}.csv\"",
        uid, safe_label
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&content_disposition).unwrap_or_else(|_| {
            axum::http::HeaderValue::from_static("attachment; filename=\"report.csv\"")
        }),
    );
    Ok(response)
}

pub async fn month_csv(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CsvQuery>,
) -> AppResult<Response> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
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
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
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
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
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

/// One row in the team report - one record per active team member.
#[derive(Serialize)]
pub struct TeamRow {
    pub user_id: i64,
    pub name: String,
    /// Target minutes for the report month (excluding weekends, holidays, absences, and future days).
    pub target_min: i64,
    /// Actual minutes: approved time entries in the report month (including today).
    pub actual_min: i64,
    /// Diff = actual - target for the report month.
    pub diff_min: i64,
    /// Vacation working-days taken in the report month (including today).
    pub vacation_days: f64,
    /// Vacation working-days planned but not yet started in the report month (from tomorrow).
    pub vacation_planned_days: f64,
    /// Sick working-days in the report month.
    pub sick_days: f64,
    /// Current cumulative flextime balance as of today.
    pub flextime_balance_min: i64,
    /// True if all fully elapsed weeks (Sunday < today) overlapping the report month
    /// have been fully submitted.
    pub weeks_all_submitted: bool,
}

#[derive(Deserialize)]
pub struct TeamQuery {
    pub month: String,
}

/// Checks whether all fully elapsed working weeks overlapping the given month
/// have been submitted for the user.
///
/// A week is "fully elapsed" when its Sunday falls before today.
/// A boundary week spanning two months (e.g. Mon 28 Apr - Sun 3 May) counts
/// for both months: all five weekdays of the week are checked, not just the
/// days that fall within the target month.
///
/// A working day is considered submitted when either:
///   - an approved absence that removes the daily target covers the day, OR
///   - at least one time entry with status "submitted" or "approved" exists.
async fn all_weeks_submitted_for_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month_start: NaiveDate,
    month_end: NaiveDate,
    user_start_date: NaiveDate,
    workdays_per_week: i16,
) -> AppResult<bool> {
    let today = reporting_today();

    // Compute the Monday of the first and last week touched by the month.
    // Monday of the week in which the first day of the month falls.
    let first_week_monday = {
        let offset = month_start.weekday().num_days_from_monday() as i64;
        month_start - Duration::days(offset)
    };
    // Monday of the week in which the last day of the month falls.
    let last_week_monday = {
        let offset = month_end.weekday().num_days_from_monday() as i64;
        month_end - Duration::days(offset)
    };

    // Collect all fully elapsed weeks (Sunday < today).
    let mut complete_week_mondays: Vec<NaiveDate> = Vec::new();
    let mut current_week_monday = first_week_monday;
    while current_week_monday <= last_week_monday {
        let week_sunday = current_week_monday + Duration::days(6);
        if week_sunday < today {
            complete_week_mondays.push(current_week_monday);
        }
        current_week_monday += Duration::days(7);
    }

    // No fully elapsed past weeks - nothing to check.
    if complete_week_mondays.is_empty() {
        return Ok(true);
    }

    let check_from = complete_week_mondays[0];
    let check_to = *complete_week_mondays.last().unwrap() + Duration::days(6);

    // Load public holidays in the check range once, then use a set for cheap lookups.
    let reports_db = crate::repository::ReportDb::new(pool.clone());
    let holiday_set = reports_db.holiday_set(check_from, check_to).await?;

    // Build a set of approved absence days, clamped to the week check range.
    let absence_rows = reports_db
        .absence_ranges_in_period(user_id, check_from, check_to)
        .await?;
    let absent_days = expand_absence_date_set(&absence_rows, check_from, check_to);

    // Load submitted/approved time entry dates. Draft days are not submitted.
    let submitted_dates = reports_db
        .submitted_dates_in_range(user_id, check_from, check_to)
        .await?;
    // A day with a draft alongside a submitted entry is not fully submitted.
    let draft_dates = reports_db
        .draft_dates_in_range(user_id, check_from, check_to)
        .await?;

    // Check each fully elapsed week.
    // For each complete week, check that all contract workdays are submitted.
    // A contract workday must be covered by either:
    //   1. An approved/cancellation_pending absence, OR
    //   2. A submitted/approved time entry (with no draft conflicts)
    for &week_monday in &complete_week_mondays {
        // Iterate only the first workdays_per_week days of the week (skip non-contract days)
        // Check only contract workdays in this week (first workdays_per_week days).
        // Non-contract days (e.g., weekend for 5-day worker) are implicitly submitted.
        for day_offset in 0..i64::from(workdays_per_week) {
            let day = week_monday + Duration::days(day_offset);

            // Skip days before the user's contract start.
            if day < user_start_date {
                continue;
            }
            // Skip public holidays.
            if holiday_set.contains(&day) {
                continue;
            }
            // Skip future days (should not occur for fully elapsed weeks, but be defensive).
            if day >= today {
                continue;
            }

            // Every working day must be covered by a target-removing absence OR a submitted entry with no
            // outstanding drafts.
            let submitted_and_clean =
                submitted_dates.contains(&day) && !draft_dates.contains(&day);
            if !absent_days.contains(&day) && !submitted_and_clean {
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

    // Admins see all active users; team leads see themselves and their direct reports.
    let team_members: Vec<crate::auth::User> = app_state
        .db
        .reports
        .active_team_members(requester.id, requester.is_admin())
        .await?
        .into_iter()
        .map(crate::users::repo_user_to_auth_user)
        .collect();

    let today = reporting_today();
    let (month_start, month_end) = month_bounds(&query.month)?;

    // Vacation split for the selected month:
    // - taken includes today
    // - planned starts tomorrow
    let vacation_taken_end = today.min(month_end);
    let tomorrow = today + Duration::days(1);
    let vacation_planned_start = tomorrow.max(month_start);

    let mut team_rows = vec![];

    for team_member in team_members {
        // Reuse the month report so target, actual, and diff stay consistent.
        let month_report =
            build_month_without_submission_status(&app_state.pool, team_member.id, &query.month)
                .await?;

        // Vacation days taken are capped at today so current-day absences count.
        let absence_count_start = month_start.max(team_member.start_date);

        let vacation_taken = if absence_count_start <= vacation_taken_end {
            crate::absences::workdays_total(
                &app_state.pool,
                team_member.id,
                "vacation",
                absence_count_start,
                vacation_taken_end,
            )
            .await?
        } else {
            0.0
        };

        // Planned vacation starts from tomorrow inside the selected month.
        let vacation_planned_start = vacation_planned_start.max(team_member.start_date);
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

        // Sick days are capped at today so current-day sick leave counts.
        // Future absences are excluded to keep month-to-date semantics.
        let sick_end = today.min(month_end);
        let sick_workdays = if absence_count_start <= sick_end {
            crate::absences::workdays_total(
                &app_state.pool,
                team_member.id,
                "sick",
                absence_count_start,
                sick_end,
            )
            .await?
        } else {
            0.0
        };

        // Current flextime balance is independent of the selected month.
        // The latest row of the current year is the balance as of today.
        let current_year = today.year();
        let overtime_rows =
            build_overtime_rows_for_year(&app_state.pool, team_member.id, current_year).await?;
        let flextime_balance_min = overtime_rows
            .last()
            .map(|r| r.cumulative_min)
            .unwrap_or(team_member.overtime_start_balance_min);

        // Submission status uses full past weeks, including boundary weeks.
        let weeks_all_submitted = all_weeks_submitted_for_month(
            &app_state.pool,
            team_member.id,
            month_start,
            month_end,
            team_member.start_date,
            team_member.workdays_per_week,
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
    time_calc::parse_stored_time(raw)
}

// Type alias for the 8-field tuple stored per time entry after stripping the date.
type RawEntryTuple = (String, String, String, String, i64, bool, String, Option<String>);

/// Pre-groups raw time entry rows (as fetched from the DB) by date.
/// Allows O(1) per-day lookup instead of scanning the full list for each day.
fn group_entries_by_date(
    rows: Vec<(
        NaiveDate,
        String,
        String,
        String,
        String,
        i64,
        bool,
        String,
        Option<String>,
    )>,
) -> HashMap<NaiveDate, Vec<RawEntryTuple>> {
    let mut map: HashMap<NaiveDate, Vec<RawEntryTuple>> = HashMap::new();
    for (date, start, end, category, color, cat_id, counts_as_work, status, comment) in rows {
        map.entry(date)
            .or_default()
            .push((start, end, category, color, cat_id, counts_as_work, status, comment));
    }
    map
}

/// Expands a list of (start, end) date ranges into a flat set of individual dates,
/// clamped to the given [from, to] window.
fn expand_absence_date_set(
    ranges: &[(NaiveDate, NaiveDate, String)],
    from: NaiveDate,
    to: NaiveDate,
) -> std::collections::HashSet<NaiveDate> {
    let mut set = std::collections::HashSet::new();
    for (range_start, range_end, _kind) in ranges {
        // All approved absences cover the day for submission purposes: target-removing absences
        // (vacation, sick, training, etc.) replace the work requirement entirely; flextime_reduction
        // blocks entry creation so there is nothing for the user to submit on that day either.
        let mut day = (*range_start).max(from);
        while day <= (*range_end).min(to) {
            set.insert(day);
            day += Duration::days(1);
        }
    }
    set
}

/// Sorts category totals descending by minutes, then ascending by name.
fn sort_categories_desc(cats: &mut Vec<CategoryTotal>) {
    cats.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.category.cmp(&b.category))
    });
}

pub async fn categories(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CategoryQuery>,
) -> AppResult<Json<Vec<CategoryTotal>>> {
    validate_range(query.from, query.to)?;
    // Clamp to today so category reports include current-day entries but no future dates.
    let effective_to = query.to.min(reporting_today());
    if query.from > effective_to {
        return Ok(Json(Vec::new()));
    }

    let target_user_id = query.user_id;
    if let Some(user_id) = target_user_id {
        // Requesting a specific user: verify access rights.
        assert_can_access_user(&app_state, &requester, user_id).await?;
    } else if !requester.is_lead() {
        // No specific user requested: only leads may see aggregated team data.
        return Err(AppError::Forbidden);
    }
    // The category breakdown shows booked time, not only approved work time.
    // Rejected entries are excluded; effective_to ensures dates are bounded to today.
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT c.name, c.color, z.start_time, z.end_time \
         FROM time_entries z \
         JOIN users u ON u.id=z.user_id \
         JOIN categories c ON c.id=z.category_id \
            WHERE z.status != 'rejected' AND c.counts_as_work = TRUE AND z.entry_date >= u.start_date \
         AND z.entry_date BETWEEN ",
    );
    builder
        .push_bind(query.from)
        .push(" AND ")
        .push_bind(effective_to);
    if let Some(user_id) = target_user_id {
        builder.push(" AND z.user_id = ").push_bind(user_id);
    } else if !requester.is_admin() {
        // Non-admin lead with no specific user: include self and direct reports.
        builder
            .push(" AND z.user_id IN (SELECT id FROM users WHERE id = ")
            .push_bind(requester.id)
            .push(" OR (role != 'admin' AND id IN (SELECT user_id FROM user_approvers WHERE approver_id = ")
            .push_bind(requester.id)
            .push(")))");
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
        .map(|((category, color), minutes)| CategoryTotal { category, color, minutes })
        .collect();
    sort_categories_desc(&mut sorted_totals);
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
    // Clamp to today so team category reports include current-day entries.
    let effective_to = query.to.min(reporting_today());
    if query.from > effective_to {
        return Ok(Json(Vec::new()));
    }

    let mut user_builder = QueryBuilder::<Postgres>::new(
        "SELECT id, first_name, last_name FROM users WHERE active=TRUE",
    );
    if !requester.is_admin() {
        user_builder
            .push(" AND (id = ")
            .push_bind(requester.id)
            .push(" OR (role != 'admin' AND id IN (SELECT user_id FROM user_approvers WHERE approver_id = ")
            .push_bind(requester.id)
            .push(")))");
    }
    user_builder.push(" ORDER BY last_name, first_name");
    let members: Vec<(i64, String, String)> = user_builder
        .build_query_as()
        .fetch_all(&app_state.pool)
        .await?;

    // Same as the individual breakdown: include every booked, non-rejected entry
    // up to today, including drafts and submitted entries.
    let mut entry_builder = QueryBuilder::<Postgres>::new(
        "SELECT z.user_id, c.name, c.color, z.start_time, z.end_time \
         FROM time_entries z \
         JOIN users u ON u.id=z.user_id \
         JOIN categories c ON c.id=z.category_id \
            WHERE z.status != 'rejected' AND c.counts_as_work = TRUE AND z.entry_date >= u.start_date \
         AND z.entry_date BETWEEN ",
    );
    entry_builder
        .push_bind(query.from)
        .push(" AND ")
        .push_bind(effective_to);
    if !requester.is_admin() {
        entry_builder
            .push(" AND z.user_id IN (SELECT id FROM users WHERE id = ")
            .push_bind(requester.id)
            .push(" OR (role != 'admin' AND id IN (SELECT user_id FROM user_approvers WHERE approver_id = ")
            .push_bind(requester.id)
            .push(")))");
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
                .map(|((category, color), minutes)| CategoryTotal { category, color, minutes })
                .collect();
            sort_categories_desc(&mut cats);
            UserCategoryRow {
                user_id: uid,
                name: format!("{first} {last}"),
                categories: cats,
            }
        })
        .collect();

    Ok(Json(result))
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
    let reports_db = crate::repository::ReportDb::new(pool.clone());
    let (user_start_date, overtime_start_balance_min): (NaiveDate, i64) =
        reports_db.user_start_and_overtime(target_user_id).await?;

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
            let month_report =
                build_month_without_submission_status(pool, target_user_id, &month_label).await?;
            cumulative_min += month_report.diff_min;
        }
    }

    for month_num in first_month_in_year..=max_month {
        let month_label = format!("{:04}-{:02}", year, month_num);
        let month_report =
            build_month_without_submission_status(pool, target_user_id, &month_label).await?;
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

/// Query parameters for the overtime endpoint (used by the Dashboard).
#[derive(Deserialize)]
pub struct OvertimeQuery {
    pub user_id: Option<i64>,
    pub year: Option<i32>,
}

/// Returns per-month overtime rows for the requested year, used by the
/// Dashboard to display the current flextime balance and monthly diff.
pub async fn overtime(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<OvertimeQuery>,
) -> AppResult<Json<Vec<MonthRow>>> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
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
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
    if query.from > query.to {
        return Err(AppError::BadRequest("from must not be after to.".into()));
    }
    if (query.to - query.from).num_days() > 366 {
        return Err(AppError::BadRequest(
            "Date range must not exceed 366 days.".into(),
        ));
    }

    let user: crate::auth::User = crate::users::repo_user_to_auth_user(
        app_state
            .db
            .users
            .find_by_id(target_user_id)
            .await?
            .ok_or(AppError::NotFound)?,
    );
    let target_per_day_min = target_minutes_per_day(user.weekly_hours, user.workdays_per_week);

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
        let month_start =
            NaiveDate::from_ymd_opt(day_before_from.year(), day_before_from.month(), 1)
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

    let time_entries_raw = app_state
        .db
        .reports
        .flextime_entries(target_user_id, query.from, query.to)
        .await?;

    let mut approved_minutes_by_day: HashMap<NaiveDate, i64> = HashMap::new();
    for (entry_date, start_time, end_time, status, counts_as_work) in time_entries_raw {
        if status != "approved" || !counts_as_work {
            continue;
        }
        let minutes =
            (parse_report_time(&end_time)? - parse_report_time(&start_time)?).num_minutes();
        *approved_minutes_by_day.entry(entry_date).or_insert(0) += minutes;
    }

    let approved_absences = app_state
        .db
        .reports
        .approved_absence_rows(target_user_id, query.from, query.to)
        .await?;

    // Expand absence ranges into a per-day map so each day can look up its kind in O(1).
    let mut absence_by_day: HashMap<NaiveDate, String> = HashMap::new();
    for (absence_start, absence_end, absence_kind) in approved_absences {
        let mut day = absence_start.max(query.from);
        while day <= absence_end.min(query.to) {
            absence_by_day.entry(day).or_insert_with(|| absence_kind.clone());
            day += Duration::days(1);
        }
    }

    let language = i18n::load_ui_language(&app_state.pool).await?;

    let holiday_map: HashMap<NaiveDate, String> = app_state
        .db
        .reports
        .holiday_rows(query.from, query.to)
        .await?
        .into_iter()
        .map(|(date, name, local_name)| {
            (
                date,
                i18n::holiday_display_name(&language, name, local_name),
            )
        })
        .collect();

    let today = reporting_today();
    let mut flextime_days = vec![];
    let mut current_date = query.from;
    while current_date <= query.to {
        // Inject the configured overtime start balance on the user's first day
        // when the requested range begins before that date.
        if current_date == user.start_date && query.from < user.start_date {
            cumulative_min += user.overtime_start_balance_min;
        }
        let holiday = holiday_map.get(&current_date).cloned();
        let absence = absence_by_day.get(&current_date).cloned();
        let before_start = current_date < user.start_date;
        let after_today = current_date > today;
        let absence_blocks_target = absence
            .as_deref()
            .map(absence_removes_target)
            .unwrap_or(false);
        let is_workday = is_contract_workday(current_date, user.workdays_per_week)
            && holiday.is_none()
            && !absence_blocks_target
            && !before_start
            && !after_today;
        let target = if is_workday { target_per_day_min } else { 0 };
        let actual = if before_start || after_today {
            0
        } else {
            approved_minutes_by_day.get(&current_date).copied().unwrap_or(0)
        };
        let day_diff_min = actual - target;
        cumulative_min += day_diff_min;
        flextime_days.push(FlextimeDay {
            date: current_date,
            actual_min: actual,
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
