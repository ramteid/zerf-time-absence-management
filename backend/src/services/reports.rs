use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::middleware::auth::User;
use crate::roles::is_assistant_role;
use crate::time_calc;
use crate::AppState;
use axum::{http::header, response::Response};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use serde::Serialize;
use std::collections::HashMap;

/// True when an absence of this kind removes the day's work target (i.e. the
/// day "costs nothing" toward the flextime balance because the user was off).
/// The lookup is performed against the absence_categories table, with a small
/// cache built once per report build to avoid round-tripping for every day.
pub fn absence_removes_target(category_lookup: &AbsenceCategoryFlags, kind: &str) -> bool {
    category_lookup
        .by_slug
        .get(kind)
        .map(|flags| !flags.is_flextime_cost)
        // Unknown slug (rare: category was deleted under live data) → default
        // to "removes target" so we err on the side of giving the user the day
        // off rather than penalising flextime for missing metadata.
        .unwrap_or(true)
}

/// Per-category behavior fields indexed by slug. Currently only the
/// flextime-cost flag is needed by the reports pipeline — the vacation-cost
/// case is handled SQL-side by `vacation_workdays_total_filtered`.
pub struct AbsenceCategoryFlags {
    pub by_slug: std::collections::HashMap<String, CategoryFlagSet>,
}

pub struct CategoryFlagSet {
    /// True when the category's cost_type is `'flextime'` (i.e. an approved
    /// absence keeps the day's work target rather than removing it).
    pub is_flextime_cost: bool,
}

impl AbsenceCategoryFlags {
    pub async fn load(pool: &crate::db::DatabasePool) -> AppResult<Self> {
        let categories = crate::repository::AbsenceCategoryDb::new(pool.clone())
            .behavior_map()
            .await?;
        let mut by_slug = std::collections::HashMap::with_capacity(categories.len());
        for category in categories {
            let is_flextime_cost = category.is_flextime_cost();
            by_slug.insert(category.slug, CategoryFlagSet { is_flextime_cost });
        }
        Ok(Self { by_slug })
    }
}

/// Verify that `requester` is allowed to read data for `target_uid`.
/// Admins may access any user. Non-admin leads may only access their direct
/// reports (users whose `approver_id` matches the lead's id). Every user may
/// always access their own data.
///
/// Additionally, targets must be active users with time tracking enabled.
/// Pure-admin accounts (tracks_time=false) and inactive users have no
/// reportable personal dataset. Pure-admin requesters may still access active
/// time-tracking users' reports as admins.
pub async fn assert_can_access_user(
    app_state: &AppState,
    requester: &User,
    target_uid: i64,
) -> AppResult<()> {
    crate::services::users::assert_can_access_user(app_state, requester, target_uid).await?;
    let target_user = app_state
        .db
        .users
        .find_by_id(target_uid)
        .await?
        .ok_or(AppError::NotFound)?;

    // Reports describe a user's own tracked working time. Pure-admin accounts
    // and inactive users do not have an active reportable dataset, even when the
    // requester is an admin who can otherwise read the account.
    if !target_user.active || !target_user.tracks_time {
        return Err(AppError::Forbidden);
    }

    Ok(())
}

/// Active team members that have a reportable personal time dataset.
///
/// Users with time tracking disabled, such as technical admin accounts, are
/// intentionally omitted from team reports and combined timesheet PDFs.
pub async fn active_reportable_team_members(
    app_state: &AppState,
    requester: &User,
) -> AppResult<Vec<User>> {
    Ok(app_state
        .db
        .reports
        .active_team_members(requester.id, requester.is_admin())
        .await?
        .into_iter()
        .filter(|team_member| team_member.tracks_time)
        .map(crate::services::users::repo_user_to_auth_user)
        .collect())
}

pub fn month_bounds(month_str: &str) -> AppResult<(NaiveDate, NaiveDate)> {
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
    let last_day = crate::time_calc::last_day_of_month(year, month_num);
    let to = NaiveDate::from_ymd_opt(year, month_num, last_day)
        .ok_or_else(|| AppError::BadRequest("date".into()))?;
    Ok((from, to))
}

#[derive(Serialize)]
pub struct DayDetail {
    pub date: NaiveDate,
    pub weekday: String,
    pub entries: Vec<EntryDetail>,
    pub actual_min: i64,
    pub target_min: i64,
    /// Absence category slug (`vacation`, `sick`, or an admin-created slug).
    /// The frontend resolves this against the `absenceCategories` store to
    /// look up the display name and color.
    pub absence: Option<String>,
    /// Absence category stored display name. Required by the backend PDF
    /// renderer (which has no access to the frontend store) so that custom
    /// admin categories print with their real name rather than the raw slug.
    pub absence_name: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weeks_all_approved: Option<bool>,
    /// Status of the calendar week containing `today`, but only when `today`
    /// falls inside this month. One of `draft | partial | submitted | approved
    /// | rejected`, mirroring the frontend `weekStatus` helper exactly. `None`
    /// when the report does not cover today (past months) or for assistants.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_week_status: Option<String>,
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

/// Determine whether a date belongs to the user's potential workday pool.
///
/// For 1-5 day schedules this is intentionally Mon-Fri (flexible day
/// placement), not a fixed Mon..N mapping.
fn is_contract_workday(date: NaiveDate, workdays_per_week: i16) -> bool {
    crate::time_calc::is_potential_workday(date, workdays_per_week)
}

/// Calculate the per-day target based on weekly hours and the potential
/// workday pool size.
fn target_minutes_per_day(weekly_hours: f64, workdays_per_week: i16) -> i64 {
    let base_days = crate::time_calc::potential_workdays_per_week(workdays_per_week);
    if base_days == 0 {
        return 0;
    }
    (weekly_hours / f64::from(base_days) * 60.0).round() as i64
}

/// Loads the auto-break configuration from the database.
/// Returns `Some(rules)` when the feature is enabled and at least tier-1 is valid,
/// or `None` when the feature is off. Rules are sorted ascending by threshold so the
/// highest applicable rule can be found by scanning from the end.
async fn load_auto_break_config(
    pool: &crate::db::DatabasePool,
) -> AppResult<Option<Vec<(i64, i64)>>> {
    use crate::services::settings::{
        AUTO_BREAK_DEDUCTION_MINUTES_2_KEY, AUTO_BREAK_DEDUCTION_MINUTES_KEY,
        AUTO_BREAK_ENABLED_KEY, AUTO_BREAK_THRESHOLD_HOURS_2_KEY, AUTO_BREAK_THRESHOLD_HOURS_KEY,
    };
    let enabled = crate::services::settings::load_setting(pool, AUTO_BREAK_ENABLED_KEY, "false")
        .await?
        == "true";
    if !enabled {
        return Ok(None);
    }
    let threshold1_str =
        crate::services::settings::load_setting(pool, AUTO_BREAK_THRESHOLD_HOURS_KEY, "").await?;
    let deduction1_str =
        crate::services::settings::load_setting(pool, AUTO_BREAK_DEDUCTION_MINUTES_KEY, "").await?;
    let (Some(t1), Some(d1)) = (
        threshold1_str.parse::<f64>().ok().filter(|&t| t > 0.0),
        deduction1_str.parse::<i64>().ok().filter(|&d| d > 0),
    ) else {
        return Ok(None);
    };
    let mut rules: Vec<(i64, i64)> = vec![(exclusive_threshold_minutes(t1), d1)];

    // Optional tier-2: only added when both fields are valid.
    let threshold2_str =
        crate::services::settings::load_setting(pool, AUTO_BREAK_THRESHOLD_HOURS_2_KEY, "").await?;
    let deduction2_str =
        crate::services::settings::load_setting(pool, AUTO_BREAK_DEDUCTION_MINUTES_2_KEY, "")
            .await?;
    if let (Some(t2), Some(d2)) = (
        threshold2_str.parse::<f64>().ok().filter(|&t| t > 0.0),
        deduction2_str.parse::<i64>().ok().filter(|&d| d > 0),
    ) {
        rules.push((exclusive_threshold_minutes(t2), d2));
        // Ensure ascending threshold order for correct highest-rule selection.
        rules.sort_by_key(|(threshold, _)| *threshold);
    }

    Ok(Some(rules))
}

fn exclusive_threshold_minutes(threshold_hours: f64) -> i64 {
    const FLOAT_ROUNDING_EPSILON: f64 = 1e-9;
    (threshold_hours * 60.0 + FLOAT_ROUNDING_EPSILON).floor() as i64
}

pub async fn build_range_with_user(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    from: NaiveDate,
    to: NaiveDate,
    label: &str,
) -> AppResult<MonthReport> {
    let user_id = user.id;
    // Role is the canonical source for fixed-target behavior. Assistants never
    // have target minutes, even if legacy/imported data contains non-zero hours.
    let target_per_day_min = if is_assistant_role(&user.role) {
        0
    } else {
        target_minutes_per_day(user.weekly_hours, user.workdays_per_week)
    };
    let today = crate::services::settings::app_today(pool).await;

    // Load auto-break config once for this report; None means feature is off.
    let auto_break_cfg = load_auto_break_config(pool).await?;

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

    let approved_absence_rows: Vec<(i64, NaiveDate, NaiveDate, String, String)> =
        reports_db.approved_absence_rows(user_id, from, to).await?;

    // Per-build category flag lookup — used once per day to decide whether an
    // approved absence removes that day's work target (vacation, sick, ...) or
    // keeps it (flextime reduction).
    let category_flags = AbsenceCategoryFlags::load(pool).await?;

    let language = i18n::load_ui_language(pool).await.unwrap_or_default();

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
        let active_absence = approved_absence_rows
            .iter()
            .find(|(_, abs_start, abs_end, _, _)| {
                current_date >= *abs_start && current_date <= *abs_end
            });
        let absence = active_absence.map(|(_, _, _, kind, _)| kind.clone());
        let absence_name = active_absence.map(|(_, _, _, _, name)| name.clone());
        let before_start = current_date < user.start_date;
        let after_today = current_date > today;

        // A day has a work target when it is a weekday within the user's contract,
        // not covered by a holiday or absence, and not in the future.
        let absence_blocks_target = absence
            .as_deref()
            .map(|kind| absence_removes_target(&category_flags, kind))
            .unwrap_or(false);
        let is_workday = is_contract_workday(current_date, user.workdays_per_week)
            && holiday.is_none()
            && !absence_blocks_target
            && !before_start;
        let target = if is_workday && !after_today {
            target_per_day_min
        } else {
            0
        };
        // full_month_target counts all contract workdays without the "capped at today" cutoff.
        let full_target = if is_workday { target_per_day_min } else { 0 };

        let mut entries: Vec<EntryDetail> = vec![];
        let mut actual = 0i64;
        let mut submitted = 0i64;
        // Times for approved and submitted crediting entries, used for block-aware break deduction.
        let mut approved_times: Vec<(NaiveTime, NaiveTime)> = vec![];
        let mut submitted_times: Vec<(NaiveTime, NaiveTime)> = vec![];
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
            ) in entries_by_date.get(&current_date).into_iter().flatten()
            {
                if status == "rejected" {
                    continue;
                }
                // Defensive: surface a 500 on malformed time strings rather than panicking.
                // The DB schema does not constrain the text format.
                let t_start = parse_report_time(start_time)?;
                let t_end = parse_report_time(end_time)?;
                let entry_minutes = (t_end - t_start).num_minutes();
                // Actual work uses approved, crediting entries only.
                if *counts_as_work && status == "approved" {
                    actual += entry_minutes;
                    approved_times.push((t_start, t_end));
                }
                // submitted_min includes submitted + approved (everything the employee filed).
                if *counts_as_work && (status == "approved" || status == "submitted") {
                    submitted += entry_minutes;
                    submitted_times.push((t_start, t_end));
                }
                // Category totals include every non-rejected entry regardless of
                // whether the category is crediting (user-guide: "Category
                // breakdowns show booked non-rejected time entries in scope").
                // Rejected entries were already skipped by the `continue` above.
                *category_minutes_by_name
                    .entry(category_name.clone())
                    .or_insert(0) += entry_minutes;
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

        // Apply automatic break deduction: merge adjacent crediting entries into
        // continuous blocks and apply the highest applicable tier's deduction.
        let break_deduction = auto_break_cfg
            .as_deref()
            .map(|rules| time_calc::compute_day_auto_break(&approved_times, rules))
            .unwrap_or(0);
        let submitted_break_deduction = auto_break_cfg
            .as_deref()
            .map(|rules| time_calc::compute_day_auto_break(&submitted_times, rules))
            .unwrap_or(0);
        actual = (actual - break_deduction).max(0);
        submitted = (submitted - submitted_break_deduction).max(0);

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
            absence_name,
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
        weeks_all_approved: None,
        current_week_status: None,
    })
}

