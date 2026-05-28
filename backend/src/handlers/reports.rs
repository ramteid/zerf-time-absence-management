use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::roles::is_assistant_role;
use crate::services::reports::{
    all_weeks_submitted_for_month, assert_can_access_user, build_month,
    build_month_without_submission_status, build_overtime_rows_for_year, build_range,
    build_range_with_user, csv_response, cumulative_at_month_end, month_bounds,
    parse_report_time, sort_categories_desc, validate_range, CategoryTotal, FlextimeDay,
    MonthReport, MonthRow, TeamRow, UserCategoryRow,
};
use crate::AppState;
use axum::{
    extract::{Query, State},
    response::Response,
    Json,
};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct MonthQuery {
    pub user_id: Option<i64>,
    pub month: String,
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

#[derive(Deserialize)]
pub struct TeamQuery {
    pub month: String,
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
    let team_members: Vec<crate::middleware::auth::User> = app_state
        .db
        .reports
        .active_team_members(requester.id, requester.is_admin())
        .await?
        .into_iter()
        // Pure-admin users (tracks_time=false) have no own tracking dataset and
        // are excluded from team report rows.
        .filter(|team_member| team_member.tracks_time)
        .map(crate::services::users::repo_user_to_auth_user)
        .collect();

    let today = crate::services::settings::app_today(&app_state.pool).await;
    let (month_start, month_end) = month_bounds(&query.month)?;

    // Vacation split for the selected month:
    // - taken includes today
    // - planned starts tomorrow
    let vacation_taken_end = today.min(month_end);
    let tomorrow = today + Duration::days(1);
    let vacation_planned_start = tomorrow.max(month_start);

    let mut team_rows = vec![];

    for team_member in team_members {
        let team_member_is_assistant = is_assistant_role(&team_member.role);
        // Reuse the month report so target, actual, and diff stay consistent.
        let month_report =
            build_month_without_submission_status(&app_state.pool, team_member.id, &query.month)
                .await?;

        // Vacation days taken are capped at today so current-day absences count.
        let absence_count_start = month_start.max(team_member.start_date);

        let vacation_taken = if absence_count_start <= vacation_taken_end {
            crate::services::absence_balance::workdays_total(
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
            crate::services::absence_balance::workdays_total(
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
            crate::services::absence_balance::workdays_total(
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
        let flextime_balance_min = if team_member_is_assistant {
            None
        } else {
            let current_year = today.year();
            let overtime_rows =
                build_overtime_rows_for_year(&app_state.pool, team_member.id, current_year).await?;
            Some(
                overtime_rows
                    .last()
                    .map(|r| r.cumulative_min)
                    .unwrap_or(team_member.overtime_start_balance_min),
            )
        };

        // Submission status uses full past weeks, including boundary weeks.
        let weeks_all_submitted = all_weeks_submitted_for_month(
            &app_state.pool,
            team_member.id,
            month_start,
            month_end,
            team_member.start_date,
            team_member_is_assistant,
            team_member.workdays_per_week,
        )
        .await?;

        team_rows.push(TeamRow {
            user_id: team_member.id,
            name: format!("{} {}", team_member.first_name, team_member.last_name),
            target_min: month_report.target_min,
            actual_min: month_report.actual_min,
            diff_min: if team_member_is_assistant {
                None
            } else {
                Some(month_report.diff_min)
            },
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

pub async fn categories(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CategoryQuery>,
) -> AppResult<Json<Vec<CategoryTotal>>> {
    validate_range(query.from, query.to)?;
    // Clamp to today so category reports include current-day entries but no future dates.
    let effective_to = query
        .to
        .min(crate::services::settings::app_today(&app_state.pool).await);
    if query.from > effective_to {
        return Ok(Json(Vec::new()));
    }

    // When no user_id is given: leads see team aggregate. Non-leads must provide
    // user_id explicitly (user-guide: team report scope is leads/admins only).
    let target_user_id = if let Some(uid) = query.user_id {
        assert_can_access_user(&app_state, &requester, uid).await?;
        Some(uid)
    } else if requester.is_lead() {
        None
    } else {
        return Err(AppError::Forbidden);
    };
    // Category breakdown reports include all non-rejected entries regardless of
    // crediting status (user-guide: "not only crediting categories").
    let rows = app_state
        .db
        .reports
        .category_rows_for_scope(
            requester.id,
            requester.is_admin(),
            target_user_id,
            query.from,
            effective_to,
        )
        .await?;
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
    sort_categories_desc(&mut sorted_totals);
    Ok(Json(sorted_totals))
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
    let effective_to = query
        .to
        .min(crate::services::settings::app_today(&app_state.pool).await);
    if query.from > effective_to {
        return Ok(Json(Vec::new()));
    }

    let members = app_state
        .db
        .reports
        .team_category_members(requester.id, requester.is_admin())
        .await?;

    // Same as the individual breakdown: all non-rejected entries up to today,
    // regardless of draft/submitted/approved state or crediting status.
    let rows = app_state
        .db
        .reports
        .team_category_entry_rows(requester.id, requester.is_admin(), query.from, effective_to)
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
    let year = match query.year {
        Some(y) => {
            // Sanity-check the year to prevent unreasonable computation ranges.
            if !(1970..=2100).contains(&y) {
                return Err(AppError::BadRequest("Year out of valid range.".into()));
            }
            y
        }
        None => crate::services::settings::app_current_year(&app_state.pool).await,
    };
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

pub async fn flextime(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<FlextimeQuery>,
) -> AppResult<Json<Vec<FlextimeDay>>> {
    let target_user_id = query.user_id.unwrap_or(requester.id);
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
    validate_range(query.from, query.to)?;

    let user: crate::middleware::auth::User = crate::services::users::repo_user_to_auth_user(
        app_state
            .db
            .users
            .find_by_id(target_user_id)
            .await?
            .ok_or(AppError::NotFound)?,
    );
    // Assistant role is the canonical source for "no flextime account" behavior.
    if is_assistant_role(&user.role) {
        return Ok(Json(vec![]));
    }
    let target_per_day_min = {
        let weekly_hours = user.weekly_hours;
        let workdays_per_week = user.workdays_per_week;
        (weekly_hours / f64::from(workdays_per_week) * 60.0).round() as i64
    };

    // Seed cumulative at query.from-1 via month-level overtime plus a small
    // partial-month report, so per-day flextime processing stays within the
    // requested output range.
    // Fetch today early so the seed clamp below can use it. The flextime balance
    // is defined as "balance at end of yesterday", so seed and main loop alike
    // must never include today's contribution.
    let today = crate::services::settings::app_today(&app_state.pool).await;
    let last_balance_day = today - Duration::days(1);

    let mut cumulative_min = if query.from < user.start_date {
        0
    } else {
        user.overtime_start_balance_min
    };
    if query.from > user.start_date {
        // Cap the seed end at yesterday so today's diff cannot leak into the
        // seeded cumulative when the requested range starts at or after today.
        let day_before_from = (query.from - Duration::days(1)).min(last_balance_day);
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
            let month_seed_report =
                build_range_with_user(&app_state.pool, &user, seed_from, day_before_from, "seed")
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

    let mut approved_crediting_minutes_by_day: HashMap<NaiveDate, i64> = HashMap::new();
    for (entry_date, start_time, end_time, status, counts_as_work) in time_entries_raw {
        if !counts_as_work || status != "approved" {
            continue;
        }
        let minutes =
            (parse_report_time(&end_time)? - parse_report_time(&start_time)?).num_minutes();
        *approved_crediting_minutes_by_day
            .entry(entry_date)
            .or_insert(0) += minutes;
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
            absence_by_day
                .entry(day)
                .or_insert_with(|| absence_kind.clone());
            day += Duration::days(1);
        }
    }

    let language = crate::i18n::load_ui_language(&app_state.pool).await?;

    let holiday_map: HashMap<NaiveDate, String> = app_state
        .db
        .reports
        .holiday_rows(query.from, query.to)
        .await?
        .into_iter()
        .map(|(date, name, local_name)| {
            (
                date,
                crate::i18n::holiday_display_name(&language, name, local_name),
            )
        })
        .collect();

    let absence_removes_target = crate::services::reports::absence_removes_target;
    let is_contract_workday = |date: NaiveDate, wpw: i16| {
        date.weekday().num_days_from_monday() < wpw as u32
    };

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
        // The flextime balance is defined as "up to and including yesterday";
        // today and any future day contribute zero to the cumulative balance.
        let after_today = current_date >= today;
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
            approved_crediting_minutes_by_day
                .get(&current_date)
                .copied()
                .unwrap_or(0)
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
