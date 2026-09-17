use crate::error::{AppError, AppResult};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};

/// Computes the automatic break deduction in minutes for a set of work entries within one day.
///
/// `rules` is a slice of `(threshold_min, deduction_min)` pairs representing break tiers.
/// This matches German labor law (ArbZG §4: a break is required for a day's work of
/// "mehr als sechs [neun] Stunden **insgesamt**" — more than six/nine hours **in total**).
/// The **highest applicable rule** — the one with the greatest threshold that the day's
/// *total* worked time strictly exceeds — determines how many minutes of break the day
/// requires; rules are **not** cumulative. Thresholds are exclusive: a day of exactly
/// 6h00m worked does not trigger the 6-hour rule; only 6h01m or more does.
///
/// Any real gap between logged entries (there is no separate "break" category in this
/// app — a break is always just unlogged time) counts as break already taken and is
/// credited against the requirement. Only the shortfall, if any, is deducted from the
/// credited work minutes. A day with one continuous entry span (no gaps) has nothing to
/// credit, so the full requirement is deducted — unchanged from a naive per-block reading.
///
/// Example: rules = [(360, 30), (540, 45)], a day worked 08:00-18:00 with a 14:00-14:30
/// gap (9h30m worked, 30 min already taken as a real gap): the day's total (570 min)
/// exceeds the 9-hour tier, requiring 45 min of break; 30 min was already taken, so only
/// 15 min more is deducted from the credited total.
///
/// Entries that are directly adjacent (one ends exactly when the next begins) are merged
/// into a single continuous work block for the purposes of computing the day's total
/// worked time and the wall-clock span; overlapping entries are merged as well (handled
/// defensively). A gap of even one minute between blocks counts toward the taken break.
pub fn compute_day_auto_break(entries: &[(NaiveTime, NaiveTime)], rules: &[(i64, i64)]) -> i64 {
    if entries.is_empty() || rules.is_empty() {
        return 0;
    }
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|(s, _)| *s);

    // Merge adjacent/overlapping entries into continuous work blocks.
    let mut blocks: Vec<(NaiveTime, NaiveTime)> = Vec::new();
    for (start, end) in sorted {
        if let Some(last) = blocks.last_mut() {
            if start <= last.1 {
                // Adjacent (start == last.1) or overlapping: extend current block.
                if end > last.1 {
                    last.1 = end;
                }
                continue;
            }
        }
        blocks.push((start, end));
    }

    // Day total worked time, summed across all blocks (this is the "insgesamt" ArbZG §4
    // tests against — not each block's own duration). Use seconds then convert to
    // minutes to avoid truncating HH:MM:SS inputs to zero.
    let worked_minutes: i64 = blocks
        .iter()
        .map(|(s, e)| (*e - *s).num_seconds() / 60)
        .sum();

    // Wall-clock span from the first entry's start to the last entry's end, minus the
    // worked time, is the total real rest time already taken between blocks today.
    // Safe to unwrap: `blocks` is non-empty because `entries` was checked non-empty above.
    let first_start = blocks.first().unwrap().0;
    let last_end = blocks.last().unwrap().1;
    let total_span_minutes = (last_end - first_start).num_seconds() / 60;
    let taken_minutes = (total_span_minutes - worked_minutes).max(0);

    // Highest applicable rule wins; 0 when no rule threshold is strictly exceeded by the
    // day's total worked time. We pick the deduction belonging to the greatest
    // threshold that is exceeded, not the greatest deduction value.
    let required_minutes = rules
        .iter()
        .filter(|(threshold, _)| worked_minutes > *threshold)
        .max_by_key(|(threshold, _)| *threshold)
        .map(|(_, deduction)| *deduction)
        .unwrap_or(0);

    (required_minutes - taken_minutes).max(0)
}

/// Compute the Monday of the ISO week that contains `date`.
/// This is the canonical implementation used across services and background tasks.
pub fn week_monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// Return the number of days in a given month (month is 1-based).
/// Returns 28 as a safe fallback if the arithmetic overflows (unreachable in practice).
pub fn last_day_of_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next_month
        .and_then(|date| date.pred_opt())
        .map(|date| date.day())
        .unwrap_or(28)
}

/// Returns the weekly pool of potential workdays used for users without fixed
/// per-weekday contracts.
///
/// For 1-5 configured days, all weekdays (Mon-Fri) are potential days.
/// For 6 configured days, Mon-Sat are potential days.
/// For 7 configured days, every calendar day is a potential day.
pub fn potential_workdays_per_week(workdays_per_week: i16) -> u32 {
    match workdays_per_week {
        i16::MIN..=0 => 0,
        1..=5 => 5,
        6 => 6,
        _ => 7,
    }
}

/// True when `date` belongs to the user's potential workday pool.
///
/// This intentionally does not pin a user to fixed weekdays for 1-5 day
/// schedules: those users can distribute their workdays across Mon-Fri.
pub fn is_potential_workday(date: NaiveDate, workdays_per_week: i16) -> bool {
    let weekday = date.weekday().num_days_from_monday();
    match workdays_per_week {
        i16::MIN..=0 => false,
        1..=5 => weekday < 5,
        6 => weekday < 6,
        _ => true,
    }
}

/// The individual days a set of date ranges costs, in calendar order, with the
/// weekly cap applied **once** across the whole window.
///
/// This is the single implementation of Zerf's leave-day calendar. Callers that
/// only need the total take `.len()`; callers that have to split the same window
/// into buckets (already taken vs. still upcoming, before vs. after a carryover
/// expiry) walk the returned dates instead of counting two narrower windows.
/// Counting narrower windows applies the weekly cap to each of them, so one
/// calendar week off could be billed twice over — a 3-day/week employee away
/// Mon-Fri was charged 3 days for Mon-Wed plus 2 for Thu-Fri instead of the 3
/// the week can ever cost.
///
/// Ranges are treated as a union, never summed: two bookings inside one week
/// together still cost only what that week is worth.
///
/// Within a week the earliest covered days are the ones that count, so the
/// result is deterministic and a day's bucket never depends on how the caller
/// happens to slice the window.
pub fn counted_workdays(
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    workdays_per_week: i16,
) -> Vec<NaiveDate> {
    let clamped: Vec<(NaiveDate, NaiveDate)> = ranges
        .iter()
        .map(|(start, end)| ((*start).max(window_start), (*end).min(window_end)))
        .filter(|(start, end)| start <= end)
        .collect();
    if clamped.is_empty() {
        return Vec::new();
    }

    // An irregular schedule has no weekly quota to cap against, so every
    // covered non-holiday calendar day counts.
    let irregular = potential_workdays_per_week(workdays_per_week) == 0;

    let mut counted = Vec::new();
    let mut used_in_week: std::collections::HashMap<NaiveDate, i16> =
        std::collections::HashMap::new();
    let mut date = window_start;
    while date <= window_end {
        let is_candidate = !holidays.contains(&date)
            && (irregular || is_potential_workday(date, workdays_per_week));
        if is_candidate && clamped.iter().any(|(start, end)| date >= *start && date <= *end) {
            if irregular {
                counted.push(date);
            } else {
                let used = used_in_week.entry(week_monday(date)).or_insert(0);
                if *used < workdays_per_week {
                    *used += 1;
                    counted.push(date);
                }
            }
        }
        date += Duration::days(1);
    }
    counted
}

/// How many of the counted days each range is charged, when the weekly cap is
/// applied once across all of them.
///
/// Returns one count per input range, in input order. Days are attributed in
/// chronological order of range start (input order breaks ties), so the first
/// booking in a week keeps its own days and a later one in the same week is
/// charged only what the week has left.
///
/// This is the per-range counterpart of [`counted_workdays`], for callers that
/// print or bill each range separately and must still have those numbers add up
/// to what the week actually costs. Counting each range on its own re-applies
/// the cap to every one of them, so two absences inside a single calendar week
/// bill a part-time contract for more days than that week can ever hold.
pub fn counted_days_per_range(
    ranges: &[(NaiveDate, NaiveDate)],
    window_start: NaiveDate,
    window_end: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    workdays_per_week: i16,
) -> Vec<f64> {
    let mut counts = vec![0.0; ranges.len()];
    if ranges.is_empty() {
        return counts;
    }
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by_key(|index| (ranges[*index].0, *index));
    for day in counted_workdays(ranges, window_start, window_end, holidays, workdays_per_week) {
        if let Some(owner) = order
            .iter()
            .find(|index| day >= ranges[**index].0 && day <= ranges[**index].1)
        {
            counts[*owner] += 1.0;
        }
    }
    counts
}

//
// ─────────────────────────────────────────────────────────────────────────────
// Contract-day leave accounting ("a leave day is one of the contract's own
// working days"). NOT WIRED INTO THE APP YET — nothing calls the three items
// below outside their tests. They exist so the rule can be reviewed against
// worked examples before any running calculation changes.
//
// Today's rule and this one differ only for a contract of fewer than five
// working days. Today a leave day removes one *potential* day of target
// (`weekly_hours / 5`), while the leave account is charged per calendar
// workday capped at the contract's weekly days. Those two units disagree, so
// the same three leave days buy a whole week off when booked Monday to Friday
// and 60% of a week when booked Monday to Wednesday.
//
// Here one leave day is worth `weekly_hours / workdays_per_week` — eight hours
// on a three-day, 24-hour contract — and it removes exactly that much target.
// A week is then either fully bought out or not, and the two units agree.
// ─────────────────────────────────────────────────────────────────────────────

/// What one ISO week costs and what it leaves to work, under the contract-day
/// rule described above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekLeaveAccounting {
    /// Absence days charged to the leave account for this week, in calendar
    /// order. Public holidays are never charged: they cost the week a day of
    /// target without costing the account anything.
    pub charged_dates: Vec<NaiveDate>,
    /// Work target left for this week once holidays and charged absence days
    /// have been taken off, in minutes. Never below zero.
    pub remaining_target_min: i64,
}

/// Minutes one of the contract's own working days is worth.
///
/// Divided by the *contracted* weekly days, not by the potential pool: that is
/// the whole point of the rule. A three-day, 24-hour contract has eight-hour
/// working days, whichever three weekdays they fall on.
pub fn contract_day_minutes(weekly_hours: f64, workdays_per_week: i16) -> i64 {
    if workdays_per_week <= 0 || !weekly_hours.is_finite() || weekly_hours <= 0.0 {
        return 0;
    }
    (weekly_hours / f64::from(workdays_per_week) * 60.0).round() as i64
}

/// Account one whole ISO week beginning on `week_monday`.
///
/// The week is judged whole, never in slices. That is what keeps a week
/// straddling New Year's Eve (or a month end) from being charged once on each
/// side of the boundary: callers cut the returned days into their buckets
/// afterwards, exactly as [`counted_workdays`] is used today.
///
/// `absence_days` holds the days a target-removing absence covers. Days that
/// are public holidays are ignored there — a holiday already costs the week a
/// day, and charging leave on top would bill the same day twice.
///
/// `contract_start` is the employee's first day. Days before it are not part
/// of the contract: they cost no leave and carry no target, the same way every
/// other view hides content before a start date. Leaving them out entirely
/// charged somebody who joined on a Wednesday for the Monday and Tuesday
/// before they were hired.
pub fn week_leave_accounting(
    week_monday: NaiveDate,
    contract_start: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    absence_days: &std::collections::HashSet<NaiveDate>,
    weekly_hours: f64,
    workdays_per_week: i16,
) -> WeekLeaveAccounting {
    let potential = potential_workdays_per_week(workdays_per_week);
    // No potential pool means no contract to measure against. Every absence day
    // that is not a holiday is charged, and there is no target to leave behind.
    if potential == 0 {
        let mut charged: Vec<NaiveDate> = (0..7i64)
            .map(|offset| week_monday + Duration::days(offset))
            .filter(|date| {
                *date >= contract_start && !holidays.contains(date) && absence_days.contains(date)
            })
            .collect();
        charged.sort_unstable();
        return WeekLeaveAccounting {
            charged_dates: charged,
            remaining_target_min: 0,
        };
    }

    // Days before the contract began are not part of this week for this
    // employee. They drop out of the baseline further down rather than being
    // subtracted from it: counting them as lost as well takes them off twice,
    // which read a new starter's first week as 141 minutes where it is 843.
    let mut holiday_days = 0u32;
    let mut absence_dates: Vec<NaiveDate> = Vec::new();
    let mut days_in_contract = 0u32;
    for offset in 0..i64::from(potential) {
        let date = week_monday + Duration::days(offset);
        if date < contract_start {
            continue;
        }
        days_in_contract += 1;
        if holidays.contains(&date) {
            holiday_days += 1;
            continue;
        }
        if absence_days.contains(&date) {
            absence_dates.push(date);
        }
    }

    // A public holiday saves a leave day only where the leave day would have
    // fallen on it. That day is simply not chargeable, which the loop above
    // already took care of. A holiday elsewhere in the week grants no discount
    // on the days that are booked: the contract does not say which weekdays
    // are worked, so a holiday the person may never have worked cannot be
    // assumed to have spared them one.
    let quota = u32::try_from(workdays_per_week).unwrap_or(0);
    let charged_count = absence_dates.len().min(quota as usize);
    let charged_dates = absence_dates[..charged_count].to_vec();

    // For the *target* a holiday counts like any other day the person does not
    // have to work, because that is what a contract day is worth under this
    // rule. The week can never lose more days than it actually holds for this
    // employee, which is the smaller of the contract's weekly days and the days
    // of the week that lie inside the contract at all.
    let losable_days = quota.min(days_in_contract);
    let lost_days = (holiday_days + charged_count as u32).min(losable_days);
    let day_value_min = contract_day_minutes(weekly_hours, workdays_per_week);

    // A week nobody was away in has to come out at exactly the number it has
    // today, down to the minute: nothing happened in it, and a rounding change
    // must not move a balance somebody already read. So the untouched week
    // keeps today's arithmetic — the potential pool times the potential day —
    // and only the days actually lost are priced as contract days.
    let potential_day_min = if potential == 0 {
        0
    } else {
        (weekly_hours.max(0.0) / f64::from(potential) * 60.0).round() as i64
    };
    // Only the days the contract actually covers form the baseline, so a first
    // week beginning mid-week starts from the days that exist for the employee.
    let untouched_week_min = i64::from(days_in_contract) * potential_day_min;
    let remaining_target_min = if lost_days >= losable_days {
        // Every day this week holds for them is gone, so nothing is left.
        // Stated outright rather than subtracted, because the two roundings
        // would otherwise leave a stray minute behind.
        0
    } else {
        (untouched_week_min - i64::from(lost_days) * day_value_min).max(0)
    };
    WeekLeaveAccounting {
        charged_dates,
        remaining_target_min,
    }
}