/// Build the per-day flextime ledger for an already-resolved user across
/// `from..=to`. This is the data behind the `/reports/flextime` endpoint,
/// factored out so the timesheet PDF can reuse the exact same seeding and
/// accumulation logic for potentially many users within a single request
/// without going through the HTTP layer.
pub async fn build_flextime_for_user(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<FlextimeDay>> {
    // Assistant role is the canonical source for "no flextime account" behavior.
    if is_assistant_role(&user.role) {
        return Ok(vec![]);
    }
    let target_user_id = user.id;
    let target_per_day_min = {
        let weekly_hours = user.weekly_hours;
        let base_days = crate::time_calc::potential_workdays_per_week(user.workdays_per_week);
        if base_days == 0 {
            0
        } else {
            (weekly_hours / f64::from(base_days) * 60.0).round() as i64
        }
    };

    let reports_db = crate::repository::ReportDb::new(pool.clone());

    // Seed cumulative at `from - 1` via month-level overtime plus a small
    // partial-month report, so per-day flextime processing stays within the
    // requested output range.
    // Fetch today early so the seed clamp below can use it. The flextime balance
    // is defined as "balance at end of yesterday", so seed and main loop alike
    // must never include today's contribution.
    let today = crate::services::settings::app_today(pool).await;
    let last_balance_day = today - Duration::days(1);

    let mut cumulative_min = if from < user.start_date {
        0
    } else {
        user.overtime_start_balance_min
    };
    if from > user.start_date {
        // Cap the seed end at yesterday so today's diff cannot leak into the
        // seeded cumulative when the requested range starts at or after today.
        let day_before_from = (from - Duration::days(1)).min(last_balance_day);
        let month_start =
            NaiveDate::from_ymd_opt(day_before_from.year(), day_before_from.month(), 1)
                .ok_or_else(|| AppError::BadRequest("date".into()))?;

        let cumulative_before_month = if month_start <= user.start_date {
            user.overtime_start_balance_min
        } else {
            let previous_month_end = month_start - Duration::days(1);
            cumulative_at_month_end(
                pool,
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
                build_range_with_user(pool, user, seed_from, day_before_from, "seed").await?;
            cumulative_min = cumulative_before_month + month_seed_report.diff_min;
        } else {
            cumulative_min = cumulative_before_month;
        }
    }

    let auto_break_cfg = load_auto_break_config(pool).await?;

    let time_entries_raw = reports_db
        .flextime_entries(target_user_id, from, to)
        .await?;

    // Accumulate per-day minutes and entry times (for block-aware break deduction).
    let mut approved_by_day: HashMap<NaiveDate, (i64, Vec<(NaiveTime, NaiveTime)>)> =
        HashMap::new();
    for (entry_date, start_time, end_time, status, counts_as_work) in time_entries_raw {
        if !counts_as_work || status != "approved" {
            continue;
        }
        let t_start = parse_report_time(&start_time)?;
        let t_end = parse_report_time(&end_time)?;
        let minutes = (t_end - t_start).num_minutes();
        let entry = approved_by_day.entry(entry_date).or_insert((0, vec![]));
        entry.0 += minutes;
        entry.1.push((t_start, t_end));
    }

    // Apply automatic break deduction per day.
    let approved_crediting_minutes_by_day: HashMap<NaiveDate, i64> = approved_by_day
        .into_iter()
        .map(|(date, (raw_minutes, times))| {
            let deduction = auto_break_cfg
                .as_deref()
                .map(|rules| time_calc::compute_day_auto_break(&times, rules))
                .unwrap_or(0);
            (date, (raw_minutes - deduction).max(0))
        })
        .collect();

    let approved_absences = reports_db
        .approved_absence_rows(target_user_id, from, to)
        .await?;

    // Category flag lookup so each day can decide whether an approved absence
    // removes that day's work target.
    let category_flags = AbsenceCategoryFlags::load(pool).await?;

    // Expand absence ranges into a per-day map so each day can look up its kind in O(1).
    // If two absences overlap (possible via race/manual DB), prioritize the one that removes target.
    let mut absence_by_day: HashMap<NaiveDate, String> = HashMap::new();
    for (_absence_id, absence_start, absence_end, absence_kind, _absence_name) in approved_absences {
        let mut day = absence_start.max(from);
        while day <= absence_end.min(to) {
            let existing = absence_by_day.get(&day);
            let should_replace = match existing {
                None => true,
                Some(existing_kind) => {
                    let new_removes = absence_removes_target(&category_flags, &absence_kind);
                    let old_removes = absence_removes_target(&category_flags, existing_kind);
                    new_removes && !old_removes
                }
            };
            if should_replace {
                absence_by_day.insert(day, absence_kind.clone());
            }
            day += Duration::days(1);
        }
    }

    let language = i18n::load_ui_language(pool).await.unwrap_or_default();

    let holiday_map: HashMap<NaiveDate, String> = reports_db
        .holiday_rows(from, to)
        .await?
        .into_iter()
        .map(|(date, name, local_name)| {
            (
                date,
                i18n::holiday_display_name(&language, name, local_name),
            )
        })
        .collect();

    let mut flextime_days = vec![];
    let mut current_date = from;
    while current_date <= to {
        // Inject the configured overtime start balance on the user's first day
        // when the requested range begins before that date.
        if current_date == user.start_date && from < user.start_date {
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
            .map(|kind| absence_removes_target(&category_flags, kind))
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
    Ok(flextime_days)
}

pub async fn build_range(
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
    let user = crate::services::users::repo_user_to_auth_user(repo_user);
    build_range_with_user(pool, &user, from, to, label).await
}

/// Collects the Monday of every fully elapsed week (Sunday < today) that overlaps the given month.
pub fn complete_weeks_in_month(
    month_start: NaiveDate,
    month_end: NaiveDate,
    today: NaiveDate,
) -> Vec<NaiveDate> {
    let first_monday = crate::time_calc::week_monday(month_start);
    let last_monday = crate::time_calc::week_monday(month_end);
    let mut mondays = Vec::new();
    let mut current = first_monday;
    while current <= last_monday {
        if current + Duration::days(6) < today {
            mondays.push(current);
        }
        current += Duration::days(7);
    }
    mondays
}

/// Fetches holidays, absent days, submitted dates, and incomplete dates for the
/// range covered by `complete_week_mondays`. Assumes the slice is non-empty.
///
/// `include_requested_absences` controls whether `requested` (pending) absences
/// are included in the absent-day set:
///  - `true` for the user-facing completeness nag and submission reminders: an
///    employee cannot log entries on days covered by a pending absence, so those
///    days must be excused to avoid an unsatisfiable requirement.
///  - `false` for the PDF-export gate: a pending absence means the month is not
///    yet settled; exporting it would produce a PDF where pending days show as
///    0-hour rows with a full daily target (unexplained deficit), because the
///    content side (`approved_absence_rows`) only shows finalized absences.
pub async fn load_week_check_data(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    complete_week_mondays: &[NaiveDate],
    include_requested_absences: bool,
) -> AppResult<(
    std::collections::HashSet<NaiveDate>,
    std::collections::HashSet<NaiveDate>,
    std::collections::HashSet<NaiveDate>,
    std::collections::HashSet<NaiveDate>,
)> {
    let check_from = complete_week_mondays[0];
    let check_to = *complete_week_mondays.last().unwrap() + Duration::days(6);
    let reports_db = crate::repository::ReportDb::new(pool.clone());
    let holiday_set = reports_db.holiday_set(check_from, check_to).await?;
    let absence_rows = if include_requested_absences {
        reports_db
            .absence_ranges_in_period(user_id, check_from, check_to)
            .await?
    } else {
        reports_db
            .finalized_absence_ranges_in_period(user_id, check_from, check_to)
            .await?
    };
    let category_flags = AbsenceCategoryFlags::load(pool).await?;
    let absent_days = expand_absence_date_set(&absence_rows, check_from, check_to, &category_flags);
    let submitted_dates = reports_db
        .submitted_dates_in_range(user_id, check_from, check_to)
        .await?;
    let incomplete_dates = reports_db
        .incomplete_dates_in_range(user_id, check_from, check_to)
        .await?;
    Ok((holiday_set, absent_days, submitted_dates, incomplete_dates))
}

async fn load_export_week_check_data(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    complete_week_mondays: &[NaiveDate],
    month_start: NaiveDate,
    month_end: NaiveDate,
) -> AppResult<(
    std::collections::HashSet<NaiveDate>,
    std::collections::HashSet<NaiveDate>,
    std::collections::HashSet<NaiveDate>,
    std::collections::HashSet<NaiveDate>,
)> {
    let check_from = complete_week_mondays[0];
    let check_to = *complete_week_mondays.last().unwrap() + Duration::days(6);
    let reports_db = crate::repository::ReportDb::new(pool.clone());
    let holiday_set = reports_db.holiday_set(check_from, check_to).await?;
    let mut absence_rows = reports_db
        .finalized_absence_ranges_in_period(user_id, check_from, check_to)
        .await?;
    let outside_month_absences = reports_db
        .absence_ranges_in_period(user_id, check_from, check_to)
        .await?
        .into_iter()
        .filter(|(start_date, end_date, _)| *start_date < month_start || *end_date > month_end);
    absence_rows.extend(outside_month_absences);
    let category_flags = AbsenceCategoryFlags::load(pool).await?;
    let absent_days = expand_absence_date_set(&absence_rows, check_from, check_to, &category_flags);
    let submitted_dates = reports_db
        .submitted_dates_in_range(user_id, check_from, check_to)
        .await?;
    let incomplete_dates = reports_db
        .incomplete_dates_in_range(user_id, check_from, check_to)
        .await?;
    Ok((holiday_set, absent_days, submitted_dates, incomplete_dates))
}

/// Returns true when every week in `complete_week_mondays` is considered submitted.
#[allow(clippy::too_many_arguments)]
pub fn check_weeks_all_submitted(
    complete_week_mondays: &[NaiveDate],
    holiday_set: &std::collections::HashSet<NaiveDate>,
    absent_days: &std::collections::HashSet<NaiveDate>,
    submitted_dates: &std::collections::HashSet<NaiveDate>,
    incomplete_dates: &std::collections::HashSet<NaiveDate>,
    user_start_date: NaiveDate,
    workdays_per_week: i16,
) -> bool {
    for &week_monday in complete_week_mondays {
        let has_incomplete = (0..7i64).any(|d| {
            let day = week_monday + Duration::days(d);
            if day < user_start_date {
                return false;
            }
            incomplete_dates.contains(&day)
        });
        if has_incomplete {
            return false;
        }

        let mut potential_days = 0i64;
        let mut covered_days = 0i64;
        for d in 0..7i64 {
            let day = week_monday + Duration::days(d);
            if !crate::time_calc::is_potential_workday(day, workdays_per_week) {
                continue;
            }
            potential_days += 1;

            if day < user_start_date
                || holiday_set.contains(&day)
                || absent_days.contains(&day)
                || submitted_dates.contains(&day)
            {
                covered_days += 1;
            }
        }

        let required_days = i64::from(workdays_per_week.max(0)).min(potential_days);
        if covered_days < required_days {
            return false;
        }
    }
    true
}

/// Returns `(all_submitted, all_approved)` for fully elapsed weeks in the month.
pub async fn submission_status_for_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month_start: NaiveDate,
    month_end: NaiveDate,
    user_start_date: NaiveDate,
    submission_exempt: bool,
    workdays_per_week: i16,
) -> AppResult<(bool, bool)> {
    // Assistants and zero-weekly-hours users are exempt: they have no fixed
    // target schedule / no booking obligation, and the monthly submission
    // reminder never fires for them either (see `roles::has_submission_obligation`).
    if submission_exempt {
        return Ok((true, true));
    }
    let today = crate::services::settings::app_today(pool).await;
    let complete_week_mondays = complete_weeks_in_month(month_start, month_end, today);
    if complete_week_mondays.is_empty() {
        return Ok((true, true));
    }
    let check_from = complete_week_mondays[0];
    let check_to = *complete_week_mondays.last().unwrap() + Duration::days(6);
    // Include requested absences: the employee cannot log entries on pending
    // absence days, so those days must be excused to prevent an unsatisfiable
    // completeness requirement in the user-facing Submissions tile.
    let (holiday_set, absent_days, submitted_dates, incomplete_dates) =
        load_week_check_data(pool, user_id, &complete_week_mondays, true).await?;
    if !check_weeks_all_submitted(
        &complete_week_mondays,
        &holiday_set,
        &absent_days,
        &submitted_dates,
        &incomplete_dates,
        user_start_date,
        workdays_per_week,
    ) {
        return Ok((false, false));
    }
    let reports_db = crate::repository::ReportDb::new(pool.clone());
    let has_pending = reports_db
        .has_pending_submitted_entries_in_range(user_id, check_from, check_to)
        .await?;
    Ok((true, !has_pending))
}

/// Mirrors the frontend `weekStatus` helper (frontend/src/lib/domain/time.js)
/// exactly so the Dashboard/Reports tiles can't disagree with the Zeiterfassung
/// view. Returns one of: `draft | partial | submitted | approved | rejected`.
pub fn compute_current_week_status(
    has_draft: bool,
    has_submitted: bool,
    has_approved: bool,
    has_rejected: bool,
) -> &'static str {
    let any_non_draft = has_submitted || has_approved || has_rejected;
    if !has_draft && !any_non_draft {
        return "draft"; // no entries at all
    }
    if has_draft {
        return if any_non_draft { "partial" } else { "draft" };
    }
    if has_approved && !has_submitted && !has_rejected {
        return "approved";
    }
    if has_submitted {
        return "submitted";
    }
    if has_rejected && !has_approved && !has_submitted {
        return "rejected";
    }
    "partial"
}

/// Returns the current week's status as a string only when `today` falls inside
/// the report's month range. `None` for past/future months and for users without
/// a submission obligation (assistants and zero-weekly-hours non-assistants).
/// Mirrors `submission_exempt` from `submission_status_for_month` so the
/// dashboard's "current week open" warning is suppressed for the same users the
/// reminder system already exempts.
pub async fn current_week_status(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month_start: NaiveDate,
    month_end: NaiveDate,
    submission_exempt: bool,
) -> AppResult<Option<String>> {
    if submission_exempt {
        return Ok(None);
    }
    let today = crate::services::settings::app_today(pool).await;
    if today < month_start || today > month_end {
        return Ok(None);
    }
    let week_monday = crate::time_calc::week_monday(today);
    let week_sunday = week_monday + Duration::days(6);
    let reports_db = crate::repository::ReportDb::new(pool.clone());
    let (has_draft, has_submitted, has_approved, has_rejected) = reports_db
        .week_status_flags(user_id, week_monday, week_sunday)
        .await?;
    Ok(Some(
        compute_current_week_status(has_draft, has_submitted, has_approved, has_rejected)
            .to_string(),
    ))
}

pub async fn build_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month: &str,
) -> AppResult<MonthReport> {
    let (from, to) = month_bounds(month)?;
    let repo_user = crate::repository::UserDb::new(pool.clone())
        .find_by_id(user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let user = crate::services::users::repo_user_to_auth_user(repo_user);
    let submission_exempt = !crate::roles::has_submission_obligation(&user.role, user.weekly_hours);
    let mut report = build_range_with_user(pool, &user, from, to, month).await?;
    let (all_submitted, all_approved) = submission_status_for_month(
        pool,
        user_id,
        from,
        to,
        user.start_date,
        submission_exempt,
        user.workdays_per_week,
    )
    .await?;
    report.weeks_all_submitted = Some(all_submitted);
    report.weeks_all_approved = Some(all_approved);
    // Pass the same submission_exempt flag used for month-level checks, so
    // zero-weekly-hours users are exempted from the current-week status nag
    // just as they are exempt from submission reminders and month-level checks.
    report.current_week_status =
        current_week_status(pool, user_id, from, to, submission_exempt).await?;
    Ok(report)
}

pub async fn build_month_without_submission_status(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month: &str,
) -> AppResult<MonthReport> {
    let (from, to) = month_bounds(month)?;
    build_range(pool, user_id, from, to, month).await
}

pub fn validate_range(from: NaiveDate, to: NaiveDate) -> AppResult<()> {
    if from > to {
        return Err(AppError::BadRequest("from must not be after to.".into()));
    }
    // Inclusive range length = diff + 1. Max inclusive 366 days => diff <= 365.
    if (to - from).num_days() > 365 {
        return Err(AppError::BadRequest(
            "Date range must not exceed 366 days.".into(),
        ));
    }
    Ok(())
}

pub fn csv_response(r: MonthReport, uid: i64, file_label: &str) -> AppResult<Response> {
    // CSV formula-injection guard: bypass via leading spaces "  =cmd" – trim spaces then check.
    fn safe(s: &str) -> String {
        let trimmed_spaces = s.trim_start_matches(' ');
        if trimmed_spaces.starts_with(['=', '+', '-', '@', '\t', '\r']) {
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
    // Use the already break-adjusted actual_min from the report rather than
    // re-summing raw entry minutes, so the CSV Total matches the UI and the
    // documented auto-break deduction behaviour.
    let csv_total_min = r.actual_min;
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
                    // Use the stored category display name rather than the raw slug so admin-
                    // created categories (whose slugs are normalized identifiers like
                    // "comp_time_q3") render with their real name in the export. Seeded
                    // categories already store their canonical English name ("Vacation",
                    // "Sick Leave", ...) — consistent with how the day's weekday is
                    // exported in English regardless of UI language.
                    safe(&day.absence_name.clone().unwrap_or_default()),
                    safe(&day.holiday.clone().unwrap_or_default()),
                ])
                .map_err(csv_err)?;
        } else {
            for entry in &day.entries {
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
                        // Use the stored category display name rather than the raw slug so admin-
                        // created categories (whose slugs are normalized identifiers like
                        // "comp_time_q3") render with their real name in the export. Seeded
                        // categories already store their canonical English name ("Vacation",
                        // "Sick Leave", ...) — consistent with how the day's weekday is
                        // exported in English regardless of UI language.
                        safe(&day.absence_name.clone().unwrap_or_default()),
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
    let content_type = axum::http::HeaderValue::from_str("text/csv; charset=utf-8")
        .map_err(|_| AppError::Internal("Failed to build CSV content-type header.".into()))?;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
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

/// Build an HTTP file-download response for a generated timesheet PDF.
/// `file_label` becomes the download filename verbatim (the caller assembles
/// it, e.g. `user-{id}-{range}` or `team-{range}`); it is sanitised the same
/// way `csv_response` sanitises its label so the header stays well-formed.
pub fn pdf_response(bytes: Vec<u8>, file_label: &str) -> AppResult<Response> {
    let mut response = Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/pdf"),
    );
    let safe_label: String = file_label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(60)
        .collect();
    let content_disposition = format!("attachment; filename=\"report-{}.pdf\"", safe_label);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&content_disposition).unwrap_or_else(|_| {
            axum::http::HeaderValue::from_static("attachment; filename=\"report.pdf\"")
        }),
    );
    Ok(response)
}

/// One account's approved absence usage for a member in the selected month.
#[derive(Serialize, Clone)]
pub struct LeaveAccountUsage {
    pub category_id: i64,
    pub taken_days: f64,
    pub planned_days: f64,
}

/// Metadata for one dynamically generated leave-account column.
#[derive(Serialize, Clone)]
pub struct LeaveAccountCategory {
    pub category_id: i64,
    pub name: String,
    pub color: String,
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
    /// Diff = actual - target for the report month. None for assistants.
    pub diff_min: Option<i64>,
    /// Usage is keyed by account category id so duplicate names cannot merge
    /// unrelated account values in clients.
    pub leave_account_usage: Vec<LeaveAccountUsage>,
    /// Sick working-days in the report month.
    pub sick_days: f64,
    /// Cumulative flextime balance at the end of the report month (or up to
    /// and including yesterday for the current month). None for assistants.
    pub flextime_balance_min: Option<i64>,
    /// True if all fully elapsed weeks (Sunday < today) overlapping the report month
    /// have been fully submitted.
    pub weeks_all_submitted: bool,
}

/// Explicitly structured team-report response. The client receives category
/// metadata once and binds every cell through `category_id`.
#[derive(Serialize)]
pub struct TeamReport {
    pub leave_account_categories: Vec<LeaveAccountCategory>,
    pub rows: Vec<TeamRow>,
}

#[derive(Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub color: String,
    pub minutes: i64,
}

