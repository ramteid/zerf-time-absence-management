use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::report_pdf::render_timesheet_pdf;
use crate::roles::is_assistant_role;
use crate::services::reports::{
    active_reportable_team_members, all_weeks_submitted_for_month, assert_can_access_user,
    build_flextime_for_user, build_month, build_month_without_submission_status,
    build_overtime_rows_for_year, build_range, build_team_timesheet_sections,
    build_timesheet_section, csv_response, month_bounds, parse_report_time, pdf_response,
    sort_categories_desc, validate_range, CategoryTotal, FlextimeDay, LeaveAccountCategory,
    LeaveAccountUsage, MonthReport, MonthRow, TeamReport, TeamRow, UserCategoryRow,
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
use std::sync::Arc;

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
        let sections =
            build_team_timesheet_sections(&app_state, &requester, from, to, &label).await?;
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
) -> AppResult<Json<TeamReport>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }

    // Admins see all active time-tracking users; team leads see themselves and
    // their direct reports when those users track time.
    let team_members = active_reportable_team_members(&app_state, &requester).await?;

    let today = crate::services::settings::app_today(&app_state.pool).await;
    let (month_start, month_end) = month_bounds(&query.month)?;

    // Load account columns and every account-booked absence range in two
    // bounded queries before fan-out. This avoids a user × account query
    // pattern as categories are added over time.
    let leave_account_definitions: Vec<_> = app_state
        .db
        .users
        .list_leave_account_definitions()
        .await?
        .into_iter()
        .filter(|definition| definition.start_year <= month_start.year())
        .collect();
    let account_ids: Vec<_> = leave_account_definitions
        .iter()
        .map(|definition| definition.category_id)
        .collect();
    let team_member_ids: Vec<_> = team_members.iter().map(|member| member.id).collect();
    let account_ranges = if team_member_ids.is_empty() || account_ids.is_empty() {
        Vec::new()
    } else {
        app_state
            .db
            .absences
            .leave_account_absence_ranges_for_users(
                &team_member_ids,
                &account_ids,
                month_start,
                month_end,
            )
            .await?
    };
    let mut account_ranges_by_user: HashMap<(i64, i64), Vec<(NaiveDate, NaiveDate)>> =
        HashMap::new();
    for range in account_ranges {
        account_ranges_by_user
            .entry((range.user_id, range.leave_account_category_id))
            .or_default()
            .push((range.start_date, range.end_date));
    }
    // Sent once at the top level rather than repeated per row — the client
    // binds every row's `leave_account_usage` entries back to a column via
    // `category_id`.
    let leave_account_categories: Vec<LeaveAccountCategory> = leave_account_definitions
        .iter()
        .map(|definition| LeaveAccountCategory {
            category_id: definition.category_id,
            name: definition.category_name.clone(),
            color: definition.color.clone(),
        })
        .collect();
    let leave_account_definitions = Arc::new(leave_account_definitions);
    let account_ranges_by_user = Arc::new(account_ranges_by_user);

    // Taken includes today; planned starts tomorrow.
    let leave_taken_end = today.min(month_end);
    let tomorrow = today + Duration::days(1);
    let leave_planned_start = tomorrow.max(month_start);

    // Spawn one Tokio task per team member so all per-user DB round-trips
    // run concurrently.  A semaphore caps simultaneous DB-holding tasks at 8
    // so a large team cannot exhaust the connection pool even under concurrency.
    let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
    let handles: Vec<_> = team_members
        .into_iter()
        .map(|team_member| {
            let pool = app_state.pool.clone();
            let query_month = query.month.clone();
            let sem = semaphore.clone();
            let account_definitions = leave_account_definitions.clone();
            let account_ranges = account_ranges_by_user.clone();
            tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                let team_member_is_assistant = is_assistant_role(&team_member.role);
                // Submission-completeness exemption also covers zero-weekly-hours
                // non-assistants (matches the monthly reminder's eligibility
                // filter) — unlike `team_member_is_assistant`, which stays
                // role-only for the flextime/overtime fields below.
                let team_member_submission_exempt = !crate::roles::has_submission_obligation(
                    &team_member.role,
                    team_member.weekly_hours,
                );
                let month_report =
                    build_month_without_submission_status(&pool, team_member.id, &query_month)
                        .await?;

                let absence_count_start = month_start.max(team_member.start_date);

                let mut leave_account_usage = Vec::with_capacity(account_definitions.len());
                for definition in account_definitions.iter() {
                    let ranges = account_ranges
                        .get(&(team_member.id, definition.category_id))
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let taken_days = if absence_count_start <= leave_taken_end {
                        crate::services::absence_balance::workdays_for_ranges_in_window(
                            &pool,
                            team_member.id,
                            ranges,
                            absence_count_start,
                            leave_taken_end,
                        )
                        .await?
                    } else {
                        0.0
                    };
                    let planned_start = leave_planned_start.max(team_member.start_date);
                    let planned_days = if planned_start <= month_end {
                        crate::services::absence_balance::workdays_for_ranges_in_window(
                            &pool,
                            team_member.id,
                            ranges,
                            planned_start,
                            month_end,
                        )
                        .await?
                    } else {
                        0.0
                    };
                    leave_account_usage.push(LeaveAccountUsage {
                        category_id: definition.category_id,
                        taken_days,
                        planned_days,
                    });
                }

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
                    // Build the overtime rows for the selected month's year so
                    // the balance reflects the end of the selected period, not
                    // today. For the current month, build_overtime_rows_for_year
                    // already caps at yesterday, matching the "up to and
                    // including yesterday" semantics. For past months the row for
                    // query_month holds the balance at the end of that month.
                    let overtime_rows =
                        build_overtime_rows_for_year(&pool, team_member.id, month_start.year())
                            .await?;
                    Some(
                        overtime_rows
                            .iter()
                            .find(|r| r.month == query_month)
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
                    team_member_submission_exempt,
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
                    leave_account_usage,
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

    Ok(Json(TeamReport {
        leave_account_categories,
        rows: result?,
    }))
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

/// Payroll report status for the dashboard tile: how far the previous month is
/// from being deliverable. Leads only — team leads see the full counts but only
/// their own team members by name (see `services::payroll_report::build_status`).
pub async fn payroll_status(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<crate::services::payroll_report::PayrollStatus>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let language = crate::i18n::load_ui_language(&app_state.pool).await?;
    Ok(Json(
        crate::services::payroll_report::build_status(&app_state, &requester, &language).await?,
    ))
}

/// Returns the list of users whose reports the requester is allowed to access.
/// Scoping mirrors the team report: leads see their direct reports + themselves,
/// admins see all active time-tracking users. Pure-admin users (tracks_time=false)
/// are excluded; inactive users are excluded.
pub async fn report_users(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<User>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let users = active_reportable_team_members(&app_state, &requester).await?;
    Ok(Json(users))
}
