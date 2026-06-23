use crate::error::{AppError, AppResult};
use chrono::{Datelike, Duration, NaiveDate};

/// Count contract workdays in a date range for a specific user.
/// Respects the user's workdays_per_week configuration (1-7 days per week).
/// Excludes public holidays.
pub async fn workdays(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone())
        .workdays_for_user(user_id, from, to)
        .await
}

/// Sum of approved (and cancellation_pending) absence workdays for a specific
/// category. Used by the team report for per-kind columns (vacation taken,
/// sick taken). Callers pass the category id resolved up front so the
/// repository query is a tight indexed lookup.
pub async fn workdays_total_for_category(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    category_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone())
        .workdays_total_for_category(user_id, category_id, from, to)
        .await
}

/// Enforce the backdating window for auto-approve (sick-like) categories.
/// Other categories already have their start date bounded by the user's Zerf
/// start_date and pass through approval; this guard exists specifically to
/// prevent fraudulent retroactive sick leave from skipping review.
pub fn validate_backdating_window(
    category: &crate::repository::AbsenceCategory,
    start_date: NaiveDate,
    today: NaiveDate,
) -> AppResult<()> {
    if !category.auto_approve_past {
        return Ok(());
    }
    let earliest = today - Duration::days(30);
    if start_date < earliest {
        return Err(AppError::BadRequest(
            "Auto-approved absences cannot be backdated more than 30 days.".into(),
        ));
    }
    Ok(())
}

/// Check whether the date range contains at least one effective workday:
/// a day that is both a contract workday (per workdays_per_week) and not a
/// public holiday.
pub fn has_effective_workday(
    start_date: NaiveDate,
    end_date: NaiveDate,
    workdays_per_week: i16,
    holidays: &std::collections::HashSet<NaiveDate>,
) -> bool {
    let mut day = start_date;
    while day <= end_date {
        let is_contract_day =
            Datelike::weekday(&day).num_days_from_monday() < workdays_per_week as u32;
        if is_contract_day && !holidays.contains(&day) {
            return true;
        }
        day += Duration::days(1);
    }
    false
}