pub fn parse_report_time(raw: &str) -> AppResult<NaiveTime> {
    time_calc::parse_stored_time(raw)
}

// Type alias for the 8-field tuple stored per time entry after stripping the date.
type RawEntryRow = (
    NaiveDate,
    String,
    String,
    String,
    String,
    i64,
    bool,
    String,
    Option<String>,
);

type RawEntryTuple = (
    String,
    String,
    String,
    String,
    i64,
    bool,
    String,
    Option<String>,
);

/// Pre-groups raw time entry rows (as fetched from the DB) by date.
/// Allows O(1) per-day lookup instead of scanning the full list for each day.
pub fn group_entries_by_date(rows: Vec<RawEntryRow>) -> HashMap<NaiveDate, Vec<RawEntryTuple>> {
    let mut map: HashMap<NaiveDate, Vec<RawEntryTuple>> = HashMap::new();
    for (date, start, end, category, color, cat_id, counts_as_work, status, comment) in rows {
        map.entry(date).or_default().push((
            start,
            end,
            category,
            color,
            cat_id,
            counts_as_work,
            status,
            comment,
        ));
    }
    map
}

/// Expands a list of (start, end, slug) date ranges into a flat set of individual
/// dates clamped to `[from, to]`.
///
/// All absence statuses that block time-entry creation (requested, approved,
/// cancellation_pending) are included — including flextime-cost (flextime-reduction)
/// absences. Although a flextime-reduction day keeps its work target in the overtime
/// ledger, the user cannot log entries on it (`validate_entry` rejects any entry
/// on a day covered by a non-auto-approve absence). Excluding flextime-cost days
/// would make those weeks permanently unsubmittable: the completeness check would
/// demand entries that the entry validator forbids. The `absence_removes_target`
/// function (used in reporting) is separate and still correctly reports that
/// flextime-cost days keep their target.
///
/// `category_flags` is retained as a parameter to avoid a breaking signature change
/// and to allow future per-category overrides; it is currently unused.
pub fn expand_absence_date_set(
    ranges: &[(NaiveDate, NaiveDate, String)],
    from: NaiveDate,
    to: NaiveDate,
    _category_flags: &AbsenceCategoryFlags,
) -> std::collections::HashSet<NaiveDate> {
    let mut set = std::collections::HashSet::new();
    for (range_start, range_end, _kind) in ranges {
        let mut day = (*range_start).max(from);
        while day <= (*range_end).min(to) {
            set.insert(day);
            day += Duration::days(1);
        }
    }
    set
}

