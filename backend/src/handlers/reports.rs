use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::report_pdf::{render_timesheet_pdf, TimesheetSection};
use crate::roles::is_assistant_role;
use crate::services::reports::{
    all_weeks_submitted_for_month, assert_can_access_user, build_flextime_for_user, build_month,
    build_month_without_submission_status, build_overtime_rows_for_year, build_range,
    build_timesheet_section, csv_response, month_bounds, parse_report_time, pdf_response,
    sort_categories_desc, validate_range, CategoryTotal, FlextimeDay, MonthReport, MonthRow,
    TeamRow, UserCategoryRow,
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

pub async fn range_pdf(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<CsvQuery>,
) -> AppResult<Response> {
    let from = query
        .from
        .ok_or_else(|| AppError::BadRequest("from is required.".into()))?;
    let to = query
        .to
        .ok_or_else(|| AppError::BadRequest("to is required.".into()))?;
    validate_range(from, to)?;
    let label = format!("{}_to_{}", from, to);
    let language = crate::i18n::load_ui_language(&app_state.pool).await?;

    let (sections, file_label) = if let Some(target_user_id) = query.user_id {
        assert_can_access_user(&app_state, &requester, target_user_id).await?;
        let user = crate::services::users::repo_user_to_auth_user(
            app_state
                .db
                .users
                .find_by_id(target_user_id)
                .await?
                .ok_or(AppError::NotFound)?,
        );
        let section = build_timesheet_section(&app_state.pool, &user, from, to, &label).await?;
        (vec![section], format!("user-{}-{}", target_user_id, label))
    } else {
        // Omitting user_id requests the combined "All" export — leads/admins
        // only, scoped to their active team (mirrors the `categories` handler's
        // "omit user_id => team scope for leads" auth pattern).
        if !requester.is_lead() {
            return Err(AppError::Forbidden);
        }
        let team_members: Vec<User> = app_state
            .db
            .reports
            .active_team_members(requester.id, requester.is_admin())
            .await?
            .into_iter()
            // Pure-admin users (tracks_time=false) have no own tracking dataset
            // and are excluded, same as the team report.
            .filter(|team_member| team_member.tracks_time)
            .map(crate::services::users::repo_user_to_auth_user)
            .collect();

        // Fetch each member's data sequentially and merge into one combined PDF
        // — keeps backend load comparable to the original per-employee export
        // flow and avoids opening many concurrent report queries at once.
        let mut sections = Vec::with_capacity(team_members.len());
        for team_member in &team_members {
            sections.push(
                build_timesheet_section(&app_state.pool, team_member, from, to, &label).await?,
            );
        }
        (sections, format!("team-{}", label))
    };

    let bytes = render_timesheet_pdf(&sections, from, to, &language);
    pdf_response(bytes, &file_label)
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

    // Spawn one Tokio task per team member so all per-user DB round-trips
    // run concurrently.  This avoids O(N×k) sequential latency when the pool is
    // under load (e.g. during integration tests running in parallel).
    let handles: Vec<_> = team_members
        .into_iter()
        .map(|team_member| {
            let pool = app_state.pool.clone();
            let query_month = query.month.clone();
            tokio::spawn(async move {
                let team_member_is_assistant = is_assistant_role(&team_member.role);
                let month_report =
                    build_month_without_submission_status(&pool, team_member.id, &query_month)
                        .await?;

                let absence_count_start = month_start.max(team_member.start_date);

                let vacation_taken = if absence_count_start <= vacation_taken_end {
                    crate::services::absence_balance::vacation_workdays(
                        &pool,
                        team_member.id,
                        absence_count_start,
                        vacation_taken_end,
                    )
                    .await?
                } else {
                    0.0
                };

                let vacation_planned_start_user =
                    vacation_planned_start.max(team_member.start_date);
                let vacation_planned = if vacation_planned_start_user <= month_end {
                    crate::services::absence_balance::vacation_workdays(
                        &pool,
                        team_member.id,
                        vacation_planned_start_user,
                        month_end,
                    )
                    .await?
                } else {
                    0.0
                };

                let sick_end = today.min(month_end);
                let sick_workdays = if absence_count_start <= sick_end {
                    crate::services::absence_balance::auto_approve_workdays(
                        &pool,
                        team_member.id,
                        absence_count_start,
                        sick_end,
                    )
                    .await?
                } else {
                    0.0
                };

                let flextime_balance_min = if team_member_is_assistant {
                    None
                } else {
                    let overtime_rows =
                        build_overtime_rows_for_year(&pool, team_member.id, today.year()).await?;
                    Some(
                        overtime_rows
                            .last()
                            .map(|r| r.cumulative_min)
                            .unwrap_or(team_member.overtime_start_balance_min),
                    )
                };

                let weeks_all_submitted = all_weeks_submitted_for_month(
                    &pool,
                    team_member.id,
                    month_start,
                    month_end,
                    team_member.start_date,
                    team_member_is_assistant,
                    team_member.workdays_per_week,
                )
                .await?;

                Ok::<TeamRow, AppError>(TeamRow {
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
                })
            })
        })
        .collect();

    // Await handles in spawn order (preserves team_members ordering).
    // On error, abort any not-yet-awaited handles so they release their DB
    // connections instead of running detached until the pool times out.
    let mut result: AppResult<Vec<TeamRow>> = Ok(Vec::with_capacity(handles.len()));
    for (i, handle) in handles.into_iter().enumerate() {
        if result.is_err() {
            handle.abort();
            continue;
        }
        match handle.await {
            Ok(Ok(row)) => result.as_mut().unwrap().push(row),
            Ok(Err(e)) => result = Err(e),
            Err(_) => result = Err(AppError::Internal(format!("team report task {i} panicked"))),
        }
    }

    Ok(Json(result?))
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
    let flextime_days =
        build_flextime_for_user(&app_state.pool, &user, query.from, query.to).await?;
    Ok(Json(flextime_days))
}
