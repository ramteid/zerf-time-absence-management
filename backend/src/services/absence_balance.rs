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

pub async fn workdays_total(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    kind: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    use crate::repository::AbsenceDb;
    AbsenceDb::new(pool.clone())
        .workdays_total(user_id, kind, from, to)
        .await
}

pub fn validate_sick_start_date(kind: &str, start_date: NaiveDate, today: NaiveDate) -> AppResult<()> {
    if kind != "sick" {
        return Ok(());
    }

    let earliest = today - Duration::days(30);
    if start_date < earliest {
        return Err(AppError::BadRequest(
            "Sick leave cannot be backdated more than 30 days.".into(),
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
        let is_contract_day = Datelike::weekday(&day).num_days_from_monday() < workdays_per_week as u32;
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
    if year <= Datelike::year(&user.start_date) {
        return Ok(0);
    }

    let absence_db = crate::repository::AbsenceDb::new(pool.clone());
    let default_days = crate::repository::UserDb::new(pool.clone())
        .get_default_leave_days()
        .await?;
    let mut incoming_carryover = 0;

    for source_year in user.start_date.year()..year {
        let entitled = annual_days_or_default(pool, user.id, source_year, default_days).await?;
        let effective_entitlement = pro_rate_entitlement(user.start_date, source_year, entitled);
        let year_from = NaiveDate::from_ymd_opt(source_year, 1, 1).unwrap();
        let year_to = NaiveDate::from_ymd_opt(source_year, 12, 31).unwrap();
        let expiry_date = parse_expiry_date(expiry_setting, source_year);

        let base_usage = if let Some(expiry) = expiry_date {
            let pre_window_end = std::cmp::min(expiry, year_to);
            let post_window_start = expiry + Duration::days(1);
            let pre_usage = if year_from <= pre_window_end {
                absence_db
                    .workdays_total_filtered(
                        user.id,
                        "vacation",
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
                    .workdays_total_filtered(
                        user.id,
                        "vacation",
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
                .workdays_total_filtered(user.id, "vacation", year_from, year_to, &["approved"])
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
    let effective_entitlement = pro_rate_entitlement(user.start_date, year, entitled);
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
        let end_year_effective = pro_rate_entitlement(user.start_date, end_year, end_year_entitled);

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

/// Compute workdays per kind (used by team report and balance).
pub async fn workdays_per_kind(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    kind: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    workdays_total(pool, user_id, kind, from, to).await
}