/// Sorts category totals descending by minutes, then ascending by name.
pub fn sort_categories_desc(cats: &mut [CategoryTotal]) {
    cats.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.category.cmp(&b.category))
    });
}

#[derive(Serialize)]
pub struct MonthRow {
    pub month: String,
    pub target_min: i64,
    pub actual_min: i64,
    pub diff_min: i64,
    pub cumulative_min: i64,
    /// Cumulative balance including submitted-but-not-yet-approved entries.
    pub submitted_cumulative_min: i64,
}

pub async fn build_overtime_rows_for_year(
    pool: &crate::db::DatabasePool,
    target_user_id: i64,
    year: i32,
) -> AppResult<Vec<MonthRow>> {
    let user = crate::repository::UserDb::new(pool.clone())
        .find_by_id(target_user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    // Assistant role is the canonical source for "no flextime account" behavior.
    if is_assistant_role(&user.role) {
        return Ok(vec![]);
    }

    let today = crate::services::settings::app_today(pool).await;
    let current_year = today.year();
    // Cap the loop so future months (with zero actuals but full targets) do not
    // produce large artificial deficits in the cumulative balance.
    let max_month: u32 = if year < current_year {
        12
    } else if year == current_year {
        today.month()
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
    let mut submitted_cumulative_min = overtime_start_balance_min;
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
            submitted_cumulative_min += month_report.submitted_min - month_report.target_min;
        }
    }

    let last_balance_day = today - Duration::days(1);
    for month_num in first_month_in_year..=max_month {
        let month_label = format!("{:04}-{:02}", year, month_num);
        let is_current_month = year == current_year && month_num == today.month();
        // The flextime balance is defined as "up to and including yesterday".
        // For the current month, build the report from month-start to yesterday
        // so today's diff is not included in the balance. Past months are
        // unaffected and use the regular full-month build.
        let month_report = if is_current_month {
            let (month_start, month_end) = month_bounds(&month_label)?;
            let cutoff = month_end.min(last_balance_day);
            if cutoff < month_start {
                // Today is the 1st of the month: no balance contribution yet.
                MonthReport {
                    user_id: target_user_id,
                    month: month_label.clone(),
                    days: vec![],
                    target_min: 0,
                    actual_min: 0,
                    diff_min: 0,
                    submitted_min: 0,
                    full_month_target_min: 0,
                    category_totals: HashMap::new(),
                    weeks_all_submitted: None,
                    weeks_all_approved: None,
                    current_week_status: None,
                }
            } else {
                build_range(pool, target_user_id, month_start, cutoff, &month_label).await?
            }
        } else {
            build_month_without_submission_status(pool, target_user_id, &month_label).await?
        };
        cumulative_min += month_report.diff_min;
        submitted_cumulative_min += month_report.submitted_min - month_report.target_min;
        month_rows.push(MonthRow {
            month: month_label,
            target_min: month_report.target_min,
            actual_min: month_report.actual_min,
            diff_min: month_report.diff_min,
            cumulative_min,
            submitted_cumulative_min,
        });
    }

    Ok(month_rows)
}