/// The leave days `ranges` cost inside `[window_start, window_end]`, under the
/// contract-day rule.
///
/// Every ISO week the window touches is accounted for **whole**, and only then
/// are the resulting days cut down to the window. A week split by the turn of
/// the year is therefore charged once, not once per year — the defect that
/// made a part-timer pay an extra day for a holiday over New Year.
///
/// `ranges` must carry the absences' real bounds, not bounds already clamped to
/// the window, or the days lying just outside it cannot be seen. `holidays`
/// likewise has to cover the whole weeks at both ends of the window.
///
/// **The caller must also fetch every absence touching those boundary weeks,
/// not only the ones overlapping the window.** A year- or month-scoped query
/// returns only absences overlapping its own period. Two separate bookings on
/// one account either side of a boundary, inside a single calendar week, then
/// reach each window as a lone range, and each window prices that week on its
/// own — which is the double charge this function exists to remove.
pub fn counted_leave_days(
    ranges: &[(NaiveDate, NaiveDate)],
    contract_start: NaiveDate,
    window_start: NaiveDate,
    window_end: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    weekly_hours: f64,
    workdays_per_week: i16,
) -> Vec<NaiveDate> {
    if ranges.is_empty() || window_end < window_start {
        return Vec::new();
    }
    let first_monday = week_monday(window_start);
    let last_monday = week_monday(window_end);

    let mut charged = Vec::new();
    let mut monday = first_monday;
    while monday <= last_monday {
        let absence_days: std::collections::HashSet<NaiveDate> = (0..7i64)
            .map(|offset| monday + Duration::days(offset))
            .filter(|date| {
                ranges
                    .iter()
                    .any(|(start, end)| date >= start && date <= end)
            })
            .collect();
        let week = week_leave_accounting(
            monday,
            contract_start,
            holidays,
            &absence_days,
            weekly_hours,
            workdays_per_week,
        );
        charged.extend(
            week.charged_dates
                .into_iter()
                .filter(|date| *date >= window_start && *date <= window_end),
        );
        monday += Duration::days(7);
    }
    charged
}

//
// ─────────────────────────────────────────────────────────────────────────────
// Fixed working weekdays. NOT WIRED INTO THE APP YET.
//
// `workdays_per_week` only ever said how many days somebody works, never which.
// Every calculation had to guess, and the guesses disagreed: the target was
// spread over Monday to Friday, leave was counted with a weekly cap standing in
// for "which days are yours", and a public holiday was pro-rated because nobody
// could say whether it fell on a working day.
//
// With the weekdays known, none of that guessing is left. A working day carries
// `weekly_hours / <number of working days>`; a day that is not a working day
// carries nothing and costs no leave. The weekly cap, the whole-week accounting
// and the special handling at month and year boundaries all become unnecessary,
// because each day now answers for itself.
// ─────────────────────────────────────────────────────────────────────────────

/// Which weekdays a contract places work on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkSchedule {
    /// Bit `n` set means ISO weekday `n + 1` is worked; Monday is bit 0.
    /// Zero means no fixed pattern at all.
    weekdays: u8,
    /// Only consulted when there is no fixed pattern: the assistants' case,
    /// where the old count is all there is.
    fallback_days_per_week: i16,
}

impl WorkSchedule {
    /// A contract with known working days, given as ISO weekday numbers where
    /// Monday is 1 and Friday is 5.
    ///
    /// Saturday and Sunday are not accepted: work is recorded Monday to Friday,
    /// and the stored pattern is constrained to those five days, so a schedule
    /// the database would refuse must not be constructible here either.
    ///
    /// Numbers outside the range are ignored, and an empty result is not a
    /// schedule and yields `None` — "works no days" is a different statement
    /// from "days not recorded", and the second is what no schedule at all
    /// means.
    pub fn fixed(weekdays: &[u8]) -> Option<Self> {
        let mut bits = 0u8;
        for day in weekdays {
            if (1..=5).contains(day) {
                bits |= 1 << (day - 1);
            }
        }
        (bits != 0).then_some(Self {
            weekdays: bits,
            fallback_days_per_week: 0,
        })
    }

    /// A contract whose working days are not recorded. Everything falls back to
    /// the potential-pool behaviour the app had before weekdays were stored.
    pub fn without_fixed_days(workdays_per_week: i16) -> Self {
        Self {
            weekdays: 0,
            fallback_days_per_week: workdays_per_week,
        }
    }

    pub fn has_fixed_days(&self) -> bool {
        self.weekdays != 0
    }

    /// True when `date` is one of the contract's working days.
    ///
    /// Without a fixed pattern this is the old question — does the date fall in
    /// the potential pool — so callers get the behaviour they had before.
    pub fn covers(&self, date: NaiveDate) -> bool {
        if !self.has_fixed_days() {
            return is_potential_workday(date, self.fallback_days_per_week);
        }
        let iso = date.weekday().number_from_monday();
        self.weekdays & (1 << (iso - 1)) != 0
    }

    /// How many days a week the contract works.
    pub fn days_per_week(&self) -> u32 {
        if self.has_fixed_days() {
            u32::from(self.weekdays.count_ones() as u8)
        } else {
            potential_workdays_per_week(self.fallback_days_per_week)
        }
    }

    /// Minutes one working day carries.
    ///
    /// The weekly hours divided by the days actually worked, which is what a
    /// working day *is* once the days are known. Dividing by the potential pool
    /// instead is what made a leave day worth less than the day it replaced.
    pub fn day_minutes(&self, weekly_hours: f64) -> i64 {
        let days = self.days_per_week();
        if days == 0 || !weekly_hours.is_finite() || weekly_hours <= 0.0 {
            return 0;
        }
        (weekly_hours / f64::from(days) * 60.0).round() as i64
    }
}

/// A person's working days over time.
///
/// Zerf stores no flextime or leave figure: it recomputes each of them from
/// scratch on every query. A single stored pattern would therefore be applied
/// to the whole past as well, so the day somebody's working days change, every
/// week they ever worked is re-judged under the new pattern — every past Monday
/// loses its target and every past Friday gains one, with nothing on the record
/// to say why.
///
/// Each entry states "from this date on, these are the working days". A change
/// adds an entry rather than replacing one, and a week asks which pattern was
/// in force on its Monday. Everything else stays live, so a sick note entered
/// for a past week still takes effect at once.
#[derive(Debug, Clone)]
pub struct WorkScheduleHistory {
    /// Ascending by the date each pattern starts applying.
    entries: Vec<(NaiveDate, WorkSchedule)>,
    /// Used for dates before the first entry, and for people who have no
    /// entries at all — the assistants, who have no work target to place.
    fallback: WorkSchedule,
}

impl WorkScheduleHistory {
    /// Build from stored entries in any order, plus the schedule to use where
    /// no entry reaches.
    pub fn new(entries: Vec<(NaiveDate, WorkSchedule)>, fallback: WorkSchedule) -> Self {
        let mut entries = entries;
        entries.sort_by_key(|(valid_from, _)| *valid_from);
        Self { entries, fallback }
    }

    /// A person whose working days were never recorded.
    pub fn without_history(fallback: WorkSchedule) -> Self {
        Self {
            entries: Vec::new(),
            fallback,
        }
    }

    /// The schedule in force on `date`: the latest entry that had already begun.
    ///
    /// An entry beginning exactly on `date` counts, because `valid_from` names
    /// the first day the pattern applies to.
    pub fn on(&self, date: NaiveDate) -> WorkSchedule {
        self.entries
            .iter()
            .rev()
            .find(|(valid_from, _)| *valid_from <= date)
            .map(|(_, schedule)| *schedule)
            .unwrap_or(self.fallback)
    }

