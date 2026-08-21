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

/// Enforce an end-date bound for absences that will be auto-approved on
/// creation (auto_approve_past category, start_date <= today).  Without this
/// guard an employee can self-approve an absence stretching months into the
/// future in a single request, bypassing any approver review for the future
/// portion.  60 days gives enough room for documented extended sick leave
/// while requiring a re-submission (and implicit approver notification) for
/// anything longer.
///
/// The check is intentionally limited to the case where the absence would be
/// immediately auto-approved (start_date <= today).  A future-start absence
/// via an auto_approve_past category still goes through the requested →
/// approved workflow because `initial_status` is "requested" when start > today.
pub fn validate_auto_approve_end_date(
    category: &crate::repository::AbsenceCategory,
    start_date: NaiveDate,
    end_date: NaiveDate,
    today: NaiveDate,
) -> AppResult<()> {
    if !category.auto_approve_past || start_date > today {
        return Ok(());
    }
    let latest_end = today + Duration::days(60);
    if end_date > latest_end {
        return Err(AppError::BadRequest(
            "This absence type can be requested at most 60 days ahead.".into(),
        ));
    }
    Ok(())
}

/// Check whether the date range contains at least one effective workday:
/// a day that belongs to the user's potential workday pool and is not a
/// public holiday.
pub fn has_effective_workday(
    start_date: NaiveDate,
    end_date: NaiveDate,
    workdays_per_week: i16,
    holidays: &std::collections::HashSet<NaiveDate>,
) -> bool {
    let mut day = start_date;
    while day <= end_date {
        let is_contract_day = crate::time_calc::is_potential_workday(day, workdays_per_week);
        if is_contract_day && !holidays.contains(&day) {
            return true;
        }
        day += Duration::days(1);
    }
    false
}