pub async fn cumulative_at_month_end(
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

    let today = crate::services::settings::app_today(pool).await;
    let current_year = today.year();
    let current_month = today.month();

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

/// Checks whether all fully elapsed working weeks overlapping the given month
/// have been submitted for the user.
pub async fn all_weeks_submitted_for_month(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month_start: NaiveDate,
    month_end: NaiveDate,
    user_start_date: NaiveDate,
    submission_exempt: bool,
    workdays_per_week: i16,
) -> AppResult<bool> {
    let today = crate::services::settings::app_today(pool).await;
    let complete_week_mondays = complete_weeks_in_month(month_start, month_end, today);
    if complete_week_mondays.is_empty() {
        return Ok(true);
    }
    // Assistants and zero-weekly-hours users have no fixed target schedule /
    // no booking obligation and no mandatory day-level submission (see
    // `roles::has_submission_obligation`).
    if submission_exempt {
        return Ok(true);
    }
    let (holiday_set, absent_days, submitted_dates, incomplete_dates) =
        load_week_check_data(pool, user_id, &complete_week_mondays, true).await?;
    Ok(check_weeks_all_submitted(
        &complete_week_mondays,
        &holiday_set,
        &absent_days,
        &submitted_dates,
        &incomplete_dates,
        user_start_date,
        workdays_per_week,
    ))
}

/// Checks whether a month is settled enough for the immutable timesheet PDF archive.
/// Pending absence requests hold this gate because PDF content only includes
/// finalized absences.
pub async fn all_weeks_ready_for_timesheet_export(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    month_start: NaiveDate,
    month_end: NaiveDate,
    user_start_date: NaiveDate,
    submission_exempt: bool,
    workdays_per_week: i16,
) -> AppResult<bool> {
    let today = crate::services::settings::app_today(pool).await;
    let complete_week_mondays = complete_weeks_in_month(month_start, month_end, today);
    if complete_week_mondays.is_empty() {
        return Ok(true);
    }
    if submission_exempt {
        return Ok(true);
    }

    let reports_db = crate::repository::ReportDb::new(pool.clone());
    if reports_db
        .has_requested_absences_in_period(user_id, month_start, month_end)
        .await?
    {
        return Ok(false);
    }

    let (holiday_set, absent_days, submitted_dates, incomplete_dates) =
        load_export_week_check_data(
            pool,
            user_id,
            &complete_week_mondays,
            month_start,
            month_end,
        )
        .await?;
    Ok(check_weeks_all_submitted(
        &complete_week_mondays,
        &holiday_set,
        &absent_days,
        &submitted_dates,
        &incomplete_dates,
        user_start_date,
        workdays_per_week,
    ))
}

/// Why a user's month is (not) settled enough to be exported.
///
/// Shared by every scheduled monthly export — the per-employee timesheet PDF
/// upload and the payroll report email — so both judge "this month is final"
/// by exactly the same rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonthExportReadiness {
    /// Everything in the period is decided; the month can be exported.
    Ready,
    /// Stored time entries or absences fall before the user's current start
    /// date, so the renderer would silently hide them from the export. A
    /// start-date correction moved backwards without fixing the underlying
    /// data, or the data was never fixed after an earlier correction.
    PreStartContent,
    /// An archived / tracking-disabled account still has draft, submitted, or
    /// unresolved rejected entries that nobody can settle from the app.
    UnresolvedTimeEntries,
    /// An undecided absence request overlaps the period.
    PendingAbsenceRequests,
    /// At least one fully elapsed week of the period is not submitted.
    WeeksNotSubmitted,
    /// The month's weeks are submitted, but at least one entry is still
    /// draft, submitted, or unresolved-rejected — i.e. not yet approved.
    /// Only checked when the caller requires full approval (see
    /// `month_export_readiness`'s `require_full_approval` parameter).
    UnapprovedTimeEntries,
}

impl MonthExportReadiness {
    pub fn is_ready(self) -> bool {
        matches!(self, MonthExportReadiness::Ready)
    }
}