    /// True when no entry was ever recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The days `ranges` cost against a leave account.
///
/// One working day covered by an absence costs one leave day. There is no
/// weekly cap and no whole-week arithmetic: a week cannot hold more working
/// days than the contract has, so there is nothing left to cap. That is also
/// why a week split by a month or year boundary is charged correctly without
/// any special handling — each day simply belongs to the period it falls in.
///
/// A public holiday costs nothing, and neither does a day before the contract
/// began. Days outside `[window_start, window_end]` are left out.
pub fn scheduled_leave_days(
    history: &WorkScheduleHistory,
    ranges: &[(NaiveDate, NaiveDate)],
    contract_start: NaiveDate,
    window_start: NaiveDate,
    window_end: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
) -> Vec<NaiveDate> {
    if ranges.is_empty() || window_end < window_start {
        return Vec::new();
    }
    let mut charged = Vec::new();
    let mut date = window_start.max(contract_start);
    while date <= window_end {
        // Asked per day, not once for the window. A window is usually a month
        // or a year, and the pattern may well have changed inside it; taking a
        // single pattern for the whole span would judge part of it under a
        // schedule that was not in force, which is the very thing the history
        // exists to prevent.
        if history.on(date).covers(date)
            && !holidays.contains(&date)
            && ranges.iter().any(|(from, to)| date >= *from && date <= *to)
        {
            charged.push(date);
        }
        date += Duration::days(1);
    }
    charged
}

/// Minutes of work the week beginning on `week_monday` asks for.
///
/// Every working day carries one day's minutes unless something takes it away:
/// a public holiday, an absence that removes the target, or the day lying
/// before the contract began. A day that is not a working day carries nothing,
/// which is what makes hours booked on it count as pure overtime and a leave
/// day booked on it cost nothing.
pub fn scheduled_week_target_min(
    history: &WorkScheduleHistory,
    week_monday: NaiveDate,
    contract_start: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    absence_days: &std::collections::HashSet<NaiveDate>,
    weekly_hours: f64,
) -> i64 {
    // Each day is worth what the contract said on that day, so the pattern is
    // asked for per day rather than once for the week. A pattern beginning
    // mid-week therefore splits the week honestly: the days before it are worth
    // the old contract's day and the days after it the new one. Such a week can
    // add up to more or less than `weekly_hours`, which is the truthful answer
    // when the number of working days changed partway through it — and it keeps
    // the target in step with the leave charge, which is also decided per day.
    (0..7i64)
        .map(|offset| week_monday + Duration::days(offset))
        .filter(|date| {
            *date >= contract_start
                && history.on(*date).covers(*date)
                && !holidays.contains(date)
                && !absence_days.contains(date)
        })
        .map(|date| history.on(date).day_minutes(weekly_hours))
        .sum()
}

/// Count effective workdays in `[from, to]`, excluding public holidays.
///
/// Thin wrapper around [`counted_workdays`] over the whole range: the weekly
/// cap, the potential-day pool and the irregular-schedule rule all live there,
/// so a range count and a per-day leave count can never drift apart.
pub fn count_workdays(
    from: NaiveDate,
    to: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    workdays_per_week: i16,
) -> f64 {
    counted_workdays(&[(from, to)], from, to, holidays, workdays_per_week).len() as f64
}

pub fn parse_hhmm_or_hhmmss(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
        .ok()
}

pub fn parse_input_time(value: &str) -> AppResult<NaiveTime> {
    parse_hhmm_or_hhmmss(value)
        .ok_or_else(|| AppError::BadRequest(format!("Invalid time: {value}")))
}

pub fn parse_stored_time(value: &str) -> AppResult<NaiveTime> {
    parse_hhmm_or_hhmmss(value)
        .ok_or_else(|| AppError::Internal("Invalid time value stored in database.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    #[test]
    fn week_monday_returns_monday_for_any_weekday() {
        // 2026-05-11 is a Monday
        let monday = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
        let friday = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 5, 17).unwrap();
        assert_eq!(week_monday(monday), monday);
        assert_eq!(week_monday(friday), monday);
        assert_eq!(week_monday(sunday), monday);
    }

    #[test]
    fn last_day_of_month_handles_standard_and_edge_cases() {
        assert_eq!(last_day_of_month(2026, 1), 31);
        assert_eq!(last_day_of_month(2026, 4), 30);
        assert_eq!(last_day_of_month(2026, 12), 31);
        assert_eq!(last_day_of_month(2025, 2), 28);
        assert_eq!(last_day_of_month(2024, 2), 29); // leap year
    }

    /// `parse_hhmm_or_hhmmss` must accept both the HH:MM and HH:MM:SS formats
    /// and return `None` for anything else.
    #[test]
    fn parse_hhmm_or_hhmmss_accepts_both_time_formats() {
        assert_eq!(
            parse_hhmm_or_hhmmss("08:30"),
            NaiveTime::from_hms_opt(8, 30, 0)
        );
        assert_eq!(
            parse_hhmm_or_hhmmss("17:45:00"),
            NaiveTime::from_hms_opt(17, 45, 0)
        );
        assert_eq!(
            parse_hhmm_or_hhmmss("00:00:00"),
            NaiveTime::from_hms_opt(0, 0, 0)
        );
        assert_eq!(
            parse_hhmm_or_hhmmss("23:59:59"),
            NaiveTime::from_hms_opt(23, 59, 59)
        );
    }

    /// Malformed strings must return `None`.
    #[test]
    fn parse_hhmm_or_hhmmss_rejects_invalid_strings() {
        assert!(parse_hhmm_or_hhmmss("").is_none());
        assert!(parse_hhmm_or_hhmmss("25:00").is_none()); // out-of-range hour
        assert!(parse_hhmm_or_hhmmss("08-30").is_none()); // wrong separator
        assert!(parse_hhmm_or_hhmmss("not-a-time").is_none());
        assert!(parse_hhmm_or_hhmmss("99:99:99").is_none()); // all fields out of range
    }

    /// `parse_input_time` must succeed for valid values and return a
    /// `BadRequest` error for invalid ones (caller provided the value).
    #[test]
    fn parse_input_time_returns_bad_request_on_invalid_input() {
        assert!(parse_input_time("09:15").is_ok());
        assert!(parse_input_time("09:15:00").is_ok());

        let err = parse_input_time("bad").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    /// `parse_stored_time` must succeed for valid values and return an
    /// `Internal` error for invalid ones (the value came from the database).
    #[test]
    fn parse_stored_time_returns_internal_error_on_invalid_data() {
        assert!(parse_stored_time("14:00").is_ok());
        assert!(parse_stored_time("14:00:00").is_ok());

        let err = parse_stored_time("corrupted").unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn compute_day_auto_break_no_entries_returns_zero() {
        assert_eq!(compute_day_auto_break(&[], &[(360, 30)]), 0);
    }

    #[test]
    fn compute_day_auto_break_empty_rules_returns_zero() {
        assert_eq!(compute_day_auto_break(&[(t(8, 0), t(18, 0))], &[]), 0);
    }

    #[test]
    fn compute_day_auto_break_single_entry_below_threshold_no_deduction() {
        // 5 h 59 min, threshold 6 h → no deduction
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(13, 59))], &[(360, 30)]),
            0
        );
    }