/// Validate that the absence range includes at least one effective workday.
pub async fn validate_absence_has_workday(
    pool: &crate::db::DatabasePool,
    workdays_per_week: i16,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> AppResult<()> {
    // Irregular-schedule users have no fixed workdays; allow any range.
    if workdays_per_week == 0 {
        return Ok(());
    }
    let holidays = crate::repository::HolidayDb::new(pool.clone())
        .get_dates_in_range(start_date, end_date)
        .await?;
    if !has_effective_workday(start_date, end_date, workdays_per_week, &holidays) {
        return Err(AppError::BadRequest(
            "Absence must include at least one workday.".into(),
        ));
    }
    Ok(())
}

/// Clamp an arbitrary date range to an inclusive year window.
/// Returns `None` when there is no overlap.
pub fn clamp_range_to_window(
    start_date: NaiveDate,
    end_date: NaiveDate,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Option<(NaiveDate, NaiveDate)> {
    let clamped_start = std::cmp::max(start_date, window_start);
    let clamped_end = std::cmp::min(end_date, window_end);
    (clamped_start <= clamped_end).then_some((clamped_start, clamped_end))
}

/// Sum workdays for a list of date ranges after clamping each range to the
/// provided inclusive window.
pub async fn workdays_for_ranges_in_window(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> AppResult<f64> {
    let mut total = 0.0;
    for (start_date, end_date) in ranges {
        if let Some((clamped_start, clamped_end)) =
            clamp_range_to_window(*start_date, *end_date, window_start, window_end)
        {
            total += workdays(pool, user_id, clamped_start, clamped_end).await?;
        }
    }
    Ok(total)
}

/// The date that anchors annual-leave proration and carryover-source-year
/// iteration: the configured `hire_date` when present, otherwise `start_date`.
///
/// `start_date` doubles as the boundary for time entries/absences and the
/// flextime starting-balance anchor, so it cannot always serve as the
/// employment-start reference too — e.g. when Zerf is introduced to an
/// existing team, an employee's Zerf `start_date` (this year) would otherwise
/// wrongly pro-rate their full-year entitlement. `hire_date` lets admins record
/// the real employment start separately; `None` preserves prior behavior.
pub fn leave_entitlement_anchor(user: &crate::middleware::auth::User) -> NaiveDate {
    user.hire_date.unwrap_or(user.start_date)
}

/// Pro-rate annual leave entitlement for a user who started mid-year.
pub fn pro_rate_entitlement(user_start_date: NaiveDate, year: i32, entitled: i64) -> i64 {
    let year_start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    if user_start_date > year_end {
        0
    } else if user_start_date > year_start {
        let months_remaining = (13 - Datelike::month(&user_start_date)) as f64;
        ((entitled as f64) * months_remaining / 12.0).ceil() as i64
    } else {
        entitled
    }
}

/// Parse the carryover expiry date setting (MM-DD) into a NaiveDate for the given year.
pub fn parse_expiry_date(setting: &str, year: i32) -> Option<NaiveDate> {
    let (month_str, day_str) = setting.split_once('-')?;
    let month: u32 = month_str.parse().ok()?;
    let configured_day: u32 = day_str.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }

    let next_month_start = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    let max_day = Datelike::day(&(next_month_start - Duration::days(1)));
    let effective_day = configured_day.min(max_day);
    NaiveDate::from_ymd_opt(year, month, effective_day)
}

/// Helper: resolve the effective annual leave entitlement for a user in a given year.
pub async fn effective_annual_days(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    year: i32,
) -> AppResult<i64> {
    crate::services::users::get_leave_days(pool, user.id, year).await
}

pub async fn annual_days_or_default(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    year: i32,
    default_days: i64,
) -> AppResult<i64> {
    crate::repository::UserDb::new(pool.clone())
        .annual_days_or_default(user_id, year, default_days)
        .await
}

pub async fn carryover_days_into_year(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    year: i32,
    expiry_setting: &str,
) -> AppResult<i64> {
    // Carryover can only be derived from years Zerf actually recorded usage
    // for, i.e. from `start_date` onward — NOT from `leave_entitlement_anchor`.
    // `hire_date` may anchor entitlement many years before `start_date`; looping
    // from there would "carry over" full entitlements for years with zero
    // recorded usage (Zerf has no data before `start_date`), wildly inflating
    // the result. The entitlement *within* each iterated year still must respect
    // `hire_date` (via `pro_rate_entitlement(anchor, ...)` below) — that is what
    // correctly gives a long-tenured new Zerf user their full (non-prorated)
    // entitlement for their start-date year.
    if year <= Datelike::year(&user.start_date) {
        return Ok(0);
    }

    let anchor = leave_entitlement_anchor(user);
    let absence_db = crate::repository::AbsenceDb::new(pool.clone());
    let mut incoming_carryover = 0;

    for source_year in user.start_date.year()..year {
        let entitled =
            annual_days_or_default(pool, user.id, source_year, user.annual_leave_days).await?;
        let effective_entitlement = pro_rate_entitlement(anchor, source_year, entitled);
        let year_from = NaiveDate::from_ymd_opt(source_year, 1, 1).unwrap();
        let year_to = NaiveDate::from_ymd_opt(source_year, 12, 31).unwrap();
        let expiry_date = parse_expiry_date(expiry_setting, source_year);

        // Carryover source is approved vacation usage. Since absence categories
        // are configurable, "vacation" is no longer a fixed slug — we sum
        // workdays across every category whose cost_type='vacation'.
        let base_usage = if let Some(expiry) = expiry_date {
            let pre_window_end = std::cmp::min(expiry, year_to);
            let post_window_start = expiry + Duration::days(1);
            let pre_usage = if year_from <= pre_window_end {
                absence_db
                    .vacation_workdays_total_filtered(
                        user.id,
                        year_from,
                        pre_window_end,
                        &["approved"],
                    )
                    .await?
            } else {
                0.0
            };
            let post_usage = if post_window_start <= year_to {
                absence_db
                    .vacation_workdays_total_filtered(
                        user.id,
                        post_window_start,
                        year_to,
                        &["approved"],
                    )
                    .await?
            } else {
                0.0
            };
            post_usage + (pre_usage - incoming_carryover as f64).max(0.0)
        } else {
            let total_usage = absence_db
                .vacation_workdays_total_filtered(user.id, year_from, year_to, &["approved"])
                .await?;
            (total_usage - incoming_carryover as f64).max(0.0)
        };

        incoming_carryover = std::cmp::max(0, effective_entitlement - base_usage.round() as i64);
    }

    Ok(incoming_carryover)
}

/// Build a year-level entitlement context.
pub async fn vacation_year_context(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    year: i32,
    today: NaiveDate,
    expiry_setting: &str,
) -> AppResult<(i64, i64, bool)> {
    let entitled = effective_annual_days(pool, user, year).await?;
    let effective_entitlement =
        pro_rate_entitlement(leave_entitlement_anchor(user), year, entitled);
    let carryover_days = carryover_days_into_year(pool, user, year, expiry_setting).await?;

    let expiry_date = parse_expiry_date(expiry_setting, year);
    let carryover_expired = expiry_date.map(|d| today > d).unwrap_or(false);
    Ok((effective_entitlement, carryover_days, carryover_expired))
}

/// Total budget usable in a year according to carryover policy.
pub fn total_entitlement_with_carryover(
    effective_entitlement: i64,
    carryover_days: i64,
    carryover_expired: bool,
) -> f64 {
    if carryover_expired {
        effective_entitlement as f64
    } else {
        effective_entitlement as f64 + carryover_days as f64
    }
}

pub fn total_entitlement_for_dated_vacation(
    effective_entitlement: i64,
    carryover_days: i64,
    expiry_date: Option<NaiveDate>,
    carryover_expired: bool,
) -> f64 {
    if expiry_date.is_some() {
        effective_entitlement as f64 + carryover_days as f64
    } else {
        total_entitlement_with_carryover(effective_entitlement, carryover_days, carryover_expired)
    }
}

pub const VACATION_DAY_EPSILON: f64 = 0.000_001;

pub fn exceeds_vacation_budget(required_days: f64, budget_days: f64) -> bool {
    required_days - budget_days > VACATION_DAY_EPSILON
}

pub async fn approved_vacation_ranges_in_year_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
    exclude_id: Option<i64>,
) -> AppResult<Vec<(NaiveDate, NaiveDate)>> {
    crate::repository::AbsenceDb::approved_vacation_ranges_in_year_tx(
        tx, user_id, from, to, exclude_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn carryover_from_year_into_next_year(
    pool: &crate::db::DatabasePool,
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    year_from: NaiveDate,
    year_to: NaiveDate,
    effective_entitlement: i64,
    carryover_days: i64,
    expiry_date: Option<NaiveDate>,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_id: Option<i64>,
    count_new_for_carryover_source: bool,
) -> AppResult<i64> {
    let mut approved_ranges =
        approved_vacation_ranges_in_year_tx(tx, user_id, year_from, year_to, exclude_id).await?;
    if count_new_for_carryover_source {
        if let Some((new_start, new_end)) =
            clamp_range_to_window(start_date, end_date, year_from, year_to)
        {
            approved_ranges.push((new_start, new_end));
        }
    }

    let base_usage = if let Some(expiry) = expiry_date {
        let pre_window_end = std::cmp::min(expiry, year_to);
        let post_window_start = expiry + Duration::days(1);
        let pre_usage = if year_from <= pre_window_end {
            workdays_for_ranges_in_window(
                pool,
                user_id,
                &approved_ranges,
                year_from,
                pre_window_end,
            )
            .await?
        } else {
            0.0
        };
        let post_usage = if post_window_start <= year_to {
            workdays_for_ranges_in_window(
                pool,
                user_id,
                &approved_ranges,
                post_window_start,
                year_to,
            )
            .await?
        } else {
            0.0
        };
        post_usage + (pre_usage - carryover_days as f64).max(0.0)
    } else {
        let total_usage =
            workdays_for_ranges_in_window(pool, user_id, &approved_ranges, year_from, year_to)
                .await?;
        (total_usage - carryover_days as f64).max(0.0)
    };

    Ok(std::cmp::max(
        0,
        effective_entitlement - base_usage.round() as i64,
    ))
}

/// Compute how much carryover remains in the queried year.
pub struct CarryoverRemainingInput<'a> {
    pub pool: &'a crate::db::DatabasePool,
    pub user_id: i64,
    pub vacation_absences: &'a [crate::services::absences::Absence],
    pub year_start: NaiveDate,
    pub today: NaiveDate,
    pub expiry_date: Option<NaiveDate>,
    pub carryover_days: i64,
    pub carryover_expired: bool,
}

pub async fn carryover_remaining_days(input: CarryoverRemainingInput<'_>) -> AppResult<f64> {
    let CarryoverRemainingInput {
        pool,
        user_id,
        vacation_absences,
        year_start,
        today,
        expiry_date,
        carryover_days,
        carryover_expired,
    } = input;

    if carryover_expired || carryover_days == 0 {
        return Ok(0.0);
    }

    let approved_or_pending_ranges: Vec<(NaiveDate, NaiveDate)> = vacation_absences
        .iter()
        .filter(|absence| absence.status == "approved" || absence.status == "cancellation_pending")
        .map(|absence| (absence.start_date, absence.end_date))
        .collect();
    let consumed = if let Some(expiry) = expiry_date {
        let cutoff = std::cmp::min(expiry, today);
        if cutoff < year_start {
            0.0
        } else {
            workdays_for_ranges_in_window(
                pool,
                user_id,
                &approved_or_pending_ranges,
                year_start,
                cutoff,
            )
            .await?
        }
    } else {
        workdays_for_ranges_in_window(
            pool,
            user_id,
            &approved_or_pending_ranges,
            year_start,
            today,
        )
        .await?
    };

    Ok((carryover_days as f64 - consumed).max(0.0))
}

/// Validate that a flextime-cost absence (cost_type='flextime') does not
/// push the user's flextime balance below the configured floor.
///
/// The check accounts for:
/// 1. The current balance as of end-of-yesterday (from `build_flextime_for_user`).
/// 2. The future-portion cost of OTHER pending/approved/cancellation_pending
///    flextime-cost absences — these are committed deductions that haven't
///    yet been realised in the balance. Without this, multiple requests that
///    each individually fit could be approved together and breach the floor.
/// 3. The future-portion cost of the proposed range itself (past days are
///    already reflected in current_balance).
///
/// `exclude_id` excludes the absence being edited/approved from (2) so it is
/// not double-counted with (3).
pub async fn validate_flextime_balance(
    pool: &crate::db::DatabasePool,
    tx: &mut crate::db::PgConnection,
    user: &crate::middleware::auth::User,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    use crate::repository::AbsenceDb;
    // Assistants have no flextime account; irregular schedules have no fixed target.
    if crate::roles::is_assistant_role(&user.role) || user.workdays_per_week == 0 {
        return Ok(());
    }
    let target_per_day_min =
        (user.weekly_hours / f64::from(user.workdays_per_week) * 60.0).round() as i64;

    let floor_min: i64 =
        crate::services::settings::load_setting(pool, "flextime_min_balance_min", "0")
            .await?
            .parse::<i64>()
            .unwrap_or(0);

    // (1) Current flextime balance = cumulative balance through end-of-yesterday.
    // `build_flextime_for_user(today, today)` seeds cumulative_min with the
    // balance as it stood yesterday, then iterates one day (today). Today's
    // contribution is zero because `after_today` zeroes both `target` and
    // `actual` for today and beyond, so the first row's `cumulative_min`
    // equals the seeded yesterday-balance unchanged.
    let today = crate::services::settings::app_today(pool).await;
    let flextime_days =
        crate::services::reports::build_flextime_for_user(pool, user, today, today).await?;
    let current_balance_min = flextime_days.first().map(|d| d.cumulative_min).unwrap_or(0);

    // (2) Committed-but-not-yet-realised flextime usage from OTHER absences.
    //
    // cost_type='flextime' absences cost `target_per_day_min` per workday
    // because the day keeps its target while the user logs zero hours. Past
    // portions of these absences are ALREADY reflected in current_balance
    // (build_flextime_for_user processed those days with target = target_per_day_min
    // and actual = 0), so we count only the future portion (`max(start, today)`
    // through `end`) to avoid double-charging.
    //
    // Including `requested` and `cancellation_pending` is conservative: a
    // pending request will probably be approved; a cancellation request might
    // not be honoured. Both COULD reduce the future balance, so we treat them
    // as committed for safety. The `exclude_id` skips the absence we're
    // validating right now (it would otherwise count itself in step 2 AND in
    // step 3 below).
    let committed_ranges =
        AbsenceDb::flextime_cost_ranges_after_tx(tx, user.id, today, exclude_id).await?;
    let mut committed_cost_min: i64 = 0;
    for (range_start, range_end) in &committed_ranges {
        let effective_start = std::cmp::max(*range_start, today);
        if effective_start > *range_end {
            // Range was entirely in the past — already in current_balance, skip.
            continue;
        }
        let days = workdays(pool, user.id, effective_start, *range_end).await?;
        committed_cost_min += (days * target_per_day_min as f64).round() as i64;
    }

    // (3) Future portion of the proposed range. Same reasoning as (2): days
    // before today were already counted in current_balance with the target
    // preserved (because cost_type='flextime' never removes the target),
    // so approving/creating the absence doesn't add NEW cost for those days.
    // Only the future portion of the new range is a fresh deduction.
    let proposed_start = std::cmp::max(start_date, today);
    let proposed_cost_min = if proposed_start > end_date {
        // Entirely backdated — no new future cost. The check below then
        // reduces to "current_balance - committed_cost >= floor", verifying
        // that already-pending commitments don't already breach the floor.
        0
    } else {
        let days = workdays(pool, user.id, proposed_start, end_date).await?;
        (days * target_per_day_min as f64).round() as i64
    };

    if current_balance_min - committed_cost_min - proposed_cost_min < floor_min {
        return Err(AppError::BadRequest(
            "Not enough flextime balance for this absence.".into(),
        ));
    }
    Ok(())
}

/// Validate that a vacation absence does not exceed the user's remaining entitlement
/// for the affected year(s). `exclude_id` allows excluding the current absence when
/// editing (pass `None` when creating).
pub async fn validate_vacation_balance(
    pool: &crate::db::DatabasePool,
    tx: &mut crate::db::PgConnection,
    user: &crate::middleware::auth::User,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_id: Option<i64>,
    count_new_for_carryover_source: bool,
) -> AppResult<()> {
    use crate::repository::AbsenceDb;

    let year = start_date.year();
    let year_from = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let year_to = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let today = crate::services::settings::app_today(pool).await;
    let expiry_setting =
        crate::services::settings::load_setting(pool, "carryover_expiry_date", "03-31").await?;
    let (effective_entitlement, carryover_days, carryover_expired) =
        vacation_year_context(pool, user, year, today, &expiry_setting).await?;
    let expiry_date = parse_expiry_date(&expiry_setting, year);
    let total_year_budget = total_entitlement_for_dated_vacation(
        effective_entitlement,
        carryover_days,
        expiry_date,
        carryover_expired,
    );

    // Sum existing vacation usage (requested + approved) in this year, excluding `exclude_id`.
    let existing_ranges =
        AbsenceDb::vacation_ranges_in_year_tx(&mut *tx, user.id, year_from, year_to, exclude_id)
            .await?;
    let used_days =
        workdays_for_ranges_in_window(pool, user.id, &existing_ranges, year_from, year_to).await?;
    // Clamp the new absence to this year and check whether adding it would exceed the budget.
    let new_days = if let Some((new_start, new_end)) =
        clamp_range_to_window(start_date, end_date, year_from, year_to)
    {
        workdays(pool, user.id, new_start, new_end).await?
    } else {
        0.0
    };
    if exceeds_vacation_budget(used_days + new_days, total_year_budget) {
        return Err(AppError::BadRequest(
            "Not enough remaining vacation days.".into(),
        ));
    }

    // Enforce carryover expiry by absence date, not request/approval date.
    if let Some(expiry) = expiry_date {
        let pre_window_end = std::cmp::min(expiry, year_to);
        let post_window_start = expiry + Duration::days(1);

        let pre_existing_days = if year_from <= pre_window_end {
            workdays_for_ranges_in_window(
                pool,
                user.id,
                &existing_ranges,
                year_from,
                pre_window_end,
            )
            .await?
        } else {
            0.0
        };
        let pre_new_days = if year_from <= pre_window_end {
            if let Some((pre_new_start, pre_new_end)) =
                clamp_range_to_window(start_date, end_date, year_from, pre_window_end)
            {
                workdays(pool, user.id, pre_new_start, pre_new_end).await?
            } else {
                0.0
            }
        } else {
            0.0
        };

        let post_existing_days = if post_window_start <= year_to {
            workdays_for_ranges_in_window(
                pool,
                user.id,
                &existing_ranges,
                post_window_start,
                year_to,
            )
            .await?
        } else {
            0.0
        };
        let post_new_days = if post_window_start <= year_to {
            if let Some((post_new_start, post_new_end)) =
                clamp_range_to_window(start_date, end_date, post_window_start, year_to)
            {
                workdays(pool, user.id, post_new_start, post_new_end).await?
            } else {
                0.0
            }
        } else {
            0.0
        };

        let pre_total = pre_existing_days + pre_new_days;
        let post_total = post_existing_days + post_new_days;
        let carryover_budget = carryover_days as f64;
        let base_budget = effective_entitlement as f64;
        let base_consumed_before_or_on_expiry = (pre_total - carryover_budget).max(0.0);
        let base_remaining_after_expiry =
            (base_budget - base_consumed_before_or_on_expiry).max(0.0);

        if exceeds_vacation_budget(post_total, base_remaining_after_expiry) {
            return Err(AppError::BadRequest(
                "Not enough remaining vacation days.".into(),
            ));
        }
    }

    // When the absence spans New Year's Day, validate the end year's budget separately.
    let end_year = end_date.year();
    if end_year != year {
        let end_year_from = NaiveDate::from_ymd_opt(end_year, 1, 1).unwrap();
        let end_year_to = NaiveDate::from_ymd_opt(end_year, 12, 31).unwrap();

        let end_year_entitled = effective_annual_days(pool, user, end_year).await?;
        let end_year_effective =
            pro_rate_entitlement(leave_entitlement_anchor(user), end_year, end_year_entitled);

        let end_year_expiry_date = parse_expiry_date(&expiry_setting, end_year);
        let current_year_carryover = carryover_from_year_into_next_year(
            pool,
            tx,
            user.id,
            year_from,
            year_to,
            effective_entitlement,
            carryover_days,
            expiry_date,
            start_date,
            end_date,
            exclude_id,
            count_new_for_carryover_source,
        )
        .await?;
        let end_year_carryover_expired = end_year_expiry_date
            .map(|expiry| today > expiry)
            .unwrap_or(false);
        let end_year_total = total_entitlement_for_dated_vacation(
            end_year_effective,
            current_year_carryover,
            end_year_expiry_date,
            end_year_carryover_expired,
        );

        let end_year_existing = AbsenceDb::vacation_ranges_in_year_tx(
            &mut *tx,
            user.id,
            end_year_from,
            end_year_to,
            exclude_id,
        )
        .await?;
        let end_year_used = workdays_for_ranges_in_window(
            pool,
            user.id,
            &end_year_existing,
            end_year_from,
            end_year_to,
        )
        .await?;
        let end_new_days = if let Some((end_new_start, end_new_end)) =
            clamp_range_to_window(start_date, end_date, end_year_from, end_year_to)
        {
            workdays(pool, user.id, end_new_start, end_new_end).await?
        } else {
            0.0
        };
        if exceeds_vacation_budget(end_year_used + end_new_days, end_year_total) {
            return Err(AppError::BadRequest(
                "Not enough remaining vacation days.".into(),
            ));
        }

        // Apply the same post-expiry rule to the end year.
        if let Some(end_year_expiry) = end_year_expiry_date {
            let end_pre_window_end = std::cmp::min(end_year_expiry, end_year_to);
            let end_post_window_start = end_year_expiry + Duration::days(1);

            let end_pre_existing_days = if end_year_from <= end_pre_window_end {
                workdays_for_ranges_in_window(
                    pool,
                    user.id,
                    &end_year_existing,
                    end_year_from,
                    end_pre_window_end,
                )
                .await?
            } else {
                0.0
            };
            let end_pre_new_days = if end_year_from <= end_pre_window_end {
                if let Some((end_pre_new_start, end_pre_new_end)) =
                    clamp_range_to_window(start_date, end_date, end_year_from, end_pre_window_end)
                {
                    workdays(pool, user.id, end_pre_new_start, end_pre_new_end).await?
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let end_post_existing_days = if end_post_window_start <= end_year_to {
                workdays_for_ranges_in_window(
                    pool,
                    user.id,
                    &end_year_existing,
                    end_post_window_start,
                    end_year_to,
                )
                .await?
            } else {
                0.0
            };
            let end_post_new_days = if end_post_window_start <= end_year_to {
                if let Some((end_post_new_start, end_post_new_end)) =
                    clamp_range_to_window(start_date, end_date, end_post_window_start, end_year_to)
                {
                    workdays(pool, user.id, end_post_new_start, end_post_new_end).await?
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let end_pre_total = end_pre_existing_days + end_pre_new_days;
            let end_post_total = end_post_existing_days + end_post_new_days;
            let end_carryover_budget = current_year_carryover as f64;
            let end_base_budget = end_year_effective as f64;
            let end_base_consumed_before_or_on_expiry =
                (end_pre_total - end_carryover_budget).max(0.0);
            let end_base_remaining_after_expiry =
                (end_base_budget - end_base_consumed_before_or_on_expiry).max(0.0);

            if exceeds_vacation_budget(end_post_total, end_base_remaining_after_expiry) {
                return Err(AppError::BadRequest(
                    "Not enough remaining vacation days.".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Compute workdays per category (used by team report). Replaces the legacy
/// `workdays_per_kind` helper that hardcoded slug-based filtering.
pub async fn workdays_per_category(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    category_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    workdays_total_for_category(pool, user_id, category_id, from, to).await
}

/// Total workdays across all categories whose `cost_type='vacation'` is
/// set. Used by the team report's vacation columns; previously this was
/// `workdays_total(pool, id, "vacation", ...)`.
pub async fn vacation_workdays(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    crate::repository::AbsenceDb::new(pool.clone())
        .vacation_workdays_total(user_id, from, to)
        .await
}

/// Total workdays across all categories whose `auto_approve_past` flag is set
/// (sick-like). Used by the team report's "sick days" column.
pub async fn auto_approve_workdays(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    crate::repository::AbsenceDb::new(pool.clone())
        .auto_approve_workdays_total(user_id, from, to)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ──────────────────────────────────────────────────────────────────────
    // validate_sick_start_date
    // ──────────────────────────────────────────────────────────────────────

    fn sample_category(slug: &str, auto_approve_past: bool) -> crate::repository::AbsenceCategory {
        crate::repository::AbsenceCategory {
            id: 1,
            slug: slug.to_string(),
            name: slug.to_string(),
            color: "#000000".to_string(),
            sort_order: 0,
            active: true,
            cost_type: "none".to_string(),
            auto_approve_past,
        }
    }

    /// Categories without auto_approve_past skip the 30-day window entirely.
    #[test]
    fn validate_backdating_window_skips_review_categories() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        let category = sample_category("vacation", false);
        let old_start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        assert!(validate_backdating_window(&category, old_start, today).is_ok());
    }

    /// An auto-approve category accepts today and the 30-day boundary.
    #[test]
    fn validate_backdating_window_accepts_recent_auto_approve_start() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        let category = sample_category("sick", true);
        let boundary = today - Duration::days(30);
        assert!(validate_backdating_window(&category, boundary, today).is_ok());
        assert!(validate_backdating_window(&category, today, today).is_ok());
    }

    /// An auto-approve category rejects start dates older than 30 days.
    #[test]
    fn validate_backdating_window_rejects_old_auto_approve_start() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        let category = sample_category("sick", true);
        let too_old = today - Duration::days(31);
        let err = validate_backdating_window(&category, too_old, today).unwrap_err();
        assert!(matches!(err, crate::error::AppError::BadRequest(_)));
    }

    // ──────────────────────────────────────────────────────────────────────
    // has_effective_workday
    // ──────────────────────────────────────────────────────────────────────

    /// A range that contains at least one Mon–Fri day and no holidays must
    /// return true.
    #[test]
    fn has_effective_workday_returns_true_when_workday_present() {
        // 2026-05-18 is a Monday.
        let monday = NaiveDate::from_ymd_opt(2026, 5, 18).unwrap();
        let friday = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        assert!(has_effective_workday(monday, friday, 5, &HashSet::new()));
    }

    /// A range that only covers Saturday and Sunday must return false for a
    /// standard 5-day contract.
    #[test]
    fn has_effective_workday_returns_false_for_weekend_only_range() {
        // 2026-05-23 Saturday, 2026-05-24 Sunday.
        let sat = NaiveDate::from_ymd_opt(2026, 5, 23).unwrap();
        let sun = NaiveDate::from_ymd_opt(2026, 5, 24).unwrap();
        assert!(!has_effective_workday(sat, sun, 5, &HashSet::new()));
    }

    /// A holiday falling on the only workday must result in false.
    #[test]
    fn has_effective_workday_returns_false_when_sole_workday_is_holiday() {
        // 2026-05-18 Monday — add it as a holiday.
        let monday = NaiveDate::from_ymd_opt(2026, 5, 18).unwrap();
        let mut holidays = HashSet::new();
        holidays.insert(monday);
        // Range is exactly one Monday — blocked by holiday.
        assert!(!has_effective_workday(monday, monday, 5, &holidays));
    }

    /// A 4-day contract (Mon–Thu) means Friday is not a workday.
    #[test]
    fn has_effective_workday_respects_workdays_per_week() {
        // 2026-05-22 is a Friday.
        let friday = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        // Friday is NOT a contract workday for a 4-day week.
        assert!(!has_effective_workday(friday, friday, 4, &HashSet::new()));
        // Thursday is the last contract workday for a 4-day week.
        let thursday = NaiveDate::from_ymd_opt(2026, 5, 21).unwrap();
        assert!(has_effective_workday(
            thursday,
            thursday,
            4,
            &HashSet::new()
        ));
    }

    // ──────────────────────────────────────────────────────────────────────
    // clamp_range_to_window
    // ──────────────────────────────────────────────────────────────────────

    /// A range fully inside the window must pass through unchanged.
    #[test]
    fn clamp_range_to_window_returns_unchanged_when_inside_window() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        let ws = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let we = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        assert_eq!(
            clamp_range_to_window(start, end, ws, we),
            Some((start, end))
        );
    }

    /// A range that starts before the window and ends inside it must be
    /// clamped to the window start.
    #[test]
    fn clamp_range_to_window_clamps_left_overhang() {
        let start = NaiveDate::from_ymd_opt(2025, 12, 20).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        let ws = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let we = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let result = clamp_range_to_window(start, end, ws, we).unwrap();
        assert_eq!(result.0, ws);
        assert_eq!(result.1, end);
    }

    /// A range that starts inside the window and ends beyond it must be
    /// clamped to the window end.
    #[test]
    fn clamp_range_to_window_clamps_right_overhang() {
        let start = NaiveDate::from_ymd_opt(2026, 12, 20).unwrap();
        let end = NaiveDate::from_ymd_opt(2027, 1, 5).unwrap();
        let ws = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let we = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let result = clamp_range_to_window(start, end, ws, we).unwrap();
        assert_eq!(result.0, start);
        assert_eq!(result.1, we);
    }

    /// A range entirely outside (before) the window must return None.
    #[test]
    fn clamp_range_to_window_returns_none_when_no_overlap() {
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        let ws = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let we = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        assert!(clamp_range_to_window(start, end, ws, we).is_none());
    }

    // ──────────────────────────────────────────────────────────────────────
    // pro_rate_entitlement
    // ──────────────────────────────────────────────────────────────────────

    /// A user who started before the year receives the full entitlement.
    #[test]
    fn pro_rate_entitlement_returns_full_when_started_before_year() {
        let start = NaiveDate::from_ymd_opt(2025, 6, 1).unwrap();
        assert_eq!(pro_rate_entitlement(start, 2026, 30), 30);
    }

    /// A user whose start date is after the end of the year gets 0.
    #[test]
    fn pro_rate_entitlement_returns_zero_when_not_yet_started() {
        let start = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();
        assert_eq!(pro_rate_entitlement(start, 2026, 30), 0);
    }

    /// A user who started on Jan 1 of the target year gets the full entitlement.
    #[test]
    fn pro_rate_entitlement_full_when_start_is_jan_first() {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(pro_rate_entitlement(start, 2026, 30), 30);
    }

    /// A user who started on July 1 (month 7) has 6 remaining months:
    /// ceil(30 * 6 / 12) = 15.
    #[test]
    fn pro_rate_entitlement_mid_year_rounds_up() {
        let start = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        // months_remaining = 13 - 7 = 6; ceil(30 * 6 / 12) = ceil(15.0) = 15
        assert_eq!(pro_rate_entitlement(start, 2026, 30), 15);
    }

    /// A user who started on December 1 has 1 remaining month:
    /// ceil(30 * 1 / 12) = ceil(2.5) = 3.
    #[test]
    fn pro_rate_entitlement_december_start_rounds_up_to_minimum() {
        let start = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        // months_remaining = 13 - 12 = 1; ceil(30 * 1 / 12) = ceil(2.5) = 3
        assert_eq!(pro_rate_entitlement(start, 2026, 30), 3);
    }

    // ──────────────────────────────────────────────────────────────────────
    // leave_entitlement_anchor
    // ──────────────────────────────────────────────────────────────────────

    /// Build a minimal auth user with the given `start_date`/`hire_date`
    /// combination — the only two fields these tests vary.
    fn user_with_dates(
        start_date: NaiveDate,
        hire_date: Option<NaiveDate>,
    ) -> crate::middleware::auth::User {
        crate::middleware::auth::User {
            id: 1,
            email: "user@example.com".to_string(),
            password_hash: "hash".to_string(),
            first_name: "Alice".to_string(),
            last_name: "Smith".to_string(),
            role: "employee".to_string(),
            weekly_hours: 40.0,
            workdays_per_week: 5,
            start_date,
            hire_date,
            active: true,
            must_change_password: false,
            created_at: chrono::Utc::now(),
            allow_reopen_without_approval: false,
            allow_submission_without_approval: false,
            dark_mode: false,
            overtime_start_balance_min: 0,
            tracks_time: true,
            annual_leave_days: 30,
            archived_at: None,
        }
    }

    /// When `hire_date` is unset, the anchor falls back to `start_date` —
    /// preserving pre-existing proration behavior for the normal case where
    /// employment and Zerf usage begin on the same day.
    #[test]
    fn leave_entitlement_anchor_falls_back_to_start_date_when_hire_date_unset() {
        let start_date = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let user = user_with_dates(start_date, None);
        assert_eq!(leave_entitlement_anchor(&user), start_date);
    }

    /// When `hire_date` is set, it takes precedence over `start_date` — this is
    /// the mid-tenure-onboarding case: the employee's Zerf `start_date` is this
    /// year, but their real employment began earlier, so the full (non-prorated)
    /// entitlement should apply.
    #[test]
    fn leave_entitlement_anchor_prefers_hire_date_when_set() {
        let start_date = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let hire_date = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let user = user_with_dates(start_date, Some(hire_date));
        assert_eq!(leave_entitlement_anchor(&user), hire_date);
        // And the resulting entitlement is the full amount, not pro-rated:
        assert_eq!(
            pro_rate_entitlement(leave_entitlement_anchor(&user), 2026, 30),
            30
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // parse_expiry_date
    // ──────────────────────────────────────────────────────────────────────

    /// A standard "03-31" setting must parse to March 31.
    #[test]
    fn parse_expiry_date_standard_setting() {
        let result = parse_expiry_date("03-31", 2026).unwrap();
        assert_eq!(result, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    /// "02-30" must be clamped to Feb 28 (or 29 in a leap year) because
    /// February never has 30 days.
    #[test]
    fn parse_expiry_date_clamps_to_month_end() {
        let normal = parse_expiry_date("02-30", 2026).unwrap();
        assert_eq!(normal, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());

        let leap = parse_expiry_date("02-30", 2024).unwrap();
        assert_eq!(leap, NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
    }

    /// "12-31" must parse correctly (December 31).
    #[test]
    fn parse_expiry_date_december() {
        let result = parse_expiry_date("12-31", 2026).unwrap();
        assert_eq!(result, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    }

    /// Invalid formats must return None.
    #[test]
    fn parse_expiry_date_returns_none_for_invalid_input() {
        assert!(parse_expiry_date("", 2026).is_none());
        assert!(parse_expiry_date("13-01", 2026).is_none()); // month 13 invalid
        assert!(parse_expiry_date("03/31", 2026).is_none()); // wrong separator
        assert!(parse_expiry_date("abc-def", 2026).is_none());
    }

    // ──────────────────────────────────────────────────────────────────────
    // total_entitlement_with_carryover
    // ──────────────────────────────────────────────────────────────────────

    /// When carryover has not expired the total includes the carryover days.
    #[test]
    fn total_entitlement_with_carryover_adds_days_when_not_expired() {
        assert_eq!(total_entitlement_with_carryover(20, 5, false), 25.0);
    }

    /// When carryover has expired only the base entitlement is returned.
    #[test]
    fn total_entitlement_with_carryover_ignores_days_when_expired() {
        assert_eq!(total_entitlement_with_carryover(20, 5, true), 20.0);
    }

    // ──────────────────────────────────────────────────────────────────────
    // total_entitlement_for_dated_vacation
    // ──────────────────────────────────────────────────────────────────────

    /// When an expiry date is configured the full entitlement including
    /// carryover is always returned (regardless of the expired flag), because
    /// the expiry is enforced date-by-date rather than globally.
    #[test]
    fn total_entitlement_for_dated_vacation_always_includes_carryover_when_expiry_set() {
        let expiry = NaiveDate::from_ymd_opt(2026, 3, 31);
        // Expired = true, but expiry date is set → carryover still applies.
        assert_eq!(
            total_entitlement_for_dated_vacation(20, 5, expiry, true),
            25.0
        );
        // Not expired with expiry date set → same result.
        assert_eq!(
            total_entitlement_for_dated_vacation(20, 5, expiry, false),
            25.0
        );
    }

    /// Without an expiry date the expired flag controls whether carryover
    /// is included (delegating to `total_entitlement_with_carryover`).
    #[test]
    fn total_entitlement_for_dated_vacation_delegates_to_carryover_when_no_expiry() {
        assert_eq!(
            total_entitlement_for_dated_vacation(20, 5, None, false),
            25.0
        );
        assert_eq!(
            total_entitlement_for_dated_vacation(20, 5, None, true),
            20.0
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // exceeds_vacation_budget
    // ──────────────────────────────────────────────────────────────────────

    /// Using more days than the budget must return true.
    #[test]
    fn exceeds_vacation_budget_returns_true_when_over_budget() {
        assert!(exceeds_vacation_budget(10.0, 9.0));
        // Just one epsilon over the limit.
        assert!(exceeds_vacation_budget(
            10.0 + VACATION_DAY_EPSILON * 2.0,
            10.0
        ));
    }

    /// Using exactly the budget or less must return false.
    #[test]
    fn exceeds_vacation_budget_returns_false_within_budget() {
        assert!(!exceeds_vacation_budget(10.0, 10.0));
        assert!(!exceeds_vacation_budget(9.0, 10.0));
        // Sub-epsilon surplus must be treated as within budget (floating-point guard).
        assert!(!exceeds_vacation_budget(
            10.0 + VACATION_DAY_EPSILON / 2.0,
            10.0
        ));
    }
}