/// Evaluate the shared month-finality gate for one user.
///
/// Historical-only accounts (archived, or time tracking switched off) cannot
/// submit anything any more, so missing workdays are a legitimate historical
/// shape for them — only undecided time-entry rows block their export. Everyone
/// else must have all elapsed weeks submitted and no pending absence request in
/// the period, because both would change the exported content afterwards.
///
/// `require_full_approval` additionally requires every time entry in the
/// period to be approved, not merely submitted. The timesheet PDF export and
/// the payroll report both need this — the PDF Total row and the payroll
/// hours count only approved, crediting minutes, so a month that is only
/// submitted would archive/report too few hours until an approver catches up.
pub async fn month_export_readiness(
    pool: &crate::db::DatabasePool,
    user: &crate::repository::User,
    from: NaiveDate,
    to: NaiveDate,
    require_full_approval: bool,
) -> AppResult<MonthExportReadiness> {
    let reports_db = crate::repository::ReportDb::new(pool.clone());

    // Universal: the renderer hides everything before the user's current
    // start date, so stored rows there would silently vanish from the export.
    if reports_db
        .has_report_content_before_start_date(user.id, from, to, user.start_date)
        .await?
    {
        return Ok(MonthExportReadiness::PreStartContent);
    }

    let is_historical_only = user.archived_at.is_some() || !user.active || !user.tracks_time;
    if is_historical_only {
        if reports_db
            .has_unresolved_time_entries_in_range(user.id, from, to)
            .await?
        {
            return Ok(MonthExportReadiness::UnresolvedTimeEntries);
        }
        return Ok(MonthExportReadiness::Ready);
    }

    // Pending absences block export regardless of submission obligation – a payroll-relevant
    // requested absence means the month is not settled, even for assistants/zero-hour users.
    if reports_db
        .has_requested_absences_in_period(user.id, from, to)
        .await?
    {
        return Ok(MonthExportReadiness::PendingAbsenceRequests);
    }

    let submission_exempt = !crate::roles::has_submission_obligation(&user.role, user.weekly_hours);
    let submitted = all_weeks_ready_for_timesheet_export(
        pool,
        user.id,
        from,
        to,
        user.start_date,
        submission_exempt,
        user.workdays_per_week,
    )
    .await?;
    if !submitted {
        return Ok(MonthExportReadiness::WeeksNotSubmitted);
    }

    if require_full_approval
        && reports_db
            .has_unresolved_time_entries_in_range(user.id, from, to)
            .await?
    {
        return Ok(MonthExportReadiness::UnapprovedTimeEntries);
    }

    Ok(MonthExportReadiness::Ready)
}

#[derive(Serialize)]
pub struct UserCategoryRow {
    pub user_id: i64,
    pub name: String,
    pub categories: Vec<CategoryTotal>,
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

/// Build a single [`TimesheetSection`] (range report + flextime ledger) for one user.
/// Used by both the on-demand PDF handler and the scheduled monthly export.
pub async fn build_timesheet_section(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    from: NaiveDate,
    to: NaiveDate,
    label: &str,
) -> AppResult<crate::report_pdf::TimesheetSection> {
    let report = build_range_with_user(pool, user, from, to, label).await?;
    let flextime_data = build_flextime_for_user(pool, user, from, to).await?;
    Ok(crate::report_pdf::TimesheetSection {
        user_name: format!("{} {}", user.first_name, user.last_name),
        report,
        flextime_data,
    })
}

/// Build the ordered timesheet sections for a lead/admin's combined "All
/// employees" PDF export: every active member the requester can see (pure-admin
/// users are excluded — they have no own tracking dataset), grouped by role
/// (team lead, employee, assistant, admin) and alphabetical within each role.
///
/// Member selection and ordering are business rules, so they live here in the
/// service layer rather than in the HTTP handler.
pub async fn build_team_timesheet_sections(
    app_state: &crate::AppState,
    requester: &User,
    from: NaiveDate,
    to: NaiveDate,
    label: &str,
) -> AppResult<Vec<crate::report_pdf::TimesheetSection>> {
    let mut team_members = active_reportable_team_members(app_state, requester).await?;
    // Group sections by role (team lead, employee, assistant, admin), same as
    // every user roster in the frontend — stable sort preserves the
    // repository's last_name/first_name/id order within each role.
    team_members.sort_by_key(|team_member| crate::roles::role_sort_rank(&team_member.role));

    // Fetch each member's data sequentially and merge into one combined PDF —
    // keeps backend load comparable to the per-employee export flow and avoids
    // opening many concurrent report queries at once.
    let mut sections = Vec::with_capacity(team_members.len());
    for team_member in &team_members {
        sections
            .push(build_timesheet_section(&app_state.pool, team_member, from, to, label).await?);
    }
    Ok(sections)
}

/// Re-queue the monthly timesheet export for `(user_id, date)` pairs whenever
/// the report upload feature is enabled. Idempotent via `ON CONFLICT DO
/// NOTHING`: if the entry is still pending from a previous queue population it
/// is left untouched.
///
/// Called after any mutation that can change the content of a past month
/// (approval, rejection, admin time-entry edit, reopen approval, absence status
/// changes). The caller passes every `(user_id, date)` pair that was affected;
/// this function groups them into `YYYY-MM` periods and inserts one queue entry
/// per distinct (user_id, period).
///
/// No-op when the upload feature is disabled so the queue does not accumulate
/// stale entries on installations that never use Nextcloud upload.
pub async fn requeue_export_for_dates(
    pool: &crate::db::DatabasePool,
    user_date_pairs: &[(i64, NaiveDate)],
) {
    requeue_export_for_dates_with_start_date_review(pool, user_date_pairs, false).await;
}

async fn requeue_export_for_dates_with_start_date_review(
    pool: &crate::db::DatabasePool,
    user_date_pairs: &[(i64, NaiveDate)],
    requires_start_date_review: bool,
) {
    if user_date_pairs.is_empty() {
        return;
    }
    // Check whether upload is enabled at all; bail out early to avoid the cost
    // of grouping when the feature is off.
    let enabled = crate::services::settings::load_setting(
        pool,
        crate::services::settings::REPORT_UPLOAD_ENABLED_KEY,
        "false",
    )
    .await
    .map(|v| v == "true")
    .unwrap_or(false);
    if !enabled {
        return;
    }

    let today = crate::services::settings::app_today(pool).await;

    // Group into distinct (user_id, YYYY-MM) pairs, excluding the current and
    // future months (those have not been archived yet so nothing to re-queue).
    let mut pairs: std::collections::HashMap<i64, std::collections::HashSet<(i32, u32)>> =
        std::collections::HashMap::new();
    for &(user_id, date) in user_date_pairs {
        // Only past months can have been exported already.
        let period_month_end = NaiveDate::from_ymd_opt(date.year(), date.month(), {
            crate::time_calc::last_day_of_month(date.year(), date.month())
        });
        if period_month_end.map(|end| end < today).unwrap_or(false) {
            pairs
                .entry(user_id)
                .or_default()
                .insert((date.year(), date.month()));
        }
    }

    let user_db = crate::repository::UserDb::new(pool.clone());
    let export_queue_db = crate::repository::TimesheetExportQueueDb::new(pool.clone());
    for (user_id, periods) in pairs {
        match user_db.find_by_id(user_id).await {
            Ok(Some(_user)) => {}
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    target: "zerf::reports",
                    "requeue_export_for_dates: failed to load user {user_id}: {e}"
                );
                continue;
            }
        }
        for (year, month) in periods {
            let period = format!("{year:04}-{month:02}");
            let result = if requires_start_date_review {
                export_queue_db
                    .populate_requiring_start_date_review(&period, &[user_id])
                    .await
            } else {
                export_queue_db.populate(&period, &[user_id]).await
            };
            if let Err(e) = result {
                tracing::warn!(
                    target: "zerf::reports",
                    "requeue_export_for_dates: failed to re-queue user {} period {}: {e}",
                    user_id,
                    period
                );
            }
        }
    }
}

/// Re-queue past months whose generated PDF can change after a user's start
/// date changes. Moving the date forward marks those rows for review so the
/// uploader cannot overwrite an archived PDF with a partial-month rendering.
pub async fn requeue_export_for_start_date_change(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    previous_start_date: NaiveDate,
    new_start_date: NaiveDate,
) {
    if previous_start_date == new_start_date {
        return;
    }

    let (range_start, range_end, requires_start_date_review) =
        if new_start_date > previous_start_date {
            (
                previous_start_date,
                new_start_date - Duration::days(1),
                true,
            )
        } else {
            (
                new_start_date,
                previous_start_date - Duration::days(1),
                false,
            )
        };

    requeue_export_for_date_range_with_start_date_review(
        pool,
        user_id,
        range_start,
        range_end,
        requires_start_date_review,
    )
    .await;
}