    #[test]
    fn compute_day_auto_break_single_entry_exactly_at_threshold_no_deduction() {
        // Exactly 6 h → no deduction. Thresholds are exclusive (ArbZG §4 requires a
        // break only for work of *more than* six hours, not for six hours flat).
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(14, 0))], &[(360, 30)]),
            0
        );
    }

    #[test]
    fn compute_day_auto_break_single_entry_one_minute_over_threshold_deducts() {
        // 6 h 1 min → threshold strictly exceeded → deduct 30 min
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(14, 1))], &[(360, 30)]),
            30
        );
    }

    #[test]
    fn compute_day_auto_break_adjacent_entries_merged_into_one_block() {
        // 8:00–12:00 immediately followed by 12:00–16:00 → 8 h continuous
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(12, 0)), (t(12, 0), t(16, 0))], &[(360, 30)]),
            30 // single block of 8 h ≥ 6 h → one deduction
        );
    }

    #[test]
    fn compute_day_auto_break_one_minute_gap_credits_only_that_minute() {
        // 8:00–12:00, then 12:01–16:00 → two blocks, but the day's total worked time
        // (479 min) still exceeds the 6 h threshold, so 30 min break is required. Only
        // the 1-minute real gap is credited against it, leaving a 29-minute deduction.
        // (Splitting entries with a token 1-minute gap does NOT void the break rule the
        // way it did under the old per-block logic — that was a loophole.)
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(12, 0)), (t(12, 1), t(16, 0))], &[(360, 30)]),
            29
        );
    }

    #[test]
    fn compute_day_auto_break_gap_between_blocks_covers_the_days_requirement() {
        // morning 7:00–13:01 (6h01m), afternoon 14:00–20:01 (6h01m), 59-min gap between.
        // Day total worked = 722 min > 6 h → 30 min required. The 59-minute real gap
        // already taken between the blocks more than covers that → 0 deduction.
        // (The old per-block logic deducted 30+30=60 min here — double-counting, since
        // the person already rested far more than the law requires for the day.)
        assert_eq!(
            compute_day_auto_break(&[(t(7, 0), t(13, 1)), (t(14, 0), t(20, 1))], &[(360, 30)]),
            0
        );
    }

    #[test]
    fn compute_day_auto_break_adjacent_three_entries_count_as_one_block() {
        // 8:00–10:00, 10:00–13:00, 13:00–16:00 → one 8 h block
        assert_eq!(
            compute_day_auto_break(
                &[
                    (t(8, 0), t(10, 0)),
                    (t(10, 0), t(13, 0)),
                    (t(13, 0), t(16, 0))
                ],
                &[(360, 30)]
            ),
            30
        );
    }

    #[test]
    fn compute_day_auto_break_unsorted_entries_handled_correctly() {
        // Entries provided out of order; 12:00–16:00 listed before 8:00–12:00
        assert_eq!(
            compute_day_auto_break(&[(t(12, 0), t(16, 0)), (t(8, 0), t(12, 0))], &[(360, 30)]),
            30
        );
    }

    #[test]
    fn compute_day_auto_break_two_tier_highest_rule_wins() {
        // Two-tier example: tier 1 = 6 h / 30 min, tier 2 = 9 h / 45 min.
        let rules: &[(i64, i64)] = &[(360, 30), (540, 45)];

        // 10 h block → tier 2 applies → 45 min (NOT 30 + 45 = 75)
        assert_eq!(compute_day_auto_break(&[(t(8, 0), t(18, 0))], rules), 45);

        // 7 h block → only tier 1 applies → 30 min
        assert_eq!(compute_day_auto_break(&[(t(8, 0), t(15, 0))], rules), 30);

        // 5 h block → no tier applies → 0
        assert_eq!(compute_day_auto_break(&[(t(8, 0), t(13, 0))], rules), 0);
    }

    #[test]
    fn compute_day_auto_break_gap_between_blocks_covers_two_tier_requirement() {
        // Two separate long blocks: 10 h and 7 h, with a 60-min gap between them.
        // Day total worked = 17 h → tier 2 (45 min) required. The 60-min gap already
        // taken more than covers it → 0 deduction (not 45+30=75, the old per-block sum).
        let rules: &[(i64, i64)] = &[(360, 30), (540, 45)];
        assert_eq!(
            compute_day_auto_break(&[(t(0, 0), t(10, 0)), (t(11, 0), t(18, 0))], rules),
            0
        );
    }

    #[test]
    fn compute_day_auto_break_johanna_case_gap_falls_short_of_requirement() {
        // Real production scenario that exposed the per-block bug: 08:00–14:00
        // (exactly 6 h, doesn't itself trigger anything) + 14:30–18:00 (3.5 h), with a
        // 30-min logged gap. Day total worked = 9.5 h > 9 h → 45 min required. Only 30
        // min was actually taken as a break, so 15 min is deducted from the credited
        // total (570 → 555 min). The old per-block logic deducted 0, silently crediting
        // the full 9.5 h despite an insufficient break.
        let rules: &[(i64, i64)] = &[(360, 30), (540, 45)];
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(14, 0)), (t(14, 30), t(18, 0))], rules),
            15
        );
    }

    #[test]
    fn compute_day_auto_break_orell_case_generous_gap_needs_no_extra_deduction() {
        // Real production scenario, the mirror-image bug: 07:15–14:00 (6h45m) +
        // 18:00–23:45 (5h45m), with a real 4-hour gap. Day total worked = 12.5 h > 9 h
        // → 45 min required, but the 4-hour gap already taken far exceeds that → 0
        // deduction. The old per-block logic deducted 30 min (from the first block
        // alone) even though the person had already rested plenty that day.
        let rules: &[(i64, i64)] = &[(360, 30), (540, 45)];
        assert_eq!(
            compute_day_auto_break(&[(t(7, 15), t(14, 0)), (t(18, 0), t(23, 45))], rules),
            0
        );
    }

    #[test]
    fn compute_day_auto_break_partial_gap_deducts_only_the_shortfall() {
        // 7 h block + 1 h block with a 20-min gap. Day total worked = 8 h > 6 h → 30 min
        // required. 20 min was already taken, so only the 10-minute shortfall is
        // deducted — not the full 30 min again.
        assert_eq!(
            compute_day_auto_break(&[(t(8, 0), t(15, 0)), (t(15, 20), t(16, 20))], &[(360, 30)]),
            10
        );
    }

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    /// How many days a set of ranges costs inside one window on its own — the
    /// shape every caller used before the buckets were cut out of a single
    /// count.
    fn count_over_window(
        ranges: &[(NaiveDate, NaiveDate)],
        window_start: NaiveDate,
        window_end: NaiveDate,
        holidays: &HashSet<NaiveDate>,
        workdays_per_week: i16,
    ) -> f64 {
        counted_workdays(ranges, window_start, window_end, holidays, workdays_per_week).len() as f64
    }

    // ──────────────────────────────────────────────────────────────────────
    // counted_workdays
    // ──────────────────────────────────────────────────────────────────────

    /// 2026-05-11 is a Monday. A reduced-hours contract (3 days a week) taking
    /// the whole calendar week off spends three leave days, never five.
    #[test]
    fn counted_workdays_caps_a_full_week_at_the_weekly_quota() {
        let holidays = HashSet::new();
        let counted = counted_workdays(
            &[(day(2026, 5, 11), day(2026, 5, 15))],
            day(2026, 5, 1),
            day(2026, 5, 31),
            &holidays,
            3,
        );
        assert_eq!(counted.len(), 3);
        // The earliest days of the week are the ones that count, so the answer
        // does not depend on where a caller later splits the list.
        assert_eq!(
            counted,
            vec![day(2026, 5, 11), day(2026, 5, 12), day(2026, 5, 13)]
        );
    }

    /// The regression this helper exists for: counting "up to Wednesday" and
    /// "from Thursday" as two windows applied the weekly cap to each of them,
    /// billing one week off as 3 + 2 = 5 days. Splitting the counted days
    /// instead keeps the total at what the week can cost.
    #[test]
    fn counted_workdays_split_mid_week_still_totals_the_weekly_quota() {
        let holidays = HashSet::new();
        let counted = counted_workdays(
            &[(day(2026, 5, 11), day(2026, 5, 15))],
            day(2026, 5, 1),
            day(2026, 5, 31),
            &holidays,
            3,
        );
        let wednesday = day(2026, 5, 13);
        let taken = counted.iter().filter(|d| **d <= wednesday).count();
        let upcoming = counted.len() - taken;
        assert_eq!(taken + upcoming, 3);

        // Counting the two halves as separate windows is what used to happen.
        let separately = count_over_window(
            &[(day(2026, 5, 11), day(2026, 5, 15))],
            day(2026, 5, 1),
            wednesday,
            &holidays,
            3,
        ) + count_over_window(
            &[(day(2026, 5, 11), day(2026, 5, 15))],
            day(2026, 5, 14),
            day(2026, 5, 31),
            &holidays,
            3,
        );
        assert_eq!(separately, 5.0, "the old two-window count over-charged");
    }

    /// Two separate bookings inside one week are a union, not a sum: together
    /// they can still only cost what the week is worth.
    #[test]
    fn counted_workdays_unions_several_ranges_in_one_week() {
        let holidays = HashSet::new();
        let counted = counted_workdays(
            &[
                (day(2026, 5, 11), day(2026, 5, 12)),
                (day(2026, 5, 13), day(2026, 5, 15)),
            ],
            day(2026, 5, 1),
            day(2026, 5, 31),
            &holidays,
            4,
        );
        assert_eq!(counted.len(), 4);
    }

    /// Holidays and weekends never count, and a five-day contract is billed
    /// for exactly the workdays it covers.
    #[test]
    fn counted_workdays_skips_weekends_and_holidays() {
        let holidays = HashSet::from([day(2026, 5, 14)]);
        let counted = counted_workdays(
            &[(day(2026, 5, 11), day(2026, 5, 17))],
            day(2026, 5, 1),
            day(2026, 5, 31),
            &holidays,
            5,
        );
        assert_eq!(counted, vec![day(2026, 5, 11), day(2026, 5, 12), day(2026, 5, 13), day(2026, 5, 15)]);
    }

    /// An irregular schedule has no quota to cap against, so every covered
    /// non-holiday calendar day counts — weekends included.
    #[test]
    fn counted_workdays_counts_every_day_for_irregular_schedules() {
        let holidays = HashSet::from([day(2026, 5, 14)]);
        let counted = counted_workdays(
            &[(day(2026, 5, 11), day(2026, 5, 17))],
            day(2026, 5, 1),
            day(2026, 5, 31),
            &holidays,
            0,
        );
        assert_eq!(counted.len(), 6);
    }

    /// Ranges outside the window contribute nothing, and an empty list is 0.
    #[test]
    fn counted_workdays_ignores_ranges_outside_the_window() {
        let holidays = HashSet::new();
        assert!(counted_workdays(
            &[(day(2026, 4, 1), day(2026, 4, 30))],
            day(2026, 5, 1),
            day(2026, 5, 31),
            &holidays,
            5,
        )
        .is_empty());
        assert!(
            counted_workdays(&[], day(2026, 5, 1), day(2026, 5, 31), &holidays, 5)
                .is_empty()
        );
    }

    /// Whatever the inputs, every day `counted_workdays` returns is a real one:
    /// inside the window, covered by a range, not a holiday, a potential
    /// workday, listed once, in calendar order, and never more of them in a
    /// week than the quota allows. These are the properties every leave-day
    /// figure in the app rests on, so they are asserted directly rather than
    /// against a second copy of the arithmetic.
    #[test]
    fn counted_workdays_only_ever_returns_real_capped_days() {
        let base = day(2026, 5, 4); // Monday
        let window_end = base + Duration::days(20);
        let holidays = HashSet::from([base + Duration::days(9)]);
        for quota in 1i16..=5 {
            for length in 0..14i64 {
                let ranges = [(base + Duration::days(1), base + Duration::days(1 + length))];
                let counted = counted_workdays(&ranges, base, window_end, &holidays, quota);

                let unique: HashSet<NaiveDate> = counted.iter().copied().collect();
                assert_eq!(unique.len(), counted.len(), "no day counted twice");
                assert!(
                    counted.windows(2).all(|pair| pair[0] < pair[1]),
                    "days come back in calendar order"
                );
                for date in &counted {
                    assert!(*date >= base && *date <= window_end, "inside the window");
                    assert!(!holidays.contains(date), "never a holiday");
                    assert!(is_potential_workday(*date, quota), "a potential workday");
                    assert!(
                        ranges.iter().any(|(start, end)| date >= start && date <= end),
                        "covered by a range"
                    );
                }
                for monday in counted.iter().map(|d| week_monday(*d)).collect::<HashSet<_>>() {
                    let in_week = counted
                        .iter()
                        .filter(|date| week_monday(**date) == monday)
                        .count();
                    assert!(in_week <= quota as usize, "the weekly cap holds");
                }
            }
        }
    }

    #[test]
    fn counted_days_per_range_splits_a_shared_week_between_the_ranges() {
        // Mon-Tue and Thu-Fri of one week on a three-day contract. Counted
        // separately that is 2 + 2 = 4 days; the week only ever holds 3.
        let monday = day(2026, 5, 4);
        let ranges = [
            (monday, monday + Duration::days(1)),
            (monday + Duration::days(3), monday + Duration::days(4)),
        ];
        let counted = counted_days_per_range(
            &ranges,
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
            3,
        );
        assert_eq!(counted, vec![2.0, 1.0]);
        assert_eq!(counted.iter().sum::<f64>(), 3.0);
    }

    #[test]
    fn counted_days_per_range_is_independent_of_the_input_order() {
        let monday = day(2026, 5, 4);
        let later_first = [
            (monday + Duration::days(3), monday + Duration::days(4)),
            (monday, monday + Duration::days(1)),
        ];
        let counted = counted_days_per_range(
            &later_first,
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
            3,
        );
        // The earlier range keeps its own days wherever it sits in the input.
        assert_eq!(counted, vec![1.0, 2.0]);
    }

    #[test]
    fn counted_days_per_range_leaves_a_week_within_quota_alone() {
        let monday = day(2026, 5, 4);
        let ranges = [
            (monday, monday + Duration::days(1)),
            (monday + Duration::days(2), monday + Duration::days(4)),
        ];
        let counted = counted_days_per_range(
            &ranges,
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
            5,
        );
        assert_eq!(counted, vec![2.0, 3.0]);
    }

    #[test]
    fn counted_days_per_range_totals_what_counted_workdays_counts() {
        let monday = day(2026, 5, 4);
        let window_end = monday + Duration::days(20);
        let holidays = HashSet::from([monday + Duration::days(9)]);
        for quota in 1i16..=5 {
            for offset in 0..10i64 {
                let ranges = [
                    (monday, monday + Duration::days(2)),
                    (
                        monday + Duration::days(offset),
                        monday + Duration::days(offset + 3),
                    ),
                ];
                let per_range =
                    counted_days_per_range(&ranges, monday, window_end, &holidays, quota);
                let union = counted_workdays(&ranges, monday, window_end, &holidays, quota).len();
                assert_eq!(
                    per_range.iter().sum::<f64>(),
                    union as f64,
                    "quota {quota}, offset {offset}: the rows must add up to the union"
                );
            }
        }
    }

    #[test]
    fn counted_days_per_range_handles_no_ranges() {
        let monday = day(2026, 5, 4);
        assert!(counted_days_per_range(
            &[],
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
            5
        )
        .is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Contract-day leave accounting. Every expectation below is a statement of
    // what the rule should produce; none of it is wired into the app yet.
    //
    // Weeks used throughout: 2026-05-04 is a Monday, 2026-05-08 the Friday.
    // ─────────────────────────────────────────────────────────────────────────

    /// Week days as an absence set, by offset from the Monday (0 = Monday).
    fn days_of(monday: NaiveDate, offsets: &[i64]) -> HashSet<NaiveDate> {
        offsets
            .iter()
            .map(|offset| monday + Duration::days(*offset))
            .collect()
    }

    /// A start date long before every week under test, so the existing cases
    /// read as they did before the contract start became a parameter.
    const LONG_HIRED: (i32, u32, u32) = (2000, 1, 1);

    fn accounting(
        monday: NaiveDate,
        holiday_offsets: &[i64],
        absence_offsets: &[i64],
        weekly_hours: f64,
        workdays_per_week: i16,
    ) -> WeekLeaveAccounting {
        week_leave_accounting(
            monday,
            day(LONG_HIRED.0, LONG_HIRED.1, LONG_HIRED.2),
            &days_of(monday, holiday_offsets),
            &days_of(monday, absence_offsets),
            weekly_hours,
            workdays_per_week,
        )
    }

    #[test]
    fn a_contract_day_is_the_week_divided_by_the_contracted_days() {
        assert_eq!(contract_day_minutes(40.0, 5), 480);
        assert_eq!(contract_day_minutes(24.0, 3), 480);
        assert_eq!(contract_day_minutes(32.0, 4), 480);
        assert_eq!(contract_day_minutes(20.0, 5), 240);
        // A half-day contract still divides by its own days, not by five.
        assert_eq!(contract_day_minutes(18.0, 3), 360);
    }

    #[test]
    fn a_contract_day_is_zero_without_a_usable_contract() {
        assert_eq!(contract_day_minutes(40.0, 0), 0);
        assert_eq!(contract_day_minutes(0.0, 5), 0);
        assert_eq!(contract_day_minutes(f64::NAN, 5), 0);
        assert_eq!(contract_day_minutes(-8.0, 5), 0);
    }

    // ── The defect this rule exists to fix ───────────────────────────────────

    #[test]
    fn three_days_off_buy_the_whole_week_on_a_three_day_contract() {
        let monday = day(2026, 5, 4);
        // Monday to Wednesday: the person's three working days.
        let partial = accounting(monday, &[], &[0, 1, 2], 24.0, 3);
        assert_eq!(partial.charged_dates.len(), 3);
        assert_eq!(
            partial.remaining_target_min, 0,
            "three leave days buy out a three-day week, leaving nothing to work"
        );

        // Monday to Friday: the whole calendar week, same three leave days.
        let whole = accounting(monday, &[], &[0, 1, 2, 3, 4], 24.0, 3);
        assert_eq!(whole.charged_dates.len(), 3, "the week cannot cost a fourth");
        assert_eq!(whole.remaining_target_min, 0);

        // The two bookings must be indistinguishable, which is the whole point.
        assert_eq!(partial.remaining_target_min, whole.remaining_target_min);
        assert_eq!(partial.charged_dates.len(), whole.charged_dates.len());
    }

    #[test]
    fn a_single_day_off_costs_a_third_of_a_three_day_week() {
        let week = accounting(day(2026, 5, 4), &[], &[0], 24.0, 3);
        assert_eq!(week.charged_dates.len(), 1);
        // 24 hours less one eight-hour contract day.
        assert_eq!(week.remaining_target_min, 960);
    }

    #[test]
    fn two_days_off_leave_one_contract_day_to_work() {
        let week = accounting(day(2026, 5, 4), &[], &[0, 1], 24.0, 3);
        assert_eq!(week.charged_dates.len(), 2);
        assert_eq!(week.remaining_target_min, 480);
    }

    // ── Full-time contracts must not move at all ─────────────────────────────

    /// A five-day contract must be untouched by this rule, down to the minute.
    /// The awkward weekly-hour values are the ones real contracts carry: they
    /// do not divide evenly by five, and rounding the week instead of the day
    /// moved them by two minutes a week.
    #[test]
    fn a_five_day_contract_keeps_exactly_the_behaviour_it_has_today() {
        let monday = day(2026, 5, 4);
        for weekly_hours in [40.0f64, 37.5, 20.0, 33.54, 23.4, 11.7, 38.9] {
            let per_day = (weekly_hours / 5.0 * 60.0).round() as i64;
            for taken in 0..=5usize {
                let offsets: Vec<i64> = (0..taken as i64).collect();
                let week = accounting(monday, &[], &offsets, weekly_hours, 5);
                assert_eq!(week.charged_dates.len(), taken);
                assert_eq!(
                    week.remaining_target_min,
                    (5 - taken as i64) * per_day,
                    "{weekly_hours}h with {taken} days off must match the \
                     per-day target the app computes today"
                );
            }
        }
    }

    /// A week nobody was away in must not move by a single minute, on any
    /// contract. Real part-time hours divide unevenly, and the first draft
    /// shifted an untouched week by one minute on 23.4 hours over four days and
    /// by two minutes on 31.2 hours over four days.
    #[test]
    fn a_week_with_no_absence_keeps_todays_target_on_every_contract() {
        let monday = day(2026, 5, 4);
        for weekly_hours in [40.0f64, 23.4, 31.2, 33.54, 24.0, 11.7, 18.0] {
            for workdays_per_week in 1i16..=7 {
                let potential = i64::from(potential_workdays_per_week(workdays_per_week));
                let today_week =
                    potential * (weekly_hours / potential as f64 * 60.0).round() as i64;
                let week = accounting(monday, &[], &[], weekly_hours, workdays_per_week);
                assert_eq!(
                    week.remaining_target_min, today_week,
                    "{weekly_hours}h over {workdays_per_week} days: an untouched \
                     week must read exactly as it does today"
                );
                assert!(week.charged_dates.is_empty());
            }
        }
    }

    /// Taking every day the contract holds leaves nothing to work, whatever the
    /// hours round to.
    #[test]
    fn a_fully_taken_contract_week_leaves_no_minute_behind() {
        let monday = day(2026, 5, 4);
        for weekly_hours in [40.0f64, 23.4, 31.2, 33.54, 24.0, 11.7, 18.0] {
            for workdays_per_week in 1i16..=5 {
                let offsets: Vec<i64> = (0..i64::from(workdays_per_week)).collect();
                let week = accounting(monday, &[], &offsets, weekly_hours, workdays_per_week);
                assert_eq!(
                    week.remaining_target_min, 0,
                    "{weekly_hours}h over {workdays_per_week} days: the whole \
                     contract week was taken"
                );
                assert_eq!(week.charged_dates.len(), workdays_per_week as usize);
            }
        }
    }

    #[test]
    fn a_five_day_contract_matches_on_clean_hours_too() {
        let monday = day(2026, 5, 4);
        for (weekly_hours, per_day) in [(40.0, 480), (37.5, 450), (20.0, 240)] {
            for taken in 0..=5usize {
                let offsets: Vec<i64> = (0..taken as i64).collect();
                let week = accounting(monday, &[], &offsets, weekly_hours, 5);
                assert_eq!(
                    week.charged_dates.len(),
                    taken,
                    "{weekly_hours}h: every booked weekday is charged"
                );
                let expected = (weekly_hours * 60.0).round() as i64 - taken as i64 * per_day;
                assert_eq!(
                    week.remaining_target_min,
                    expected.max(0),
                    "{weekly_hours}h, {taken} days off: the target is the untouched days"
                );
            }
        }
    }

    // ── Holidays ─────────────────────────────────────────────────────────────

    #[test]
    fn a_holiday_costs_a_contract_day_and_no_leave() {
        let week = accounting(day(2026, 5, 4), &[0], &[], 24.0, 3);
        assert!(week.charged_dates.is_empty(), "a holiday is not leave");
        assert_eq!(week.remaining_target_min, 960);
    }

    #[test]
    fn a_holiday_beside_the_booked_days_grants_no_discount() {
        // Monday is a public holiday; Tuesday to Friday are booked off. The
        // holiday is not one of the booked days, so it saves no leave: the
        // contract does not pin which weekdays are worked, and a Monday the
        // person may never have worked cannot be assumed to have spared them
        // a day.
        let week = accounting(day(2026, 5, 4), &[0], &[1, 2, 3, 4], 24.0, 3);
        assert_eq!(week.charged_dates.len(), 3, "the full three days are spent");
        assert_eq!(week.remaining_target_min, 0, "the week is fully covered");
    }

    #[test]
    fn a_holiday_under_the_booked_days_does_save_one() {
        // A five-day contract booking the whole week, with Monday a holiday.
        // Monday is one of the booked days and costs nothing, so four leave
        // days cover the week instead of five.
        let week = accounting(day(2026, 5, 4), &[0], &[0, 1, 2, 3, 4], 40.0, 5);
        assert_eq!(week.charged_dates.len(), 4);
        assert_eq!(week.remaining_target_min, 0);
    }

    #[test]
    fn a_holiday_covered_by_an_absence_is_never_charged_twice() {
        let monday = day(2026, 5, 4);
        let week = accounting(monday, &[1], &[0, 1, 2], 24.0, 3);
        assert_eq!(
            week.charged_dates,
            vec![monday, monday + Duration::days(2)],
            "Tuesday is a holiday, so only Monday and Wednesday cost leave"
        );
        assert_eq!(week.remaining_target_min, 0);
    }

    #[test]
    fn a_week_of_nothing_but_holidays_costs_no_leave_and_no_work() {
        let week = accounting(day(2026, 5, 4), &[0, 1, 2, 3, 4], &[0, 1, 2, 3, 4], 24.0, 3);
        assert!(week.charged_dates.is_empty());
        assert_eq!(week.remaining_target_min, 0);
    }

    // ── Four-day contracts, the shape production actually holds ──────────────

    #[test]
    fn a_four_day_contract_is_bought_out_by_four_days() {
        let monday = day(2026, 5, 4);
        let four = accounting(monday, &[], &[0, 1, 2, 3], 32.0, 4);
        assert_eq!(four.charged_dates.len(), 4);
        assert_eq!(four.remaining_target_min, 0);

        let whole = accounting(monday, &[], &[0, 1, 2, 3, 4], 32.0, 4);
        assert_eq!(whole.charged_dates.len(), 4, "the fifth weekday is free");
        assert_eq!(whole.remaining_target_min, 0);
    }

    #[test]
    fn a_four_day_contract_keeps_one_day_of_work_after_three_days_off() {
        let week = accounting(day(2026, 5, 4), &[], &[0, 1, 2], 32.0, 4);
        assert_eq!(week.charged_dates.len(), 3);
        assert_eq!(week.remaining_target_min, 480);
    }

    // ── Weekends and longer weeks ────────────────────────────────────────────

    #[test]
    fn a_weekend_day_costs_nothing_on_a_weekday_contract() {
        // Saturday and Sunday are outside a 1-5 day contract's pool.
        let week = accounting(day(2026, 5, 4), &[], &[5, 6], 24.0, 3);
        assert!(week.charged_dates.is_empty());
        assert_eq!(week.remaining_target_min, 1440);
    }

    #[test]
    fn a_six_day_contract_reaches_into_saturday() {
        let monday = day(2026, 5, 4);
        let week = accounting(monday, &[], &[5], 36.0, 6);
        assert_eq!(week.charged_dates, vec![monday + Duration::days(5)]);
        assert_eq!(week.remaining_target_min, 1800);
    }

    #[test]
    fn a_seven_day_contract_reaches_into_sunday() {
        let monday = day(2026, 5, 4);
        let week = accounting(monday, &[], &[6], 35.0, 7);
        assert_eq!(week.charged_dates, vec![monday + Duration::days(6)]);
        assert_eq!(week.remaining_target_min, 1800);
    }

    // ── Degenerate contracts ─────────────────────────────────────────────────

    #[test]
    fn a_contract_without_hours_leaves_no_target_but_still_spends_leave() {
        let week = accounting(day(2026, 5, 4), &[], &[0, 1], 0.0, 3);
        assert_eq!(week.charged_dates.len(), 2, "the days are still taken off");
        assert_eq!(week.remaining_target_min, 0);
    }

    #[test]
    fn a_contract_without_a_workday_pool_charges_every_booked_day() {
        let monday = day(2026, 5, 4);
        let week = accounting(monday, &[], &[0, 5, 6], 0.0, 0);
        assert_eq!(
            week.charged_dates.len(),
            3,
            "an irregular schedule has no weekday pool and no quota"
        );
        assert_eq!(week.remaining_target_min, 0);
    }

    // ── Across a window: months, and the turn of the year ────────────────────

    #[test]
    fn counted_leave_days_charges_a_new_year_week_once_not_once_per_year() {
        // Monday 2025-12-29 to Friday 2026-01-02, holidays deliberately left
        // out so this measures the year boundary alone. Under today's rule the
        // two halves are charged separately: three days from 2025 plus two
        // from 2026, five for one week a three-day contract owes three for.
        let monday = day(2025, 12, 29);
        let ranges = [(monday, day(2026, 1, 2))];
        let in_2025 = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2025, 1, 1),
            day(2025, 12, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        let in_2026 = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2026, 1, 1),
            day(2026, 12, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        assert_eq!(
            in_2025.len() + in_2026.len(),
            3,
            "the week costs three days in total: {in_2025:?} / {in_2026:?}"
        );
        // The earliest days count first, so this week's three all sit in 2025.
        assert_eq!(
            in_2025,
            vec![monday, monday + Duration::days(1), monday + Duration::days(2)]
        );
        assert!(in_2026.is_empty());
    }

    #[test]
    fn counted_leave_days_spills_into_the_new_year_when_the_old_one_runs_short() {
        // Monday 2024-12-30 to Friday 2025-01-03, holidays again left out. Only
        // Monday and Tuesday fall in 2024, so the third day the week costs has
        // to come out of 2025 — and exactly one day does.
        let monday = day(2024, 12, 30);
        let ranges = [(monday, day(2025, 1, 3))];
        let in_2024 = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2024, 1, 1),
            day(2024, 12, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        let in_2025 = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2025, 1, 1),
            day(2025, 12, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        assert_eq!(in_2024, vec![monday, monday + Duration::days(1)]);
        assert_eq!(in_2025, vec![day(2025, 1, 1)]);
        assert_eq!(in_2024.len() + in_2025.len(), 3, "still three days in total");
    }

    #[test]
    fn a_new_year_week_with_its_holiday_costs_three_days_not_four() {
        // The case that actually occurs in Germany: whenever weekdays straddle
        // the turn of the year, New Year's Day is one of them and is a public
        // holiday. It is inside the booked range, so it costs nothing, and the
        // week's three days go on the remaining weekdays.
        //
        // Today's rule charges four for this week: three from 2025 (Monday to
        // Wednesday) plus one from 2026 (the Friday), because each side of the
        // boundary is capped on its own.
        let monday = day(2025, 12, 29);
        let holidays = HashSet::from([day(2026, 1, 1)]);
        let ranges = [(monday, day(2026, 1, 2))];
        let in_2025 = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2025, 1, 1),
            day(2025, 12, 31),
            &holidays,
            24.0,
            3,
        );
        let in_2026 = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2026, 1, 1),
            day(2026, 12, 31),
            &holidays,
            24.0,
            3,
        );
        assert_eq!(
            in_2025,
            vec![monday, monday + Duration::days(1), monday + Duration::days(2)]
        );
        assert!(in_2026.is_empty());
        assert_eq!(in_2025.len() + in_2026.len(), 3);

        // And the week is fully bought out: nothing is left to work.
        let absence_days: HashSet<NaiveDate> = (0..5i64)
            .map(|offset| monday + Duration::days(offset))
            .collect();
        let week = week_leave_accounting(monday, day(2000, 1, 1), &holidays, &absence_days, 24.0, 3);
        assert_eq!(week.remaining_target_min, 0);
    }

    #[test]
    fn counted_leave_days_matches_a_week_inside_one_year() {
        // The same absence shape, well away from any boundary, must cost the
        // same three days — that equality is the defect's absence.
        let monday = day(2026, 5, 4);
        let ranges = [(monday, monday + Duration::days(4))];
        let charged = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2026, 1, 1),
            day(2026, 12, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        assert_eq!(charged.len(), 3);
    }

    #[test]
    fn counted_leave_days_charges_a_week_once_across_a_month_boundary() {
        // Monday 2026-08-31 to Friday 2026-09-04.
        let monday = day(2026, 8, 31);
        let ranges = [(monday, monday + Duration::days(4))];
        let august = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2026, 8, 1),
            day(2026, 8, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        let september = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2026, 9, 1),
            day(2026, 9, 30),
            &HashSet::new(),
            24.0,
            3,
        );
        assert_eq!(august.len() + september.len(), 3);
        assert_eq!(august, vec![monday]);
        assert_eq!(september.len(), 2);
    }

    #[test]
    fn counted_leave_days_unions_two_bookings_inside_one_week() {
        let monday = day(2026, 5, 4);
        let ranges = [
            (monday, monday + Duration::days(1)),
            (monday + Duration::days(2), monday + Duration::days(4)),
        ];
        let charged = counted_leave_days(
            &ranges,
            day(2000, 1, 1),
            day(2026, 5, 1),
            day(2026, 5, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        assert_eq!(charged.len(), 3, "two bookings still cost one week");
    }

    #[test]
    fn counted_leave_days_ignores_ranges_outside_the_window() {
        let charged = counted_leave_days(
            &[(day(2026, 3, 2), day(2026, 3, 6))],
            day(2000, 1, 1),
            day(2026, 5, 1),
            day(2026, 5, 31),
            &HashSet::new(),
            24.0,
            3,
        );
        assert!(charged.is_empty());
    }

    // ── The contract's own start date ────────────────────────────────────────

    #[test]
    fn days_before_the_contract_began_cost_nothing_at_all() {
        // Somebody hired on Wednesday 2026-07-01. Monday and Tuesday of that
        // week are before their first day.
        let monday = day(2026, 6, 29);
        let start = day(2026, 7, 1);
        let week = week_leave_accounting(
            monday,
            start,
            &HashSet::new(),
            &HashSet::new(),
            23.4,
            4,
        );
        // Three weekdays exist for them, each worth today's potential day of
        // 281 minutes, and two of the contract's four days are gone with the
        // days that never belonged to it.
        // Three weekdays exist for them, each worth today's 281 minutes. The
        // Monday and Tuesday before they were hired take nothing off, because
        // they were never part of this week for this employee.
        assert_eq!(week.remaining_target_min, 3 * 281);
        assert!(week.charged_dates.is_empty());
    }

    #[test]
    fn a_first_week_charges_no_leave_before_the_start_date() {
        let monday = day(2026, 6, 29);
        let start = day(2026, 7, 1);
        // An absence covering the whole week, which cannot really happen but
        // proves the days before the start are never billed.
        let absences: HashSet<NaiveDate> =
            (0..5i64).map(|offset| monday + Duration::days(offset)).collect();
        let week =
            week_leave_accounting(monday, start, &HashSet::new(), &absences, 23.4, 4);
        assert_eq!(
            week.charged_dates,
            vec![start, start + Duration::days(1), start + Duration::days(2)],
            "only Wednesday, Thursday and Friday can cost leave"
        );
        assert_eq!(week.remaining_target_min, 0);
    }

    #[test]
    fn a_week_entirely_before_the_start_date_is_empty() {
        let week = week_leave_accounting(
            day(2026, 6, 22),
            day(2026, 7, 1),
            &HashSet::new(),
            &HashSet::new(),
            23.4,
            4,
        );
        assert!(week.charged_dates.is_empty());
        assert_eq!(week.remaining_target_min, 0);
    }

    #[test]
    fn counted_leave_days_skips_everything_before_the_start_date() {
        let charged = counted_leave_days(
            &[(day(2026, 6, 29), day(2026, 7, 3))],
            day(2026, 7, 1),
            day(2026, 1, 1),
            day(2026, 12, 31),
            &HashSet::new(),
            23.4,
            4,
        );
        assert_eq!(
            charged,
            vec![day(2026, 7, 1), day(2026, 7, 2), day(2026, 7, 3)]
        );
    }

    /// The one thing a caller has to get right, written down as a test so it
    /// cannot be forgotten: a window-scoped query hands over only the absences
    /// overlapping its own period, and two bookings on one account either side
    /// of a boundary inside one week then get priced twice.
    #[test]
    fn a_window_that_cannot_see_the_whole_week_charges_it_twice() {
        let old_year = (day(2025, 12, 29), day(2025, 12, 30));
        let new_year = (day(2026, 1, 1), day(2026, 1, 2));
        let hired = day(2020, 1, 1);
        let no_holidays = HashSet::new();

        // Handed both bookings, each window prices the shared week correctly.
        let both = [old_year, new_year];
        let in_2025 = counted_leave_days(
            &both, hired, day(2025, 1, 1), day(2025, 12, 31), &no_holidays, 24.0, 3);
        let in_2026 = counted_leave_days(
            &both, hired, day(2026, 1, 1), day(2026, 12, 31), &no_holidays, 24.0, 3);
        assert_eq!(in_2025.len() + in_2026.len(), 3, "the week is worth three days");

        // Handed only its own booking, each window charges the week on its own.
        let alone_2025 = counted_leave_days(
            &[old_year], hired, day(2025, 1, 1), day(2025, 12, 31), &no_holidays, 24.0, 3);
        let alone_2026 = counted_leave_days(
            &[new_year], hired, day(2026, 1, 1), day(2026, 12, 31), &no_holidays, 24.0, 3);
        assert_eq!(
            alone_2025.len() + alone_2026.len(),
            4,
            "four days for a three-day week — the caller must fetch both bookings"
        );
    }

    #[test]
    fn counted_leave_days_handles_an_empty_or_inverted_request() {
        assert!(counted_leave_days(
            &[],
            day(2000, 1, 1),
            day(2026, 5, 1),
            day(2026, 5, 31),
            &HashSet::new(),
            24.0,
            3
        )
        .is_empty());
        assert!(counted_leave_days(
            &[(day(2026, 5, 4), day(2026, 5, 8))],
            day(2000, 1, 1),
            day(2026, 5, 31),
            day(2026, 5, 1),
            &HashSet::new(),
            24.0,
            3
        )
        .is_empty());
    }

    #[test]
    fn counted_leave_days_never_charges_a_week_more_than_the_contract_holds() {
        // Sweep a year of bookings of every length against every contract and
        // check the invariant the rule exists for: no ISO week is ever charged
        // more leave days than the contract has working days in it.
        let year_start = day(2026, 1, 1);
        let holidays = HashSet::from([day(2026, 1, 1), day(2026, 4, 3), day(2026, 5, 1)]);
        for workdays_per_week in 1i16..=5 {
            for start_offset in 0..90i64 {
                for length in 0..16i64 {
                    let start = year_start + Duration::days(start_offset);
                    let ranges = [(start, start + Duration::days(length))];
                    let charged = counted_leave_days(
                        &ranges,
                        day(2000, 1, 1),
                        year_start,
                        day(2026, 12, 31),
                        &holidays,
                        24.0,
                        workdays_per_week,
                    );
                    let mut per_week: std::collections::HashMap<NaiveDate, usize> =
                        std::collections::HashMap::new();
                    for date in &charged {
                        *per_week.entry(week_monday(*date)).or_insert(0) += 1;
                    }
                    for (monday, count) in per_week {
                        assert!(
                            count <= workdays_per_week as usize,
                            "week of {monday} charged {count} days on a \
                             {workdays_per_week}-day contract"
                        );
                    }
                    for date in &charged {
                        assert!(!holidays.contains(date), "a holiday is never charged");
                        assert!(
                            is_potential_workday(*date, workdays_per_week),
                            "only potential workdays are charged"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_target_left_never_exceeds_the_week_and_never_falls_below_zero() {
        let monday = day(2026, 5, 4);
        for workdays_per_week in 1i16..=7 {
            for holiday_count in 0..=5i64 {
                for absence_count in 0..=7i64 {
                    let holiday_offsets: Vec<i64> = (0..holiday_count).collect();
                    let absence_offsets: Vec<i64> = (0..absence_count).collect();
                    let week = accounting(
                        monday,
                        &holiday_offsets,
                        &absence_offsets,
                        24.0,
                        workdays_per_week,
                    );
                    // The week is worth its own rounded contract days, which
                    // is not always exactly the contract's hours: 24 hours
                    // over seven days rounds to 206 minutes a day and so to
                    // 1442 minutes a week. Today's per-day target rounds the
                    // same way, so this is the bound to hold against.
                    let week_total = i64::from(workdays_per_week)
                        * contract_day_minutes(24.0, workdays_per_week);
                    assert!(week.remaining_target_min >= 0);
                    assert!(week.remaining_target_min <= week_total);
                    assert!(week.charged_dates.len() <= workdays_per_week as usize);
                    assert!(
                        week.charged_dates.windows(2).all(|pair| pair[0] < pair[1]),
                        "charged days come back in calendar order"
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Fixed working weekdays.
    //
    // The cases below are the questions this design was worked out from, with
    // the two real part-time contracts as the worked examples:
    //   Sabine  — Monday to Thursday, 23.4 hours a week, contract day 351 min.
    //   Orell   — Tuesday to Friday, 31.2 hours a week, contract day 468 min.
    // 2026-05-04 is a Monday; 2026-07-20, 2026-08-24 and 2026-12-21 are too.
    // ─────────────────────────────────────────────────────────────────────────

    const MON: u8 = 1;
    const TUE: u8 = 2;
    const WED: u8 = 3;
    const THU: u8 = 4;
    const FRI: u8 = 5;
    // Saturday and Sunday exist here only so the tests can show they are
    // refused; no contract is recorded on them.
    const SAT: u8 = 6;
    const SUN: u8 = 7;

    fn sabine() -> WorkSchedule {
        WorkSchedule::fixed(&[MON, TUE, WED, THU]).unwrap()
    }
    fn orell() -> WorkSchedule {
        WorkSchedule::fixed(&[TUE, WED, THU, FRI]).unwrap()
    }
    fn full_time() -> WorkSchedule {
        WorkSchedule::fixed(&[MON, TUE, WED, THU, FRI]).unwrap()
    }
    fn hired_long_ago() -> NaiveDate {
        day(2000, 1, 1)
    }
    /// One pattern in force for all time, which is what most cases need.
    fn always(schedule: WorkSchedule) -> WorkScheduleHistory {
        WorkScheduleHistory::new(
            vec![(day(1900, 1, 1), schedule)],
            WorkSchedule::without_fixed_days(5),
        )
    }
    fn span(monday: NaiveDate, offsets: &[i64]) -> HashSet<NaiveDate> {
        offsets.iter().map(|o| monday + Duration::days(*o)).collect()
    }
    fn target(
        schedule: &WorkSchedule,
        monday: NaiveDate,
        holiday_offsets: &[i64],
        absence_offsets: &[i64],
        weekly_hours: f64,
    ) -> i64 {
        scheduled_week_target_min(
            &always(*schedule),
            monday,
            hired_long_ago(),
            &span(monday, holiday_offsets),
            &span(monday, absence_offsets),
            weekly_hours,
        )
    }

    // ── The schedule itself ──────────────────────────────────────────────────

    #[test]
    fn a_schedule_needs_at_least_one_real_weekday() {
        assert!(WorkSchedule::fixed(&[]).is_none());
        assert!(WorkSchedule::fixed(&[0]).is_none());
        assert!(WorkSchedule::fixed(&[8]).is_none());
        assert!(WorkSchedule::fixed(&[6, 7]).is_none(), "no weekend-only contract");
        assert!(WorkSchedule::fixed(&[0, 8, 9]).is_none());
        assert!(WorkSchedule::fixed(&[MON]).is_some());
    }

    #[test]
    fn a_repeated_weekday_is_still_one_day() {
        let schedule = WorkSchedule::fixed(&[MON, MON, TUE]).unwrap();
        assert_eq!(schedule.days_per_week(), 2);
    }

    #[test]
    fn a_schedule_knows_its_own_days() {
        let monday = day(2026, 5, 4);
        for (offset, expected) in [(0, true), (1, true), (2, true), (3, true), (4, false)] {
            assert_eq!(
                sabine().covers(monday + Duration::days(offset)),
                expected,
                "Sabine, offset {offset}"
            );
        }
        for (offset, expected) in [(0, false), (1, true), (2, true), (3, true), (4, true)] {
            assert_eq!(
                orell().covers(monday + Duration::days(offset)),
                expected,
                "Orell, offset {offset}"
            );
        }
        assert!(!sabine().covers(monday + Duration::days(5)), "no Saturday");
        assert!(!sabine().covers(monday + Duration::days(6)), "no Sunday");
    }

    #[test]
    fn a_weekend_schedule_is_refused() {
        // Work is recorded Monday to Friday and the stored pattern is limited
        // to those days, so a weekend schedule must not be constructible.
        assert!(WorkSchedule::fixed(&[SAT, SUN]).is_none());
        assert!(WorkSchedule::fixed(&[SAT]).is_none());
        // A weekend day alongside real ones is simply dropped.
        let schedule = WorkSchedule::fixed(&[MON, SAT]).unwrap();
        assert_eq!(schedule.days_per_week(), 1);
        assert!(!schedule.covers(day(2026, 5, 9)), "Saturday is never worked");
    }

    #[test]
    fn a_contract_day_is_the_week_divided_by_the_days_actually_worked() {
        assert_eq!(sabine().day_minutes(23.4), 351);
        assert_eq!(orell().day_minutes(31.2), 468);
        assert_eq!(full_time().day_minutes(40.0), 480);
        assert_eq!(full_time().day_minutes(11.7), 140);
    }

    #[test]
    fn a_contract_day_is_zero_without_usable_hours() {
        assert_eq!(sabine().day_minutes(0.0), 0);
        assert_eq!(sabine().day_minutes(-8.0), 0);
        assert_eq!(sabine().day_minutes(f64::NAN), 0);
    }

    #[test]
    fn a_contract_without_recorded_days_keeps_the_old_pool() {
        // The assistants' case: no pattern, so the old questions are answered
        // the old way.
        let monday = day(2026, 5, 4);
        let assistant = WorkSchedule::without_fixed_days(7);
        assert!(!assistant.has_fixed_days());
        assert_eq!(assistant.days_per_week(), 7);
        assert!(assistant.covers(monday + Duration::days(6)), "Sunday counts");

        let five = WorkSchedule::without_fixed_days(5);
        assert_eq!(five.days_per_week(), 5);
        assert!(five.covers(monday));
        assert!(!five.covers(monday + Duration::days(5)));
    }

    // ── A full-time contract must not move ───────────────────────────────────

    #[test]
    fn a_five_day_schedule_reads_exactly_as_the_app_does_today() {
        let monday = day(2026, 5, 4);
        for weekly_hours in [40.0f64, 36.66, 33.54, 33.15, 11.7, 37.5] {
            let today_per_day = (weekly_hours / 5.0 * 60.0).round() as i64;
            assert_eq!(full_time().day_minutes(weekly_hours), today_per_day);
            for taken in 0..=5i64 {
                let offsets: Vec<i64> = (0..taken).collect();
                assert_eq!(
                    target(&full_time(), monday, &[], &offsets, weekly_hours),
                    (5 - taken) * today_per_day,
                    "{weekly_hours}h with {taken} days off"
                );
            }
        }
    }

    // ── Sabine: Monday to Thursday ───────────────────────────────────────────

    #[test]
    fn sabines_four_days_off_buy_her_whole_week() {
        let monday = day(2026, 5, 4);
        // Monday to Thursday booked off: every working day she has.
        assert_eq!(target(&sabine(), monday, &[], &[0, 1, 2, 3], 23.4), 0);
        // Monday to Friday booked off costs her the same, because Friday was
        // never hers to take.
        assert_eq!(target(&sabine(), monday, &[], &[0, 1, 2, 3, 4], 23.4), 0);
    }

    #[test]
    fn sabine_pays_nothing_for_a_friday() {
        let monday = day(2026, 5, 4);
        let charged = scheduled_leave_days(
            &always(sabine()),
            &[(monday + Duration::days(4), monday + Duration::days(4))],
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        );
        assert!(charged.is_empty(), "Friday is not one of her days");
        assert_eq!(
            target(&sabine(), monday, &[], &[4], 23.4),
            4 * 351,
            "and her week is untouched by it"
        );
    }

    #[test]
    fn sabines_open_friday_asks_for_no_work() {
        // Her real week of 2026-09-07: Monday to Thursday on holiday, Friday
        // neither booked nor absent. Today that Friday carries 281 minutes and
        // blocks the week; as one of her non-working days it carries nothing.
        let monday = day(2026, 9, 7);
        assert_eq!(target(&sabine(), monday, &[], &[0, 1, 2, 3], 23.4), 0);
    }

    #[test]
    fn sabine_loses_one_working_day_at_a_time() {
        let monday = day(2026, 5, 4);
        for taken in 0..=4i64 {
            let offsets: Vec<i64> = (0..taken).collect();
            assert_eq!(
                target(&sabine(), monday, &[], &offsets, 23.4),
                (4 - taken) * 351,
                "{taken} of her four days off"
            );
        }
    }

    #[test]
    fn sabines_week_is_worth_exactly_her_contract() {
        // 23.4 hours are 1404 minutes. Spreading them over five weekdays and
        // rounding gave 1405, one minute more than her contract says.
        assert_eq!(target(&sabine(), day(2026, 5, 4), &[], &[], 23.4), 1404);
    }

    // ── Orell: Tuesday to Friday ─────────────────────────────────────────────

    #[test]
    fn orells_monday_hours_are_pure_overtime() {
        // Monday carries no target for him, so anything booked on it is gain.
        let monday = day(2026, 5, 4);
        assert_eq!(
            target(&orell(), monday, &[], &[], 31.2),
            4 * 468,
            "his week asks for his four days and nothing for the Monday"
        );
        assert!(!orell().covers(monday));
    }

    #[test]
    fn orell_pays_nothing_for_a_monday_off() {
        let monday = day(2026, 5, 4);
        let charged = scheduled_leave_days(
            &always(orell()),
            &[(monday, monday)],
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        );
        assert!(charged.is_empty(), "he does not work Mondays");
        assert_eq!(target(&orell(), monday, &[], &[0], 31.2), 4 * 468);
    }

    #[test]
    fn orells_four_days_off_buy_his_whole_week() {
        let monday = day(2026, 5, 4);
        assert_eq!(target(&orell(), monday, &[], &[1, 2, 3, 4], 31.2), 0);
        assert_eq!(target(&orell(), monday, &[], &[0, 1, 2, 3, 4], 31.2), 0);
    }

    #[test]
    fn orells_friday_sick_note_still_counts() {
        // The real case: sick Thursday 2026-07-23 and Friday 2026-07-24. Both
        // are his working days, so the payroll report keeps both.
        let charged = scheduled_leave_days(
            &always(orell()),
            &[(day(2026, 7, 23), day(2026, 7, 24))],
            hired_long_ago(),
            day(2026, 1, 1),
            day(2026, 12, 31),
            &HashSet::new(),
        );
        assert_eq!(charged, vec![day(2026, 7, 23), day(2026, 7, 24)]);
    }

    #[test]
    fn the_same_week_costs_each_of_them_their_own_four_days() {
        // One calendar week booked off end to end. Both contracts have four
        // days, so both pay four — but not the same four.
        let monday = day(2026, 5, 4);
        let whole_week = [(monday, monday + Duration::days(6))];
        let hers = scheduled_leave_days(
            &always(sabine()), &whole_week, hired_long_ago(), monday,
            monday + Duration::days(6), &HashSet::new());
        let his = scheduled_leave_days(
            &always(orell()), &whole_week, hired_long_ago(), monday,
            monday + Duration::days(6), &HashSet::new());
        assert_eq!(hers.len(), 4);
        assert_eq!(his.len(), 4);
        assert_eq!(hers[0], monday, "her week starts on the Monday");
        assert_eq!(his[0], monday + Duration::days(1), "his starts on the Tuesday");
        assert_ne!(hers, his);
    }

    // ── Public holidays ──────────────────────────────────────────────────────

    #[test]
    fn a_holiday_on_a_working_day_costs_the_day_and_no_leave() {
        let monday = day(2026, 5, 4);
        assert_eq!(target(&sabine(), monday, &[0], &[], 23.4), 3 * 351);
        let charged = scheduled_leave_days(
            &always(sabine()),
            &[(monday, monday)],
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &span(monday, &[0]),
        );
        assert!(charged.is_empty(), "a holiday is never leave");
    }

    #[test]
    fn a_holiday_on_a_day_off_changes_nothing() {
        // Friday is not Sabine's day, so a Friday holiday passes her by.
        let monday = day(2026, 5, 4);
        assert_eq!(target(&sabine(), monday, &[4], &[], 23.4), 4 * 351);
        // And a Monday holiday passes Orell by.
        assert_eq!(target(&orell(), monday, &[0], &[], 31.2), 4 * 468);
    }

    #[test]
    fn a_holiday_inside_a_booked_week_saves_exactly_one_leave_day() {
        let monday = day(2026, 5, 4);
        let charged = scheduled_leave_days(
            &always(sabine()),
            &[(monday, monday + Duration::days(4))],
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &span(monday, &[1]),
        );
        assert_eq!(
            charged,
            vec![monday, monday + Duration::days(2), monday + Duration::days(3)],
            "Tuesday is a holiday, so three of her four days are spent"
        );
        assert_eq!(target(&sabine(), monday, &[1], &[0, 2, 3], 23.4), 0);
    }

    #[test]
    fn a_week_of_nothing_but_holidays_asks_for_nothing() {
        let monday = day(2026, 5, 4);
        assert_eq!(target(&sabine(), monday, &[0, 1, 2, 3], &[], 23.4), 0);
    }

    // ── The contract's own start date ────────────────────────────────────────

    #[test]
    fn days_before_the_first_working_day_carry_nothing() {
        // Hired on Wednesday 2026-07-01: Monday and Tuesday of that week are
        // before her time.
        let monday = day(2026, 6, 29);
        let start = day(2026, 7, 1);
        assert_eq!(
            scheduled_week_target_min(
                &always(sabine()), monday, start, &HashSet::new(), &HashSet::new(), 23.4),
            2 * 351,
            "Wednesday and Thursday are the working days she has that week"
        );
    }

    #[test]
    fn a_scheduled_week_entirely_before_the_start_date_is_empty() {
        assert_eq!(
            scheduled_week_target_min(
                &always(sabine()), day(2026, 6, 22), day(2026, 7, 1),
                &HashSet::new(), &HashSet::new(), 23.4),
            0
        );
    }

    #[test]
    fn no_leave_is_charged_before_the_start_date() {
        let charged = scheduled_leave_days(
            &always(sabine()),
            &[(day(2026, 6, 29), day(2026, 7, 2))],
            day(2026, 7, 1),
            day(2026, 1, 1),
            day(2026, 12, 31),
            &HashSet::new(),
        );
        assert_eq!(charged, vec![day(2026, 7, 1), day(2026, 7, 2)]);
    }

    // ── Month and year boundaries ────────────────────────────────────────────

    #[test]
    fn a_week_over_new_year_is_charged_once_across_the_two_years() {
        // Monday 2025-12-29 to Friday 2026-01-02, with New Year's Day a public
        // holiday. Under the old cap this week cost Sabine four days: three
        // from 2025 and one from 2026, each side capped on its own.
        let ranges = [(day(2025, 12, 29), day(2026, 1, 2))];
        let holidays = HashSet::from([day(2026, 1, 1)]);
        let in_2025 = scheduled_leave_days(
            &always(sabine()), &ranges, hired_long_ago(),
            day(2025, 1, 1), day(2025, 12, 31), &holidays);
        let in_2026 = scheduled_leave_days(
            &always(sabine()), &ranges, hired_long_ago(),
            day(2026, 1, 1), day(2026, 12, 31), &holidays);
        assert_eq!(in_2025, vec![day(2025, 12, 29), day(2025, 12, 30), day(2025, 12, 31)]);
        assert!(in_2026.is_empty(), "her Thursday that week is New Year's Day");
        assert_eq!(in_2025.len() + in_2026.len(), 3);
    }

    #[test]
    fn a_week_over_new_year_splits_by_the_day_each_belongs_to() {
        // Orell's Tuesday to Friday across the same turn of the year: Tuesday
        // and Wednesday fall in 2025, Thursday is New Year's Day, Friday is in
        // 2026.
        let ranges = [(day(2025, 12, 29), day(2026, 1, 2))];
        let holidays = HashSet::from([day(2026, 1, 1)]);
        let in_2025 = scheduled_leave_days(
            &always(orell()), &ranges, hired_long_ago(),
            day(2025, 1, 1), day(2025, 12, 31), &holidays);
        let in_2026 = scheduled_leave_days(
            &always(orell()), &ranges, hired_long_ago(),
            day(2026, 1, 1), day(2026, 12, 31), &holidays);
        assert_eq!(in_2025, vec![day(2025, 12, 30), day(2025, 12, 31)]);
        assert_eq!(in_2026, vec![day(2026, 1, 2)]);
        assert_eq!(in_2025.len() + in_2026.len(), 3);
    }

    #[test]
    fn two_bookings_either_side_of_a_boundary_are_not_double_charged() {
        // The case the whole-week rule needed a caller warning for. With the
        // days known, each window sees only its own days and no cap re-applies,
        // so passing one range per window gives the same answer as passing both.
        let old_year = (day(2025, 12, 29), day(2025, 12, 30));
        let new_year = (day(2026, 1, 1), day(2026, 1, 2));
        let both = [old_year, new_year];
        let apart_2025 = scheduled_leave_days(
            &always(sabine()), &[old_year], hired_long_ago(),
            day(2025, 1, 1), day(2025, 12, 31), &HashSet::new());
        let apart_2026 = scheduled_leave_days(
            &always(sabine()), &[new_year], hired_long_ago(),
            day(2026, 1, 1), day(2026, 12, 31), &HashSet::new());
        let together_2025 = scheduled_leave_days(
            &always(sabine()), &both, hired_long_ago(),
            day(2025, 1, 1), day(2025, 12, 31), &HashSet::new());
        let together_2026 = scheduled_leave_days(
            &always(sabine()), &both, hired_long_ago(),
            day(2026, 1, 1), day(2026, 12, 31), &HashSet::new());
        assert_eq!(apart_2025, together_2025);
        assert_eq!(apart_2026, together_2026);
        // Monday and Tuesday in the old year, Thursday in the new one. The
        // Friday of that week belongs to the booking but not to her contract,
        // so three days is the whole cost however the windows are cut.
        assert_eq!(apart_2025, vec![day(2025, 12, 29), day(2025, 12, 30)]);
        assert_eq!(apart_2026, vec![day(2026, 1, 1)]);
        assert_eq!(apart_2025.len() + apart_2026.len(), 3);
    }

    #[test]
    fn the_months_of_a_year_add_up_to_the_year() {
        // Sabine's real vacation, 2026-08-26 to 2026-09-10 plus 2026-09-14.
        // Under the old cap August and September added to thirteen days while
        // the year said twelve.
        let ranges = [
            (day(2026, 8, 26), day(2026, 9, 10)),
            (day(2026, 9, 14), day(2026, 9, 14)),
        ];
        let no_holidays = HashSet::new();
        let year = scheduled_leave_days(
            &always(sabine()), &ranges, hired_long_ago(),
            day(2026, 1, 1), day(2026, 12, 31), &no_holidays).len();
        let months: usize = (1..=12u32)
            .map(|month| {
                let first = NaiveDate::from_ymd_opt(2026, month, 1).unwrap();
                let last = NaiveDate::from_ymd_opt(
                    2026, month, last_day_of_month(2026, month)).unwrap();
                scheduled_leave_days(
                    &always(sabine()), &ranges, hired_long_ago(), first, last, &no_holidays).len()
            })
            .sum();
        assert_eq!(year, 11, "the Friday inside her range is not her day");
        assert_eq!(months, year, "every month together is the year");
    }

    #[test]
    fn orells_months_add_up_to_his_year_too() {
        let ranges = [(day(2026, 8, 24), day(2026, 9, 11))];
        let no_holidays = HashSet::new();
        let year = scheduled_leave_days(
            &always(orell()), &ranges, hired_long_ago(),
            day(2026, 1, 1), day(2026, 12, 31), &no_holidays).len();
        let august = scheduled_leave_days(
            &always(orell()), &ranges, hired_long_ago(),
            day(2026, 8, 1), day(2026, 8, 31), &no_holidays).len();
        let september = scheduled_leave_days(
            &always(orell()), &ranges, hired_long_ago(),
            day(2026, 9, 1), day(2026, 9, 30), &no_holidays).len();
        assert_eq!(year, 12);
        assert_eq!(august, 4, "his Mondays in August cost him nothing");
        assert_eq!(august + september, year);
    }

    // ── Sweeps ───────────────────────────────────────────────────────────────

    #[test]
    fn a_week_never_costs_more_leave_than_the_contract_has_days() {
        let holidays = HashSet::from([day(2026, 1, 1), day(2026, 4, 3), day(2026, 5, 1)]);
        for schedule in [sabine(), orell(), full_time(),
                         WorkSchedule::fixed(&[WED]).unwrap(),
                         WorkSchedule::fixed(&[MON, FRI]).unwrap()] {
            for start_offset in 0..120i64 {
                for length in 0..16i64 {
                    let from = day(2026, 1, 1) + Duration::days(start_offset);
                    let charged = scheduled_leave_days(
                        &always(schedule),
                        &[(from, from + Duration::days(length))],
                        hired_long_ago(),
                        day(2026, 1, 1),
                        day(2026, 12, 31),
                        &holidays,
                    );
                    let mut per_week: std::collections::HashMap<NaiveDate, usize> =
                        std::collections::HashMap::new();
                    for date in &charged {
                        *per_week.entry(week_monday(*date)).or_insert(0) += 1;
                        assert!(schedule.covers(*date), "only working days are charged");
                        assert!(!holidays.contains(date), "never a holiday");
                    }
                    for (_, count) in per_week {
                        assert!(count as u32 <= schedule.days_per_week());
                    }
                }
            }
        }
    }

    #[test]
    fn a_weeks_target_never_exceeds_the_contract_and_never_goes_negative() {
        let monday = day(2026, 5, 4);
        for schedule in [sabine(), orell(), full_time(),
                         WorkSchedule::fixed(&[TUE, THU]).unwrap()] {
            let full = i64::from(schedule.days_per_week()) * schedule.day_minutes(23.4);
            for holidays in 0..=5i64 {
                for absences in 0..=7i64 {
                    let value = target(
                        &schedule,
                        monday,
                        &(0..holidays).collect::<Vec<_>>(),
                        &(0..absences).collect::<Vec<_>>(),
                        23.4,
                    );
                    assert!(value >= 0);
                    assert!(value <= full);
                }
            }
        }
    }

    #[test]
    fn leave_days_and_the_target_always_agree_on_which_days_are_gone() {
        // Whatever a week loses to absence, the target loses exactly the same
        // number of contract days. The two must never disagree, because that
        // disagreement is the defect the whole design started from.
        let monday = day(2026, 5, 4);
        for schedule in [sabine(), orell(), full_time()] {
            let day_value = schedule.day_minutes(23.4);
            let full = i64::from(schedule.days_per_week()) * day_value;
            for absences in 0..=5i64 {
                let offsets: Vec<i64> = (0..absences).collect();
                let ranges: Vec<(NaiveDate, NaiveDate)> = offsets
                    .iter()
                    .map(|o| (monday + Duration::days(*o), monday + Duration::days(*o)))
                    .collect();
                let charged = scheduled_leave_days(
                    &always(schedule), &ranges, hired_long_ago(), monday,
                    monday + Duration::days(6), &HashSet::new());
                let value = target(&schedule, monday, &[], &offsets, 23.4);
                assert_eq!(
                    value,
                    full - charged.len() as i64 * day_value,
                    "{absences} days booked off"
                );
            }
        }
    }

    // ── The pattern's own history ────────────────────────────────────────────

    #[test]
    fn a_person_without_recorded_days_always_gets_the_fallback() {
        let history = WorkScheduleHistory::without_history(
            WorkSchedule::without_fixed_days(7));
        assert!(history.is_empty());
        assert!(!history.on(day(2026, 5, 4)).has_fixed_days());
        assert_eq!(history.on(day(2026, 5, 4)).days_per_week(), 7);
    }

    #[test]
    fn a_date_before_the_first_entry_gets_the_fallback() {
        let history = WorkScheduleHistory::new(
            vec![(day(2026, 7, 1), sabine())],
            WorkSchedule::without_fixed_days(5),
        );
        assert!(!history.on(day(2026, 6, 30)).has_fixed_days());
        assert_eq!(history.on(day(2026, 7, 1)), sabine(), "the first day counts");
    }

    #[test]
    fn a_change_of_working_days_leaves_the_past_alone() {
        // Sabine works Monday to Thursday from her first day, and moves to
        // Tuesday to Friday on 2027-03-01. Weeks before that date must keep
        // the pattern they were actually worked under.
        let history = WorkScheduleHistory::new(
            vec![(day(2026, 7, 1), sabine()), (day(2027, 3, 1), orell())],
            WorkSchedule::without_fixed_days(5),
        );
        assert_eq!(history.on(day(2026, 7, 1)), sabine());
        assert_eq!(history.on(day(2027, 1, 4)), sabine(), "a week well before");
        assert_eq!(history.on(day(2027, 2, 28)), sabine(), "the day before");
        assert_eq!(history.on(day(2027, 3, 1)), orell(), "the day itself");
        assert_eq!(history.on(day(2028, 1, 3)), orell(), "long after");
    }

    #[test]
    fn a_past_weeks_target_does_not_move_when_the_days_change() {
        // The property the history exists for, stated in minutes. A week in
        // January 2027 asks for Sabine's four days whether or not she changes
        // pattern in March.
        let before = WorkScheduleHistory::new(
            vec![(day(2026, 7, 1), sabine())],
            WorkSchedule::without_fixed_days(5),
        );
        let after = WorkScheduleHistory::new(
            vec![(day(2026, 7, 1), sabine()), (day(2027, 3, 1), orell())],
            WorkSchedule::without_fixed_days(5),
        );
        let january = day(2027, 1, 4); // a Monday
        let target_before = scheduled_week_target_min(
            &before, january, day(2026, 7, 1),
            &HashSet::new(), &HashSet::new(), 23.4);
        let target_after = scheduled_week_target_min(
            &after, january, day(2026, 7, 1),
            &HashSet::new(), &HashSet::new(), 23.4);
        assert_eq!(target_before, 4 * 351);
        assert_eq!(
            target_after, target_before,
            "recording a later change must not move a week already worked"
        );
    }

    #[test]
    fn a_past_leave_day_does_not_move_when_the_days_change() {
        // Two Mondays taken off in 2027, one either side of a move from
        // Monday-to-Thursday to Tuesday-to-Friday on 1 March. The window is the
        // whole year, so it spans the change: the January Monday was a working
        // day and costs leave, the June one was not and costs nothing.
        //
        // A single pattern taken for the whole window would charge both or
        // neither, which is what the history exists to prevent.
        let january_monday = day(2027, 1, 4);
        let june_monday = day(2027, 6, 7);
        let history = WorkScheduleHistory::new(
            vec![(day(2026, 7, 1), sabine()), (day(2027, 3, 1), orell())],
            WorkSchedule::without_fixed_days(5),
        );
        let charged = scheduled_leave_days(
            &history,
            &[(january_monday, january_monday), (june_monday, june_monday)],
            day(2026, 7, 1),
            day(2027, 1, 1),
            day(2027, 12, 31),
            &HashSet::new(),
        );
        assert_eq!(charged, vec![january_monday]);
    }

    #[test]
    fn a_pattern_beginning_mid_week_splits_that_week_honestly() {
        // A three-day contract (Monday to Wednesday) becomes a five-day one on
        // Wednesday 2026-05-06. Both carry 23.4 hours a week, so a day is worth
        // 468 minutes before the change and 281 after it.
        //
        // Monday and Tuesday are worth the old contract's day, Wednesday to
        // Friday the new one. Taking one pattern for the whole week would read
        // it as three old-contract days and miss Thursday and Friday entirely.
        let monday = day(2026, 5, 4);
        let history = WorkScheduleHistory::new(
            vec![
                (day(2020, 1, 1), WorkSchedule::fixed(&[MON, TUE, WED]).unwrap()),
                (day(2026, 5, 6), full_time()),
            ],
            WorkSchedule::without_fixed_days(5),
        );
        let target = scheduled_week_target_min(
            &history, monday, hired_long_ago(),
            &HashSet::new(), &HashSet::new(), 23.4);
        assert_eq!(target, 2 * 468 + 3 * 281, "each day is worth what its own contract said");
        assert_ne!(target, 3 * 468, "not three days of the old contract");
    }

    #[test]
    fn a_leave_day_mid_week_is_judged_by_the_pattern_of_its_own_day() {
        // Same change on the Wednesday. Thursday is a working day only under
        // the new pattern, so a Thursday booked off costs leave; the Friday
        // before the change would not have.
        let monday = day(2026, 5, 4);
        let history = WorkScheduleHistory::new(
            vec![
                (day(2020, 1, 1), WorkSchedule::fixed(&[MON, TUE, WED]).unwrap()),
                (day(2026, 5, 6), full_time()),
            ],
            WorkSchedule::without_fixed_days(5),
        );
        let charged = scheduled_leave_days(
            &history,
            &[(monday, monday + Duration::days(4))],
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        );
        assert_eq!(
            charged,
            vec![
                monday,
                monday + Duration::days(1),
                monday + Duration::days(2),
                monday + Duration::days(3),
                monday + Duration::days(4),
            ],
            "Monday to Wednesday under the old pattern, Thursday and Friday under the new"
        );
    }

    #[test]
    fn entries_may_arrive_in_any_order() {
        let jumbled = WorkScheduleHistory::new(
            vec![(day(2027, 3, 1), orell()), (day(2026, 7, 1), sabine())],
            WorkSchedule::without_fixed_days(5),
        );
        assert_eq!(jumbled.on(day(2027, 1, 4)), sabine());
        assert_eq!(jumbled.on(day(2027, 6, 1)), orell());
    }

    #[test]
    fn the_newest_entry_that_has_begun_wins() {
        let history = WorkScheduleHistory::new(
            vec![
                (day(2026, 1, 1), full_time()),
                (day(2026, 7, 1), sabine()),
                (day(2027, 3, 1), orell()),
            ],
            WorkSchedule::without_fixed_days(5),
        );
        assert_eq!(history.on(day(2026, 6, 30)), full_time());
        assert_eq!(history.on(day(2026, 12, 31)), sabine());
        assert_eq!(history.on(day(2030, 1, 1)), orell());
    }

    #[test]
    fn an_empty_or_inverted_request_charges_nothing() {
        let monday = day(2026, 5, 4);
        assert!(scheduled_leave_days(
            &always(sabine()), &[], hired_long_ago(), monday,
            monday + Duration::days(6), &HashSet::new()).is_empty());
        assert!(scheduled_leave_days(
            &always(sabine()), &[(monday, monday)], hired_long_ago(),
            monday + Duration::days(6), monday, &HashSet::new()).is_empty());
    }

    #[test]
    fn a_range_outside_the_window_charges_nothing() {
        assert!(scheduled_leave_days(
            &always(sabine()),
            &[(day(2026, 3, 2), day(2026, 3, 6))],
            hired_long_ago(),
            day(2026, 5, 1),
            day(2026, 5, 31),
            &HashSet::new()
        )
        .is_empty());
    }
}