/// Validate that the absence range includes at least one effective workday.
/// For irregular users (workdays_per_week==0) we count calendar days excluding
/// holidays, so a pure-holiday range must still be rejected.
pub async fn validate_absence_has_workday(
    pool: &crate::db::DatabasePool,
    workdays_per_week: i16,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> AppResult<()> {
    let holidays = crate::repository::HolidayDb::new(pool.clone())
        .get_dates_in_range(start_date, end_date)
        .await?;
    if workdays_per_week == 0 {
        let mut day = start_date;
        while day <= end_date {
            if !holidays.contains(&day) {
                return Ok(());
            }
            day += Duration::days(1);
        }
        return Err(AppError::BadRequest(
            "Absence must include at least one workday.".into(),
        ));
    }
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
/// provided inclusive window. Ranges are treated as a union, not summed independently,
/// so the weekly cap (`workdays_per_week`) is applied once per week across all ranges
/// (fixing the double-count bug where Mon-Tue + Wed-Fri with quota 4 counted as 5).
pub async fn workdays_for_ranges_in_window(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> AppResult<f64> {
    if ranges.is_empty() {
        return Ok(0.0);
    }
    // Collect clamped ranges.
    let mut clamped: Vec<(NaiveDate, NaiveDate)> = Vec::new();
    for (start_date, end_date) in ranges {
        if let Some((cs, ce)) = clamp_range_to_window(*start_date, *end_date, window_start, window_end)
        {
            clamped.push((cs, ce));
        }
    }
    if clamped.is_empty() {
        return Ok(0.0);
    }
    // Single DB round-trip for holidays and workdays config.
    let absence_db = crate::repository::AbsenceDb::new(pool.clone());
    let holidays = absence_db.holidays_set(window_start, window_end).await?;
    let workdays_per_week = absence_db.user_workdays_per_week(user_id).await?;
    Ok(workdays_for_ranges_in_window_with_calendar(
        &clamped,
        window_start,
        window_end,
        &holidays,
        workdays_per_week,
    ))
}

/// Count workdays for already-loaded absence ranges with a caller-provided
/// calendar. Team reports use this after loading every account range and the
/// month's holidays in bulk, avoiding a database round trip for every
/// user/account cell. Implements union + weekly cap to avoid double counting.
pub fn workdays_for_ranges_in_window_with_calendar(
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    workdays_per_week: i16,
) -> f64 {
    // Clamp and collect.
    let mut clamped: Vec<(NaiveDate, NaiveDate)> = Vec::new();
    for (s, e) in ranges {
        if let Some((cs, ce)) = clamp_range_to_window(*s, *e, window_start, window_end) {
            clamped.push((cs, ce));
        }
    }
    if clamped.is_empty() {
        return 0.0;
    }
    // Sort for efficient coverage check.
    clamped.sort_by_key(|(s, _)| *s);

    if workdays_per_week <= 0 {
        // Irregular: count calendar days (excluding holidays) in union.
        let mut count = 0.0;
        let mut day = window_start;
        while day <= window_end {
            if holidays.contains(&day) {
                day += Duration::days(1);
                continue;
            }
            if clamped.iter().any(|(s, e)| day >= *s && day <= *e) {
                count += 1.0;
            }
            day += Duration::days(1);
        }
        return count;
    }

    // Count effective days per ISO week for the union of ranges.
    let mut effective_days_by_week: std::collections::HashMap<NaiveDate, i16> =
        std::collections::HashMap::new();
    let mut day = window_start;
    while day <= window_end {
        if !crate::time_calc::is_potential_workday(day, workdays_per_week) || holidays.contains(&day) {
            day += Duration::days(1);
            continue;
        }
        // Is day covered by any clamped range?
        let covered = clamped.iter().any(|(s, e)| day >= *s && day <= *e);
        if covered {
            let monday = crate::time_calc::week_monday(day);
            *effective_days_by_week.entry(monday).or_insert(0) += 1;
        }
        day += Duration::days(1);
    }
    effective_days_by_week
        .into_values()
        .map(|days| std::cmp::min(days, workdays_per_week) as f64)
        .sum()
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
///
/// Counts in twelfths — the month of entry counts as a whole month — which is
/// how German leave law (BUrlG §5) apportions a partial year. A day-granular
/// variant was tried and reverted: it silently changed every mid-year joiner's
/// entitlement and does not match the twelfths rule employees are used to.
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

/// Return the account configuration guaranteed by the database invariant.
/// Keeping this conversion at the service boundary gives callers a clear error
/// instead of silently calculating a balance from malformed direct-SQL data.
fn leave_account_configuration(
    category: &crate::repository::AbsenceCategory,
) -> AppResult<(i64, &str, i32)> {
    if !category.has_leave_account() {
        return Err(AppError::Internal(
            "Leave-account calculation requires a leave-account category.".into(),
        ));
    }
    let default_days = category.leave_account_default_days.ok_or_else(|| {
        AppError::Internal("Leave-account category has no default entitlement.".into())
    })?;
    let expiry = category
        .leave_account_carryover_expiry
        .as_deref()
        .ok_or_else(|| {
            AppError::Internal("Leave-account category has no carryover expiry.".into())
        })?;
    let start_year = category
        .leave_account_start_year
        .ok_or_else(|| AppError::Internal("Leave-account category has no start year.".into()))?;
    Ok((default_days, expiry, start_year))
}

/// A user has no entitlement and no carryover before both the user and the
/// account exist. The account start year deliberately does not affect the
/// within-year proration anchor.
pub fn effective_leave_account_start_year(
    user: &crate::middleware::auth::User,
    category: &crate::repository::AbsenceCategory,
) -> AppResult<i32> {
    let (_, _, account_start_year) = leave_account_configuration(category)?;
    Ok(user.start_date.year().max(account_start_year))
}

/// True when a leave-account tile should remain visible for this user.
/// Access grants visibility unconditionally. When access has been revoked,
/// the tile still stays visible if there is an active (not
/// cancelled/rejected) absence charged to the account that is ongoing or in
/// the future — so nobody loses sight of what is still consuming that
/// balance. Once every active booking on a revoked account is fully in the
/// past (or there never was one), the tile is hidden. This narrows the
/// original "revoked accounts stay visible" rule (see `PLAN.md`'s
/// leave-account access addendum) for this specific case.
pub async fn leave_account_tile_is_visible(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    category_id: i64,
    today: NaiveDate,
) -> AppResult<bool> {
    use crate::repository::{AbsenceCategoryDb, AbsenceDb};
    if AbsenceCategoryDb::new(pool.clone())
        .is_enabled_for_user(category_id, user_id)
        .await?
    {
        return Ok(true);
    }
    AbsenceDb::new(pool.clone())
        .has_active_or_future_leave_account_usage(user_id, category_id, today)
        .await
}

/// Resolve a user's per-category base entitlement with a possible yearly
/// override. Values are stored by immutable category id, never by name/slug.
pub async fn effective_leave_account_days(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    category_id: i64,
    year: i32,
) -> AppResult<i64> {
    crate::repository::UserDb::new(pool.clone())
        .effective_leave_account_days(user_id, category_id, year)
        .await
}

pub async fn carryover_days_into_year(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    category: &crate::repository::AbsenceCategory,
    year: i32,
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
    let (_, expiry_setting, _) = leave_account_configuration(category)?;
    let first_year = effective_leave_account_start_year(user, category)?;
    if year <= first_year {
        return Ok(0);
    }

    let today = crate::services::settings::app_today(pool).await;
    let anchor = leave_entitlement_anchor(user);
    let absence_db = crate::repository::AbsenceDb::new(pool.clone());
    let mut incoming_carryover = 0;

    for source_year in first_year..year {
        let entitled =
            effective_leave_account_days(pool, user.id, category.id, source_year).await?;
        let effective_entitlement = pro_rate_entitlement(anchor, source_year, entitled);
        let year_from = NaiveDate::from_ymd_opt(source_year, 1, 1).unwrap();
        let year_to = NaiveDate::from_ymd_opt(source_year, 12, 31).unwrap();
        let expiry_date = parse_expiry_date(expiry_setting, source_year);

        // Phantom-carryover guard: if this source year has not yet started (it
        // is entirely in the future), no vacation rows exist yet so base_usage
        // would be 0 and we would carry over a full phantom entitlement for a
        // year that hasn't happened. Skip such years entirely by treating them
        // as if the employee used their full entitlement — i.e. no net carryover
        // flows out of a year that hasn't started.
        //
        // Years that HAVE started (current year or past years) are handled
        // normally: all recorded absences (including future-dated rows inside
        // the year) are counted, because they already exist in the database.
        if source_year > today.year() {
            // No carry-through from an unstarted year.
            incoming_carryover = 0;
            continue;
        }

        // Pessimistic sourcing: count requested and cancellation_pending absences
        // as consumed in the source year, not just approved ones. This closes the
        // cross-year double-grant path:
        //
        //   A requested December absence reserves December's budget via
        //   vacation_ranges_in_year_tx (which includes requested/pending) while
        //   simultaneously appearing as unused when computing next-year carryover
        //   (which previously counted only "approved"). Both sides would be
        //   approved, exceeding the entitlement by the pending amount.
        //
        // With pessimistic sourcing, a pending request that reserves this year's
        //  budget also reduces the carryover it grants, so the sum of approved
        // days across both years cannot exceed the real entitlement.
        let statuses = &["approved", "requested", "cancellation_pending"];

        // Carryover source is scoped to the account booked on the absence,
        // not to the displayed absence category or its current cost type.
        let base_usage = if let Some(expiry) = expiry_date {
            let pre_window_end = std::cmp::min(expiry, year_to);
            let post_window_start = expiry + Duration::days(1);
            let pre_usage = if year_from <= pre_window_end {
                absence_db
                    .leave_account_workdays_total_filtered(
                        user.id,
                        category.id,
                        year_from,
                        pre_window_end,
                        statuses,
                    )
                    .await?
            } else {
                0.0
            };
            let post_usage = if post_window_start <= year_to {
                absence_db
                    .leave_account_workdays_total_filtered(
                        user.id,
                        category.id,
                        post_window_start,
                        year_to,
                        statuses,
                    )
                    .await?
            } else {
                0.0
            };
            post_usage + (pre_usage - incoming_carryover as f64).max(0.0)
        } else {
            let total_usage = absence_db
                .leave_account_workdays_total_filtered(
                    user.id,
                    category.id,
                    year_from,
                    year_to,
                    statuses,
                )
                .await?;
            (total_usage - incoming_carryover as f64).max(0.0)
        };

        incoming_carryover = std::cmp::max(0, effective_entitlement - base_usage.round() as i64);
    }

    Ok(incoming_carryover)
}

/// Build a year-level entitlement context for one leave account.
pub async fn leave_account_year_context(
    pool: &crate::db::DatabasePool,
    user: &crate::middleware::auth::User,
    category: &crate::repository::AbsenceCategory,
    year: i32,
    today: NaiveDate,
) -> AppResult<(i64, i64, bool)> {
    let (_, expiry_setting, _) = leave_account_configuration(category)?;
    if year < effective_leave_account_start_year(user, category)? {
        return Ok((0, 0, false));
    }
    let entitled = effective_leave_account_days(pool, user.id, category.id, year).await?;
    let effective_entitlement =
        pro_rate_entitlement(leave_entitlement_anchor(user), year, entitled);
    let carryover_days = carryover_days_into_year(pool, user, category, year).await?;

    let expiry_date = parse_expiry_date(expiry_setting, year).ok_or_else(|| {
        AppError::Internal("Leave-account category has an invalid carryover expiry.".into())
    })?;
    let carryover_expired = today > expiry_date;
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

pub const LEAVE_ACCOUNT_DAY_EPSILON: f64 = 0.000_001;

pub fn exceeds_leave_account_budget(required_days: f64, budget_days: f64) -> bool {
    required_days - budget_days > LEAVE_ACCOUNT_DAY_EPSILON
}

/// Calculate the source-year amount that can flow to the following year. The
/// caller supplies account-scoped ranges so the same rules work for persisted
/// balances and for a transaction that includes a proposed request.
async fn carryover_from_source_ranges(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    source_year: i32,
    effective_entitlement: i64,
    incoming_carryover: i64,
    expiry_setting: &str,
    ranges: &[(NaiveDate, NaiveDate)],
) -> AppResult<i64> {
    let year_from =
        NaiveDate::from_ymd_opt(source_year, 1, 1).expect("valid source year has a first day");
    let year_to =
        NaiveDate::from_ymd_opt(source_year, 12, 31).expect("valid source year has a last day");
    let expiry = parse_expiry_date(expiry_setting, source_year).ok_or_else(|| {
        AppError::Internal("Leave-account category has an invalid carryover expiry.".into())
    })?;
    let pre_window_end = std::cmp::min(expiry, year_to);
    let post_window_start = expiry + Duration::days(1);
    let pre_usage = if year_from <= pre_window_end {
        workdays_for_ranges_in_window(pool, user_id, ranges, year_from, pre_window_end).await?
    } else {
        0.0
    };
    let post_usage = if post_window_start <= year_to {
        workdays_for_ranges_in_window(pool, user_id, ranges, post_window_start, year_to).await?
    } else {
        0.0
    };
    let base_usage = post_usage + (pre_usage - incoming_carryover as f64).max(0.0);
    Ok((effective_entitlement - base_usage.round() as i64).max(0))
}

/// Compute carryover for a pending mutation. It uses the transaction-visible
/// account ranges and includes the proposed range exactly once, so a request
/// crossing New Year's Day cannot mint carryover from its own source-year days.
#[allow(clippy::too_many_arguments)]
async fn carryover_days_into_year_tx(
    pool: &crate::db::DatabasePool,
    tx: &mut crate::db::PgConnection,
    user: &crate::middleware::auth::User,
    category: &crate::repository::AbsenceCategory,
    year: i32,
    proposed_start: NaiveDate,
    proposed_end: NaiveDate,
    exclude_id: Option<i64>,
) -> AppResult<i64> {
    let (_, expiry_setting, _) = leave_account_configuration(category)?;
    let first_year = effective_leave_account_start_year(user, category)?;
    if year <= first_year {
        return Ok(0);
    }

    let today = crate::services::settings::app_today(pool).await;
    let mut incoming_carryover = 0;
    for source_year in first_year..year {
        if source_year > today.year() {
            incoming_carryover = 0;
            continue;
        }
        let year_from =
            NaiveDate::from_ymd_opt(source_year, 1, 1).expect("valid source year has a first day");
        let year_to =
            NaiveDate::from_ymd_opt(source_year, 12, 31).expect("valid source year has a last day");
        let entitled =
            effective_leave_account_days(pool, user.id, category.id, source_year).await?;
        let effective_entitlement =
            pro_rate_entitlement(leave_entitlement_anchor(user), source_year, entitled);
        let mut ranges = crate::repository::AbsenceDb::leave_account_ranges_in_year_tx(
            tx,
            user.id,
            category.id,
            year_from,
            year_to,
            exclude_id,
        )
        .await?;
        if let Some(range) = clamp_range_to_window(proposed_start, proposed_end, year_from, year_to)
        {
            ranges.push(range);
        }
        incoming_carryover = carryover_from_source_ranges(
            pool,
            user.id,
            source_year,
            effective_entitlement,
            incoming_carryover,
            expiry_setting,
            &ranges,
        )
        .await?;
    }
    Ok(incoming_carryover)
}

/// Compute how much carryover remains in the queried year.
pub struct CarryoverRemainingInput<'a> {
    pub pool: &'a crate::db::DatabasePool,
    pub user_id: i64,
    /// Approved and cancellation-pending ranges already booked against this
    /// specific account in the queried year.
    pub leave_account_ranges: &'a [(NaiveDate, NaiveDate)],
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
        leave_account_ranges,
        year_start,
        today,
        expiry_date,
        carryover_days,
        carryover_expired,
    } = input;

    if carryover_expired || carryover_days == 0 {
        return Ok(0.0);
    }

    let consumed = if let Some(expiry) = expiry_date {
        let cutoff = std::cmp::min(expiry, today);
        if cutoff < year_start {
            0.0
        } else {
            workdays_for_ranges_in_window(pool, user_id, leave_account_ranges, year_start, cutoff)
                .await?
        }
    } else {
        workdays_for_ranges_in_window(pool, user_id, leave_account_ranges, year_start, today)
            .await?
    };

    Ok((carryover_days as f64 - consumed).max(0.0))
}

/// Count target-bearing days in `[from, to]` for flextime-cost absence
/// accounting: every day the flextime ledger charges a target on (see
/// `services::reports::build_flextime_for_user`'s day loop) minus public
/// holidays.
///
/// This is deliberately NOT the same day count as `workdays()`/
/// `workdays_for_ranges_in_window`, which cap at the configured weekly quota
/// (`workdays_per_week`) — correct for leave-account billing, where a 3-day/
/// week contract should only be charged 3 leave days for a calendar week off.
/// A `cost_type='flextime'` absence works differently: it keeps every
/// potential weekday's target in the ledger (1-4 day/week contracts are not
/// pinned to fixed weekdays — see `time_calc::is_potential_workday`), so a
/// full calendar week off costs the full weekly target across all 5 (or 6/7)
/// potential weekdays, not just the contracted count. Using the capped count
/// here would under-reserve a full-week flextime-cost absence for anyone on a
/// reduced schedule.
async fn flextime_cost_workdays(
    pool: &crate::db::DatabasePool,
    workdays_per_week: i16,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<f64> {
    if to < from {
        return Ok(0.0);
    }
    let holidays = crate::repository::HolidayDb::new(pool.clone())
        .get_dates_in_range(from, to)
        .await?;
    let mut count = 0.0;
    let mut day = from;
    while day <= to {
        if crate::time_calc::is_potential_workday(day, workdays_per_week) && !holidays.contains(&day)
        {
            count += 1.0;
        }
        day += Duration::days(1);
    }
    Ok(count)
}

/// Validate that a flextime-cost absence (cost_type='flextime') does not
/// push the user's flextime balance below the configured floor.
///
/// The check accounts for:
/// 1. The current balance through the end of the last fully approved week
///    (from `build_flextime_for_user`).
/// 2. The post-cutoff cost of OTHER pending/approved/cancellation_pending
///    flextime-cost absences. These are committed deductions that are not yet
///    reflected in the balance. Without this, multiple requests that each
///    individually fit could be approved together and breach the floor.
/// 3. The post-cutoff cost of the proposed range itself.
///
/// `exclude_id` excludes the absence being edited/approved from (2) so it is
/// not double-counted with (3).
///
/// Known limitation: the guard covers absence cost only. Approving a timesheet
/// week is never floor-checked, so a week whose booked hours fall short of the
/// target still lowers the balance — possibly below the floor — the moment it
/// is approved and the cutoff moves past it. That is independent of any
/// absence and predates the cutoff rule. Fixing it needs a prospective-cutoff
/// simulation at approval time and is out of scope here.
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
    let base_days = crate::time_calc::potential_workdays_per_week(user.workdays_per_week);
    if base_days == 0 {
        return Ok(());
    }
    let target_per_day_min = (user.weekly_hours / f64::from(base_days) * 60.0).round() as i64;

    let floor_min: i64 =
        crate::services::settings::load_setting(pool, "flextime_min_balance_min", "0")
            .await?
            .parse::<i64>()
            .unwrap_or(0);

    // (1) Current flextime balance = cumulative balance through the cutoff date
    // (end of last fully approved week).
    let cutoff_date = crate::services::reports::flex_balance_cutoff_date(
        pool,
        user.id,
        user.start_date,
        user.workdays_per_week,
    )
    .await?;
    let balance_through_cutoff = if cutoff_date < user.start_date {
        // Cutoff is before the start date: no approved history exists yet.
        0
    } else {
        let (flextime_days, _) =
            crate::services::reports::build_flextime_for_user(pool, user, cutoff_date, cutoff_date)
                .await?;
        flextime_days.first().map(|d| d.cumulative_min).unwrap_or(0)
    };

    let unaccounted_from = cutoff_date + Duration::days(1);

    // Admin bookings are not gated on week approval, so one dated after the
    // cutoff is already binding even though the ledger above stops there.
    // Counting it here keeps a post-cutoff debit from being spent twice.
    // Deliberately unbounded at the far end: a carry-in balance booked for a
    // contract that starts in the future is just as committed as one from
    // last week, and dropping it would understate what the person may spend.
    let post_cutoff_adjustments_min = crate::repository::FlextimeAdjustmentDb::new(pool.clone())
        .sum_from(user.id, user.start_date, unaccounted_from)
        .await?;
    let current_balance_min = balance_through_cutoff + post_cutoff_adjustments_min;

    // (2) Committed-but-not-yet-accounted flextime usage from OTHER absences.
    //
    // cost_type='flextime' absences cost `target_per_day_min` per workday
    // because the day keeps its target while the user logs zero hours. Portions
    // through the cutoff are already reflected in current_balance, so count
    // every later day, including a past day after the cutoff, to avoid a gap
    // between the ledger cutoff and today.
    //
    // Including `requested` and `cancellation_pending` is conservative: a
    // pending request will probably be approved; a cancellation request might
    // not be honoured. Both can reduce the balance, so we treat them as
    // committed for safety. The `exclude_id` skips the absence we're
    // validating right now (it would otherwise count itself in step 2 AND in
    // step 3 below).
    let committed_ranges =
        AbsenceDb::flextime_cost_ranges_after_tx(tx, user.id, unaccounted_from, exclude_id).await?;
    let mut committed_cost_min: i64 = 0;
    for (range_start, range_end) in &committed_ranges {
        let effective_start = std::cmp::max(*range_start, unaccounted_from);
        if effective_start > *range_end {
            // Range was entirely before the cutoff and is already accounted for.
            continue;
        }
        let days =
            flextime_cost_workdays(pool, user.workdays_per_week, effective_start, *range_end)
                .await?;
        committed_cost_min += (days * target_per_day_min as f64).round() as i64;
    }

    // (3) Post-cutoff portion of the proposed range. Same reasoning as (2):
    // days through the cutoff were already counted in current_balance with
    // the target preserved (because cost_type='flextime' never removes the
    // target), so approving or creating the absence does not add new cost for
    // those days.
    let proposed_start = std::cmp::max(start_date, unaccounted_from);
    let proposed_cost_min = if proposed_start > end_date {
        // Entirely accounted for. The check below then reduces to
        // "current_balance - committed_cost >= floor", verifying that already
        // pending commitments do not already breach the floor.
        0
    } else {
        let days =
            flextime_cost_workdays(pool, user.workdays_per_week, proposed_start, end_date).await?;
        (days * target_per_day_min as f64).round() as i64
    };

    if current_balance_min - committed_cost_min - proposed_cost_min < floor_min {
        return Err(AppError::BadRequest(
            "Not enough flextime balance for this absence.".into(),
        ));
    }
    Ok(())
}

/// Validate the account recorded for a new, edited, or approved absence.
/// The account id is passed from the category at creation and from the stored
/// booking at approval, so historical migrations cannot be rebilled when a
/// displayed category later changes.
pub async fn validate_leave_account_balance(
    pool: &crate::db::DatabasePool,
    tx: &mut crate::db::PgConnection,
    user: &crate::middleware::auth::User,
    category: &crate::repository::AbsenceCategory,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    let (_, expiry_setting, _) = leave_account_configuration(category)?;
    let first_year = effective_leave_account_start_year(user, category)?;
    let today = crate::services::settings::app_today(pool).await;
    for year in start_date.year()..=end_date.year() {
        if year < first_year {
            return Err(AppError::BadRequest(
                "This leave account has not started for the selected year.".into(),
            ));
        }
        let year_from =
            NaiveDate::from_ymd_opt(year, 1, 1).expect("valid selected year has a first day");
        let year_to =
            NaiveDate::from_ymd_opt(year, 12, 31).expect("valid selected year has a last day");
        let entitled = effective_leave_account_days(pool, user.id, category.id, year).await?;
        let effective_entitlement =
            pro_rate_entitlement(leave_entitlement_anchor(user), year, entitled);
        let carryover_days = carryover_days_into_year_tx(
            pool, tx, user, category, year, start_date, end_date, exclude_id,
        )
        .await?;
        let expiry = parse_expiry_date(expiry_setting, year).ok_or_else(|| {
            AppError::Internal("Leave-account category has an invalid carryover expiry.".into())
        })?;
        let existing_ranges = crate::repository::AbsenceDb::leave_account_ranges_in_year_tx(
            tx,
            user.id,
            category.id,
            year_from,
            year_to,
            exclude_id,
        )
        .await?;
        validate_leave_account_year(
            pool,
            user.id,
            start_date,
            end_date,
            year_from,
            year_to,
            expiry,
            today,
            effective_entitlement,
            carryover_days,
            &existing_ranges,
        )
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn validate_leave_account_year(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    proposed_start: NaiveDate,
    proposed_end: NaiveDate,
    year_from: NaiveDate,
    year_to: NaiveDate,
    expiry: NaiveDate,
    today: NaiveDate,
    effective_entitlement: i64,
    carryover_days: i64,
    existing_ranges: &[(NaiveDate, NaiveDate)],
) -> AppResult<()> {
    // Split existing and proposed ranges into pre-expiry and post-expiry windows.
    // Carryover, when present, may only cover pre-expiry consumption, even after
    // it has expired – past absences that used it while it was valid keep that
    // coverage (mirrors compute_balances display logic).
    let pre_window_end = std::cmp::min(expiry, year_to);
    let post_window_start = expiry + Duration::days(1);

    let pre_existing = if year_from <= pre_window_end {
        workdays_for_ranges_in_window(pool, user_id, existing_ranges, year_from, pre_window_end)
            .await?
    } else {
        0.0
    };
    let pre_proposed = if let Some(range) =
        clamp_range_to_window(proposed_start, proposed_end, year_from, pre_window_end)
    {
        workdays(pool, user_id, range.0, range.1).await?
    } else {
        0.0
    };
    let post_existing = if post_window_start <= year_to {
        workdays_for_ranges_in_window(pool, user_id, existing_ranges, post_window_start, year_to)
            .await?
    } else {
        0.0
    };
    let post_proposed = if let Some(range) =
        clamp_range_to_window(proposed_start, proposed_end, post_window_start, year_to)
    {
        workdays(pool, user_id, range.0, range.1).await?
    } else {
        0.0
    };

    // Whether carryover is still usable for new pre-expiry bookings.
    let effective_carryover = if today > expiry { 0 } else { carryover_days };

    // For availability we must consider that pre-expiry consumption may have
    // been covered by carryover even after expiry.
    let base_used_before_expiry = (pre_existing + pre_proposed - carryover_days as f64).max(0.0);
    let base_remaining = (effective_entitlement as f64 - base_used_before_expiry).max(0.0);

    if today > expiry {
        // Expired: only post-expiry days count against base entitlement,
        // but base consumption includes pre-expiry minus original carryover.
        if exceeds_leave_account_budget(base_used_before_expiry + post_existing + post_proposed, effective_entitlement as f64) {
            return Err(AppError::BadRequest(
                "Not enough remaining leave-account days.".into(),
            ));
        }
    } else {
        // Not expired: total budget includes carryover, and post-expiry
        // portion must still fit into remaining base.
        let used_days = pre_existing + post_existing;
        let proposed_days = pre_proposed + post_proposed;
        let total_budget = effective_entitlement as f64 + effective_carryover as f64;
        if exceeds_leave_account_budget(used_days + proposed_days, total_budget) {
            return Err(AppError::BadRequest(
                "Not enough remaining leave-account days.".into(),
            ));
        }
        let base_used_for_remaining_check =
            (pre_existing + pre_proposed - effective_carryover as f64).max(0.0);
        let base_remaining_for_post =
            (effective_entitlement as f64 - base_used_for_remaining_check).max(0.0);
        if exceeds_leave_account_budget(post_existing + post_proposed, base_remaining_for_post) {
            return Err(AppError::BadRequest(
                "Not enough remaining leave-account days.".into(),
            ));
        }
        // For non-expired path we already validated total; still need to ensure
        // post fits base. Return early.
        return Ok(());
    }

    // Expired path: also validate post-expiry portion fits remaining base.
    if exceeds_leave_account_budget(post_existing + post_proposed, base_remaining) {
        return Err(AppError::BadRequest(
            "Not enough remaining leave-account days.".into(),
        ));
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
            unpaid: false,
            medical_certificate_relevant: false,
            leave_account_default_days: None,
            leave_account_carryover_expiry: None,
            leave_account_start_year: None,
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

    /// A 4-day schedule does not pin fixed weekdays; Friday can be a valid
    /// potential workday within Mon-Fri.
    #[test]
    fn has_effective_workday_respects_workdays_per_week() {
        // 2026-05-22 is a Friday.
        let friday = NaiveDate::from_ymd_opt(2026, 5, 22).unwrap();
        assert!(has_effective_workday(friday, friday, 4, &HashSet::new()));
        // Thursday is also a valid potential workday.
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

    /// Twelfths: a July 1 start leaves 6 of 12 months => ceil(30 * 6 / 12) = 15.
    #[test]
    fn pro_rate_entitlement_mid_year_rounds_up() {
        let start = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        assert_eq!(pro_rate_entitlement(start, 2026, 30), 15);
    }

    /// The month of entry counts in full, so a December start still yields
    /// one twelfth => ceil(30 * 1 / 12) = 3.
    #[test]
    fn pro_rate_entitlement_december_start_rounds_up_to_minimum() {
        let start = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
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
            tracks_time: true,
            archived_at: None,
            receives_error_notifications: false,
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
    // exceeds_leave_account_budget
    // ──────────────────────────────────────────────────────────────────────

    /// Using more days than the budget must return true.
    #[test]
    fn exceeds_leave_account_budget_returns_true_when_over_budget() {
        assert!(exceeds_leave_account_budget(10.0, 9.0));
        // Just one epsilon over the limit.
        assert!(exceeds_leave_account_budget(
            10.0 + LEAVE_ACCOUNT_DAY_EPSILON * 2.0,
            10.0
        ));
    }

    /// Using exactly the budget or less must return false.
    #[test]
    fn exceeds_leave_account_budget_returns_false_within_budget() {
        assert!(!exceeds_leave_account_budget(10.0, 10.0));
        assert!(!exceeds_leave_account_budget(9.0, 10.0));
        // Sub-epsilon surplus must be treated as within budget (floating-point guard).
        assert!(!exceeds_leave_account_budget(
            10.0 + LEAVE_ACCOUNT_DAY_EPSILON / 2.0,
            10.0
        ));
    }
}