/// Re-queue every past-month timesheet touched by an absence date range.
pub async fn requeue_export_for_absence_period(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    start_date: NaiveDate,
    end_date: NaiveDate,
) {
    if start_date > end_date {
        return;
    }
    requeue_export_for_date_range_with_start_date_review(
        pool, user_id, start_date, end_date, false,
    )
    .await;
}

async fn requeue_export_for_date_range_with_start_date_review(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    start_date: NaiveDate,
    end_date: NaiveDate,
    requires_start_date_review: bool,
) {
    if start_date > end_date {
        return;
    }
    let mut pairs = Vec::new();
    let mut day = start_date;
    while day <= end_date {
        pairs.push((user_id, day));
        day += Duration::days(1);
    }
    requeue_export_for_dates_with_start_date_review(pool, &pairs, requires_start_date_review).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::collections::HashSet;

    #[test]
    fn absence_removes_target_keeps_flextime_reduction_as_exception() {
        let mut by_slug = std::collections::HashMap::new();
        by_slug.insert(
            "vacation".to_string(),
            CategoryFlagSet {
                is_flextime_cost: false,
            },
        );
        by_slug.insert(
            "sick".to_string(),
            CategoryFlagSet {
                is_flextime_cost: false,
            },
        );
        by_slug.insert(
            "flextime_reduction".to_string(),
            CategoryFlagSet {
                is_flextime_cost: true,
            },
        );
        let flags = AbsenceCategoryFlags { by_slug };
        assert!(absence_removes_target(&flags, "vacation"));
        assert!(absence_removes_target(&flags, "sick"));
        assert!(!absence_removes_target(&flags, "flextime_reduction"));
        // Unknown slug — defensive fallback returns "removes target" so users
        // don't lose flextime to a missing-metadata bug.
        assert!(absence_removes_target(&flags, "mystery"));
    }

    #[test]
    fn month_bounds_parses_and_validates_inputs() {
        let (from, to) = month_bounds("2026-02").unwrap();
        assert_eq!(from, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(to, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());

        let (dec_from, dec_to) = month_bounds("2024-12").unwrap();
        assert_eq!(dec_from, NaiveDate::from_ymd_opt(2024, 12, 1).unwrap());
        assert_eq!(dec_to, NaiveDate::from_ymd_opt(2024, 12, 31).unwrap());

        assert!(month_bounds("2026/02").is_err());
        assert!(month_bounds("x-02").is_err());
        assert!(month_bounds("2026-99").is_err());
    }

    #[test]
    fn weekday_and_contract_workday_follow_iso_week_rules() {
        let monday = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let friday = NaiveDate::from_ymd_opt(2026, 5, 8).unwrap();
        let saturday = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();

        assert_eq!(weekday_en(monday), "Monday");
        assert_eq!(weekday_en(friday), "Friday");
        assert!(is_contract_workday(monday, 5));
        assert!(is_contract_workday(friday, 5));
        assert!(!is_contract_workday(saturday, 5));
        assert!(is_contract_workday(friday, 4));
    }

    #[test]
    fn target_minutes_per_day_uses_weekly_hours_divided_by_workdays() {
        assert_eq!(target_minutes_per_day(40.0, 5), 480);
        assert_eq!(target_minutes_per_day(40.0, 4), 480);
        assert_eq!(target_minutes_per_day(37.5, 5), 450);
    }

    #[test]
    fn current_week_status_empty_week_is_draft() {
        assert_eq!(
            compute_current_week_status(false, false, false, false),
            "draft"
        );
    }

    #[test]
    fn current_week_status_only_drafts_is_draft() {
        assert_eq!(
            compute_current_week_status(true, false, false, false),
            "draft"
        );
    }

    #[test]
    fn current_week_status_draft_plus_submitted_is_partial() {
        assert_eq!(
            compute_current_week_status(true, true, false, false),
            "partial"
        );
    }

    #[test]
    fn current_week_status_only_approved_is_approved() {
        assert_eq!(
            compute_current_week_status(false, false, true, false),
            "approved"
        );
    }

    #[test]
    fn current_week_status_any_submitted_dominates_approved() {
        assert_eq!(
            compute_current_week_status(false, true, true, false),
            "submitted"
        );
    }

    #[test]
    fn current_week_status_only_rejected_is_rejected() {
        assert_eq!(
            compute_current_week_status(false, false, false, true),
            "rejected"
        );
    }

    #[test]
    fn current_week_status_rejected_plus_approved_is_partial() {
        assert_eq!(
            compute_current_week_status(false, false, true, true),
            "partial"
        );
    }

    #[test]
    fn complete_weeks_in_month_includes_only_fully_elapsed_weeks() {
        let month_start = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let month_end = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 5, 20).unwrap();

        let mondays = complete_weeks_in_month(month_start, month_end, today);
        assert_eq!(
            mondays,
            vec![
                NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
                NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
                NaiveDate::from_ymd_opt(2026, 5, 11).unwrap(),
            ]
        );
    }

    #[test]
    fn check_weeks_all_submitted_handles_submitted_and_excused_weeks() {
        let monday = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let complete_week_mondays = vec![monday];
        let mut submitted_dates = HashSet::new();
        for offset in 0..5 {
            submitted_dates.insert(monday + Duration::days(offset));
        }

        assert!(check_weeks_all_submitted(
            &complete_week_mondays,
            &HashSet::new(),
            &HashSet::new(),
            &submitted_dates,
            &HashSet::new(),
            NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            5,
        ));

        submitted_dates.remove(&(monday + Duration::days(4)));
        assert!(!check_weeks_all_submitted(
            &complete_week_mondays,
            &HashSet::new(),
            &HashSet::new(),
            &submitted_dates,
            &HashSet::new(),
            NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            5,
        ));

        let holiday_set: HashSet<NaiveDate> = (0..5).map(|d| monday + Duration::days(d)).collect();
        assert!(check_weeks_all_submitted(
            &complete_week_mondays,
            &holiday_set,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            5,
        ));

        let mut incomplete = HashSet::new();
        incomplete.insert(monday + Duration::days(2));
        assert!(!check_weeks_all_submitted(
            &complete_week_mondays,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &incomplete,
            NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            5,
        ));
    }

    #[test]
    fn validate_range_checks_order_and_max_window() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let ok_to = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        assert!(validate_range(from, ok_to).is_ok());

        assert!(validate_range(ok_to, from).is_err());
        let too_far = NaiveDate::from_ymd_opt(2027, 1, 5).unwrap();
        assert!(validate_range(from, too_far).is_err());
    }

    #[test]
    fn parse_and_group_helpers_handle_rows_and_time_parsing() {
        assert_eq!(
            parse_report_time("08:30:00").unwrap(),
            NaiveTime::from_hms_opt(8, 30, 0).unwrap()
        );
        assert!(parse_report_time("bad-time").is_err());

        let day = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        let grouped = group_entries_by_date(vec![
            (
                day,
                "08:00".to_string(),
                "12:00".to_string(),
                "Project".to_string(),
                "#111".to_string(),
                1,
                true,
                "approved".to_string(),
                None,
            ),
            (
                day,
                "13:00".to_string(),
                "17:00".to_string(),
                "Meeting".to_string(),
                "#222".to_string(),
                2,
                false,
                "submitted".to_string(),
                Some("note".to_string()),
            ),
        ]);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped.get(&day).unwrap().len(), 2);
    }

    #[test]
    fn expand_absence_date_set_clamps_to_requested_window() {
        let from = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 5, 12).unwrap();
        let ranges = vec![(
            NaiveDate::from_ymd_opt(2026, 5, 8).unwrap(),
            NaiveDate::from_ymd_opt(2026, 5, 11).unwrap(),
            "vacation".to_string(),
        )];
        let flags = AbsenceCategoryFlags {
            by_slug: Default::default(),
        };
        let set = expand_absence_date_set(&ranges, from, to, &flags);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&NaiveDate::from_ymd_opt(2026, 5, 10).unwrap()));
        assert!(set.contains(&NaiveDate::from_ymd_opt(2026, 5, 11).unwrap()));
    }

    /// `expand_absence_date_set` includes flextime-cost (flextime-reduction) ranges.
    /// Flextime-cost absence days must excuse the daily submission requirement because
    /// `validate_entry` blocks time entries on any day covered by a non-auto-approve
    /// absence (including flextime-cost). Excluding them would make the week
    /// permanently unsubmittable.
    #[test]
    fn expand_absence_date_set_includes_flextime_cost_ranges() {
        let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let ranges = vec![
            (
                NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(),
                NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(),
                "flextime_reduction".to_string(),
            ),
            (
                NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(),
                NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(),
                "vacation".to_string(),
            ),
        ];
        let mut by_slug = std::collections::HashMap::new();
        by_slug.insert(
            "flextime_reduction".to_string(),
            CategoryFlagSet {
                is_flextime_cost: true,
            },
        );
        by_slug.insert(
            "vacation".to_string(),
            CategoryFlagSet {
                is_flextime_cost: false,
            },
        );
        let flags = AbsenceCategoryFlags { by_slug };
        let set = expand_absence_date_set(&ranges, from, to, &flags);
        // Both the flextime_reduction day and the vacation day are included.
        assert_eq!(set.len(), 2);
        assert!(set.contains(&NaiveDate::from_ymd_opt(2026, 5, 5).unwrap()));
        assert!(set.contains(&NaiveDate::from_ymd_opt(2026, 5, 6).unwrap()));
    }

    #[test]
    fn sort_categories_desc_orders_by_minutes_then_name() {
        let mut categories = vec![
            CategoryTotal {
                category: "B".to_string(),
                color: "#2".to_string(),
                minutes: 120,
            },
            CategoryTotal {
                category: "A".to_string(),
                color: "#1".to_string(),
                minutes: 120,
            },
            CategoryTotal {
                category: "C".to_string(),
                color: "#3".to_string(),
                minutes: 30,
            },
        ];
        sort_categories_desc(&mut categories);
        assert_eq!(categories[0].category, "A");
        assert_eq!(categories[1].category, "B");
        assert_eq!(categories[2].category, "C");
    }

    /// `complete_weeks_in_month` returns an empty vec when today is the very
    /// first day of the month (no week can have elapsed yet).
    #[test]
    fn complete_weeks_in_month_returns_empty_when_no_week_elapsed() {
        // May 1, 2026 is a Friday. The first overlapping week starts on
        // April 27 (Monday); its Sunday is May 3. With today = May 1, even
        // that partial week is not yet complete (May 3 >= May 1).
        let month_start = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let month_end = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(); // first of month

        let mondays = complete_weeks_in_month(month_start, month_end, today);
        assert!(
            mondays.is_empty(),
            "expected no complete weeks, got {mondays:?}"
        );
    }

    /// `check_weeks_all_submitted` considers a week fully excused when every
    /// contract workday is before the user's start date.
    #[test]
    fn check_weeks_all_submitted_excuses_week_before_user_start() {
        let monday = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let complete_weeks = vec![monday];
        // User starts on the Monday of the NEXT week.
        let user_start = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
        // No submitted entries, no holidays, no absences, but all workdays are
        // before user_start — the week must be considered excused.
        assert!(check_weeks_all_submitted(
            &complete_weeks,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            user_start,
            5,
        ));
    }

    /// `check_weeks_all_submitted` returns false when a week has no submitted
    /// days and at least one workday is not excused.
    #[test]
    fn check_weeks_all_submitted_returns_false_for_unsubmitted_unexcused_week() {
        let monday = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let complete_weeks = vec![monday];
        let user_start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        assert!(!check_weeks_all_submitted(
            &complete_weeks,
            &HashSet::new(), // no holidays
            &HashSet::new(), // no absences
            &HashSet::new(), // no submitted dates
            &HashSet::new(), // no incomplete dates
            user_start,
            5,
        ));
    }

    #[test]
    fn exclusive_threshold_minutes_preserves_fractional_break_boundaries() {
        assert_eq!(exclusive_threshold_minutes(6.0), 360);
        assert_eq!(exclusive_threshold_minutes(6.01), 360);
        assert_eq!(exclusive_threshold_minutes(6.1), 366);
    }

    /// `validate_range` accepts a single-day range (from == to).
    #[test]
    fn validate_range_accepts_single_day_range() {
        let d = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        assert!(validate_range(d, d).is_ok());
    }

    /// `validate_range` accepts exactly 366 days inclusive (365 diff) – the maximum allowed.
    #[test]
    fn validate_range_accepts_exactly_366_days() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(); // 365 diff = 366 inclusive
        assert_eq!((to - from).num_days(), 365);
        assert!(validate_range(from, to).is_ok());
    }

    /// `month_bounds` handles December correctly (wraps year to January next year).
    #[test]
    fn month_bounds_december_wraps_to_january_next_year() {
        let (from, to) = month_bounds("2026-12").unwrap();
        assert_eq!(from, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        assert_eq!(to, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    }

    /// `expand_absence_date_set` returns an empty set for empty input.
    #[test]
    fn expand_absence_date_set_returns_empty_for_no_ranges() {
        let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let flags = AbsenceCategoryFlags {
            by_slug: Default::default(),
        };
        let set = expand_absence_date_set(&[], from, to, &flags);
        assert!(set.is_empty());
    }

    /// `sort_categories_desc` is stable: equal-minute categories are sorted by
    /// name ascending, preserving a consistent ordering across runs.
    #[test]
    fn sort_categories_desc_with_all_equal_minutes_sorts_by_name() {
        let mut cats = vec![
            CategoryTotal {
                category: "Zebra".to_string(),
                color: "#3".to_string(),
                minutes: 60,
            },
            CategoryTotal {
                category: "Alpha".to_string(),
                color: "#1".to_string(),
                minutes: 60,
            },
            CategoryTotal {
                category: "Mango".to_string(),
                color: "#2".to_string(),
                minutes: 60,
            },
        ];
        sort_categories_desc(&mut cats);
        assert_eq!(cats[0].category, "Alpha");
        assert_eq!(cats[1].category, "Mango");
        assert_eq!(cats[2].category, "Zebra");
    }

    #[tokio::test]
    async fn csv_response_adds_formula_injection_guard_and_headers() {
        let day = DayDetail {
            date: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            weekday: "Friday".to_string(),
            entries: vec![EntryDetail {
                start_time: "08:00".to_string(),
                end_time: "10:00".to_string(),
                category: "=cmd".to_string(),
                color: "#000".to_string(),
                minutes: 120,
                counts_as_work: true,
                status: "approved".to_string(),
                comment: Some("@note".to_string()),
            }],
            actual_min: 120,
            target_min: 480,
            absence: Some("+absence".to_string()),
            absence_name: Some("+absence".to_string()),
            holiday: Some("\tholiday".to_string()),
        };
        let report = MonthReport {
            user_id: 1,
            month: "2026-05".to_string(),
            days: vec![day],
            target_min: 480,
            actual_min: 120,
            diff_min: -360,
            submitted_min: 120,
            full_month_target_min: 480,
            category_totals: HashMap::new(),
            weeks_all_submitted: Some(true),
            weeks_all_approved: Some(true),
            current_week_status: None,
        };

        let response = csv_response(report, 1, "2026/05").unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "text/csv; charset=utf-8"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap()
                .to_str()
                .unwrap(),
            "attachment; filename=\"report-user-1-202605.csv\""
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(body.starts_with(&[0xEF, 0xBB, 0xBF]));

        let mut reader = csv::Reader::from_reader(&body[3..]);
        let rows: Vec<csv::StringRecord> = reader.records().map(|r| r.unwrap()).collect();
        assert_eq!(rows[0].get(4).unwrap(), "'=cmd");
        assert_eq!(rows[0].get(7).unwrap(), "'@note");
        assert_eq!(rows[0].get(8).unwrap(), "'+absence");
        assert_eq!(rows[0].get(9).unwrap(), "'\tholiday");
        assert_eq!(rows[1].get(1).unwrap(), "Total");
        assert_eq!(rows[1].get(5).unwrap(), "120");
    }

    /// Bug B10: `validate_range` is the shared helper used by the flextime
    /// endpoint (replacing the previously inlined duplicate). Verify it rejects
    /// an inverted range and a range that exceeds 366 days — the same edge
    /// cases the inline code guarded against.
    #[test]
    fn validate_range_rejects_inverted_and_too_long_ranges() {
        let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(); // inverted
        assert!(validate_range(from, to).is_err());

        let long_to = NaiveDate::from_ymd_opt(2027, 5, 3).unwrap(); // > 366 days
        assert!((long_to - from).num_days() > 366);
        assert!(validate_range(from, long_to).is_err());
    }

    /// `validate_range` accepts a range that is exactly at the 366-day inclusive boundary (365 diff).
    #[test]
    fn validate_range_accepts_366_day_flextime_window() {
        let from = NaiveDate::from_ymd_opt(2025, 5, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(); // 365 diff = 366 inclusive
        assert_eq!((to - from).num_days(), 365);
        assert!(validate_range(from, to).is_ok());
    }
}
