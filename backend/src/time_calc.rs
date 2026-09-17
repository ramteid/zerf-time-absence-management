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
fn is_potential_workday(date: NaiveDate, workdays_per_week: i16) -> bool {
    let weekday = date.weekday().num_days_from_monday();
    match workdays_per_week {
        i16::MIN..=0 => false,
        1..=5 => weekday < 5,
        6 => weekday < 6,
        _ => true,
    }
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
    /// A day outside that range refuses the **whole** pattern rather than being
    /// dropped from it. Dropping it silently turns `[1, 6]` into a one-day
    /// contract and charges that person four days of leave a week they never
    /// owed; refusing it falls back to the recorded day count, which at least
    /// still says how many days they work. An empty list is not a schedule
    /// either — "works no days" is a different statement from "days not
    /// recorded", and the second is what no schedule at all means.
    pub fn fixed(weekdays: &[u8]) -> Option<Self> {
        let mut bits = 0u8;
        for day in weekdays {
            if !(1..=5).contains(day) {
                return None;
            }
            bits |= 1 << (day - 1);
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
    ///
    /// An irregular contract (`workdays_per_week` of zero or less) is the one
    /// case where that pool answers "no" for all seven days. It is not a
    /// contract with no working days; it is one with no *fixed* days at all,
    /// and the app charges every calendar day of an absence against it
    /// (`counted_workdays`' irregular branch, `validate_absence_has_workday`).
    /// Reading the empty pool literally made every leave day such a person took
    /// free of charge, so it is answered here instead.
    pub fn covers(&self, date: NaiveDate) -> bool {
        if !self.has_fixed_days() {
            if self.is_irregular() {
                return true;
            }
            return is_potential_workday(date, self.fallback_days_per_week);
        }
        let iso = date.weekday().number_from_monday();
        self.weekdays & (1 << (iso - 1)) != 0
    }

    /// No fixed days *and* no weekday pool: every calendar day can carry an
    /// absence, and none of them carries a target.
    fn is_irregular(&self) -> bool {
        !self.has_fixed_days() && potential_workdays_per_week(self.fallback_days_per_week) == 0
    }

    /// How many days of one ISO week may be charged to a leave account, or
    /// `None` when nothing needs capping.
    ///
    /// A recorded pattern needs no cap: a week cannot hold more of that
    /// contract's working days than the contract has. That is the whole reason
    /// the fixed-weekday rule can drop the weekly quota, the whole-week
    /// arithmetic and the special handling at month and year boundaries.
    ///
    /// Without a pattern the old day *count* is all there is, and a count is a
    /// quota rather than a set of days — so it has to keep working as a quota,
    /// exactly as `counted_workdays` applies it today. Leaving it off charged a
    /// three-day contract with no recorded pattern five days for one week away.
    ///
    /// An irregular contract has no quota either: today every calendar day of
    /// an absence is charged to it.
    pub fn weekly_leave_cap(&self) -> Option<i16> {
        if self.has_fixed_days() || self.is_irregular() {
            return None;
        }
        Some(self.fallback_days_per_week)
    }

    /// How many days a week the contract works, for the purpose of pricing one
    /// of them.
    ///
    /// An irregular contract answers zero, and so carries no target — which is
    /// what the app does today. It still [`covers`](Self::covers) every day,
    /// because a day with no target can still hold an absence. The two
    /// questions genuinely have different answers there.
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
/// adds an entry rather than replacing one.
///
/// Every calculation asks this per day, never once for a whole week or window.
/// Asking once and reusing the answer judges days under a pattern that was not
/// in force on them, which is the defect the history exists to prevent: a
/// pattern beginning on a Wednesday would otherwise decide that week's Monday
/// too. Everything else stays live, so a sick note entered for a past week
/// still takes effect at once.
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
/// One working day covered by an absence costs one leave day. For a contract
/// whose weekdays are recorded there is no weekly cap and no whole-week
/// arithmetic: a week cannot hold more working days than the contract has, so
/// there is nothing left to cap. That is also why a week split by a month or
/// year boundary is charged correctly without any special handling — each day
/// simply belongs to the period it falls in.
///
/// A contract with **no recorded pattern** keeps the old weekly quota, because
/// its day count is a quota and nothing more; see
/// [`WorkSchedule::weekly_leave_cap`]. Those people therefore get exactly the
/// number of days `counted_workdays` charges them today.
///
/// That quota carries `counted_workdays`' caller rule with it, so the caller
/// rule has **not** gone away for everybody. A quota is counted per week within
/// one call. Splitting a span into two calls — "taken up to today" plus
/// "upcoming from tomorrow", or the halves either side of a carryover expiry —
/// gives the week straddling the split a fresh quota in each call and charges
/// it twice. Ask for the whole span in one call and cut the returned days into
/// buckets afterwards. A contract whose weekdays are recorded has no quota and
/// is immune to this.
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
    // Only days judged by a schedule with no recorded pattern are counted here.
    // Those keep the old weekly quota, because a count is all their contract
    // says; a recorded pattern needs no quota at all.
    let mut capped_in_week: std::collections::HashMap<NaiveDate, i16> =
        std::collections::HashMap::new();
    let mut date = window_start.max(contract_start);
    while date <= window_end {
        // Asked per day, not once for the window. A window is usually a month
        // or a year, and the pattern may well have changed inside it; taking a
        // single pattern for the whole span would judge part of it under a
        // schedule that was not in force, which is the very thing the history
        // exists to prevent.
        let schedule = history.on(date);
        if schedule.covers(date)
            && !holidays.contains(&date)
            && ranges.iter().any(|(from, to)| date >= *from && date <= *to)
        {
            match schedule.weekly_leave_cap() {
                Some(cap) => {
                    let used = capped_in_week.entry(week_monday(date)).or_insert(0);
                    if *used < cap {
                        *used += 1;
                        charged.push(date);
                    }
                }
                None => charged.push(date),
            }
        }
        date += Duration::days(1);
    }
    charged
}

/// How many of the charged leave days each range carries.
///
/// Returns one count per input range, in input order. Days are attributed in
/// chronological order of range start, with input order breaking ties, so the
/// first booking in a week keeps its own days and a later one overlapping it is
/// charged only what is left.
///
/// This is the per-range counterpart of [`scheduled_leave_days`], for callers
/// that print or bill each range separately and still need those numbers to add
/// up to what the days actually cost. The payroll report is one: two sick notes
/// inside one calendar week must not claim more days of continued pay than that
/// week holds for the contract.
pub fn scheduled_days_per_range(
    history: &WorkScheduleHistory,
    ranges: &[(NaiveDate, NaiveDate)],
    contract_start: NaiveDate,
    window_start: NaiveDate,
    window_end: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
) -> Vec<f64> {
    let mut counts = vec![0.0; ranges.len()];
    if ranges.is_empty() {
        return counts;
    }
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by_key(|index| (ranges[*index].0, *index));
    for day in scheduled_leave_days(
        history,
        ranges,
        contract_start,
        window_start,
        window_end,
        holidays,
    ) {
        if let Some(owner) = order
            .iter()
            .find(|index| day >= ranges[**index].0 && day <= ranges[**index].1)
        {
            counts[*owner] += 1.0;
        }
    }
    counts
}

/// Minutes one single day asks for.
///
/// This is the whole rule in one place, and every target in the app is built
/// from it. A day carries `weekly_hours / <days the contract works>` when the
/// contract works that weekday, and nothing at all otherwise — so hours booked
/// on a day it does not work are overtime in full.
///
/// `excused` is the caller's own reason for the day to ask nothing: a public
/// holiday, an absence that removes the target, or, for the flextime ledger, a
/// week that is not yet approved. Those differ between callers, which is why
/// the day itself cannot decide them; everything that does not differ lives
/// here rather than being written out again at each call site.
///
/// A day before the contract began asks nothing either, the same way every
/// view in the app hides content stored before a start date.
pub fn scheduled_day_minutes(
    history: &WorkScheduleHistory,
    date: NaiveDate,
    contract_start: NaiveDate,
    excused: bool,
    weekly_hours: f64,
) -> i64 {
    if date < contract_start || excused {
        return 0;
    }
    let schedule = history.on(date);
    if schedule.covers(date) {
        schedule.day_minutes(weekly_hours)
    } else {
        0
    }
}

fn parse_hhmm_or_hhmmss(value: &str) -> Option<NaiveTime> {
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

    /// The calendar this app used before a contract could name the
    /// weekdays it works. Nothing ships it any more: it is kept here as the
    /// reference the equivalence test below measures against, so a contract
    /// with no recorded pattern can be shown to be charged exactly what it
    /// always was.
    fn counted_workdays(
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

    // ──────────────────────────────────────────────────────────────────────
    // scheduled_days_per_range — the per-row counterpart the payroll report
    // prints. Its rows have to add up to what the days actually cost.
    // ──────────────────────────────────────────────────────────────────────

    /// A day two bookings both cover is charged once, to the earlier of them.
    /// Bookings of one person cannot overlap in the app, but the function must
    /// not invent a day if one ever does.
    #[test]
    fn scheduled_days_per_range_charges_a_shared_day_to_the_earlier_row() {
        let monday = day(2026, 5, 4);
        let ranges = [
            (monday, monday + Duration::days(1)),
            (monday + Duration::days(1), monday + Duration::days(2)),
        ];
        let counted = scheduled_days_per_range(
            &always(sabine()),
            &ranges,
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        );
        assert_eq!(counted, vec![2.0, 1.0], "Monday and Tuesday, then Wednesday");
        assert_eq!(counted.iter().sum::<f64>(), 3.0, "three days, not four");
    }

    #[test]
    fn scheduled_days_per_range_is_independent_of_the_input_order() {
        let monday = day(2026, 5, 4);
        let later_first = [
            (monday + Duration::days(1), monday + Duration::days(2)),
            (monday, monday + Duration::days(1)),
        ];
        let counted = scheduled_days_per_range(
            &always(sabine()),
            &later_first,
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        );
        // The earlier range keeps its own days wherever it sits in the input.
        assert_eq!(counted, vec![1.0, 2.0]);
    }

    /// A contract whose weekdays were never recorded still carries a weekly
    /// quota, and two bookings in one week share it instead of each claiming
    /// it in full. Counted separately these two are 2 + 2; the week only ever
    /// holds three days for a three-day contract.
    #[test]
    fn scheduled_days_per_range_shares_one_weeks_quota_without_a_pattern() {
        let monday = day(2026, 5, 4);
        let ranges = [
            (monday, monday + Duration::days(1)),
            (monday + Duration::days(3), monday + Duration::days(4)),
        ];
        let counted = scheduled_days_per_range(
            &WorkScheduleHistory::without_history(WorkSchedule::without_fixed_days(3)),
            &ranges,
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        );
        assert_eq!(counted, vec![2.0, 1.0]);
        assert_eq!(counted.iter().sum::<f64>(), 3.0);
    }

    /// Whatever the rows come to individually, together they are exactly the
    /// days the bookings cost. A document that prints the rows therefore
    /// claims neither more nor less than the leave figures beside it.
    #[test]
    fn scheduled_days_per_range_rows_total_the_days_actually_charged() {
        let monday = day(2026, 5, 4);
        let window_end = monday + Duration::days(20);
        let holidays = HashSet::from([monday + Duration::days(9)]);
        for history in [
            always(sabine()),
            always(orell()),
            always(full_time()),
            WorkScheduleHistory::without_history(WorkSchedule::without_fixed_days(3)),
        ] {
            for offset in 0..10i64 {
                let ranges = [
                    (monday, monday + Duration::days(2)),
                    (
                        monday + Duration::days(offset),
                        monday + Duration::days(offset + 3),
                    ),
                ];
                let per_range = scheduled_days_per_range(
                    &history,
                    &ranges,
                    hired_long_ago(),
                    monday,
                    window_end,
                    &holidays,
                );
                let union = scheduled_leave_days(
                    &history,
                    &ranges,
                    hired_long_ago(),
                    monday,
                    window_end,
                    &holidays,
                )
                .len();
                assert_eq!(
                    per_range.iter().sum::<f64>(),
                    union as f64,
                    "offset {offset}: the rows must add up to the days charged"
                );
            }
        }
    }

    #[test]
    fn scheduled_days_per_range_handles_no_ranges() {
        let monday = day(2026, 5, 4);
        assert!(scheduled_days_per_range(
            &always(sabine()),
            &[],
            hired_long_ago(),
            monday,
            monday + Duration::days(6),
            &HashSet::new(),
        )
        .is_empty());
    }

    // ──────────────────────────────────────────────────────────────────────
    // scheduled_day_minutes — the one rule every target in the app is built
    // from. Each of its four ways of answering nothing is pinned here.
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn a_working_day_asks_for_its_share_of_the_week() {
        // Sabine works Monday to Thursday on 23.4 hours: 1404 minutes over
        // four days is 351 a day.
        let monday = day(2026, 5, 4);
        assert_eq!(
            scheduled_day_minutes(&always(sabine()), monday, hired_long_ago(), false, 23.4),
            351
        );
    }

    #[test]
    fn a_day_the_contract_does_not_work_asks_for_nothing() {
        // The Friday of the same week. Hours booked on it are overtime in full.
        let friday = day(2026, 5, 8);
        assert_eq!(
            scheduled_day_minutes(&always(sabine()), friday, hired_long_ago(), false, 23.4),
            0
        );
    }

    #[test]
    fn an_excused_day_asks_for_nothing_however_the_caller_excused_it() {
        // The caller's reason — a public holiday, an absence that removes the
        // target, a week not yet approved — is not the day's business.
        let monday = day(2026, 5, 4);
        assert_eq!(
            scheduled_day_minutes(&always(sabine()), monday, hired_long_ago(), true, 23.4),
            0
        );
    }

    #[test]
    fn a_day_before_the_contract_began_asks_for_nothing() {
        let monday = day(2026, 5, 4);
        let started_later = day(2026, 5, 5);
        assert_eq!(
            scheduled_day_minutes(&always(sabine()), monday, started_later, false, 23.4),
            0
        );
        // And the very first day of the contract does ask.
        assert_eq!(
            scheduled_day_minutes(
                &always(sabine()),
                started_later,
                started_later,
                false,
                23.4
            ),
            351
        );
    }

    /// An assistant has no fixed days and no target. Both halves matter, and
    /// they pull in opposite directions: every calendar day of theirs can
    /// carry an absence, yet none of them asks for work.
    #[test]
    fn an_assistant_carries_every_calendar_day_and_no_target() {
        let assistant = WorkScheduleHistory::without_history(WorkSchedule::without_fixed_days(7));
        let monday = day(2026, 5, 4);
        let saturday = day(2026, 5, 9);
        let sunday = day(2026, 5, 10);

        // Their weekly hours are zero, so no day asks for anything.
        for date in [monday, saturday, sunday] {
            assert_eq!(
                scheduled_day_minutes(&assistant, date, hired_long_ago(), false, 0.0),
                0,
                "{date} asks for nothing"
            );
        }

        // But a weekend absence is still charged to them, which is what the
        // app has always done for an hourly contract.
        let charged = scheduled_leave_days(
            &assistant,
            &[(saturday, sunday)],
            hired_long_ago(),
            monday,
            sunday,
            &HashSet::new(),
        );
        assert_eq!(charged, vec![saturday, sunday], "both weekend days count");
    }

    /// A leave range that runs across a change of working days is charged by
    /// each day's own pattern, not by whichever pattern the range began under.
    #[test]
    fn a_leave_range_across_a_change_is_charged_day_by_day() {
        // Monday to Thursday until the Wednesday, Tuesday to Friday from it.
        let monday = day(2026, 5, 4);
        let wednesday = day(2026, 5, 6);
        let history = WorkScheduleHistory::new(
            vec![(day(1900, 1, 1), sabine()), (wednesday, orell())],
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
        // Monday and Tuesday under the old pattern, Wednesday to Friday under
        // the new one — five days, because the change widened the week.
        assert_eq!(
            charged,
            vec![
                monday,
                monday + Duration::days(1),
                wednesday,
                monday + Duration::days(3),
                monday + Duration::days(4),
            ]
        );
    }


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
    /// Minutes a span asks for: the per-day rule summed. Every report loop
    /// does exactly this with its own extra reasons to excuse a day, so this
    /// is how the worked examples below exercise the shipped rule.
    fn span_target_min(
        history: &WorkScheduleHistory,
        from: NaiveDate,
        to: NaiveDate,
        contract_start: NaiveDate,
        holidays: &HashSet<NaiveDate>,
        absence_days: &HashSet<NaiveDate>,
        weekly_hours: f64,
    ) -> i64 {
        let mut total = 0i64;
        let mut date = from;
        while date <= to {
            let excused = holidays.contains(&date) || absence_days.contains(&date);
            total += scheduled_day_minutes(history, date, contract_start, excused, weekly_hours);
            date += Duration::days(1);
        }
        total
    }

    /// A whole ISO week of [`span_target_min`].
    fn week_target_min(
        history: &WorkScheduleHistory,
        week_monday: NaiveDate,
        contract_start: NaiveDate,
        holidays: &HashSet<NaiveDate>,
        absence_days: &HashSet<NaiveDate>,
        weekly_hours: f64,
    ) -> i64 {
        span_target_min(
            history,
            week_monday,
            week_monday + Duration::days(6),
            contract_start,
            holidays,
            absence_days,
            weekly_hours,
        )
    }

    fn target(
        schedule: &WorkSchedule,
        monday: NaiveDate,
        holiday_offsets: &[i64],
        absence_offsets: &[i64],
        weekly_hours: f64,
    ) -> i64 {
        week_target_min(
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
        // A weekend day alongside real ones refuses the whole pattern rather
        // than quietly leaving a one-day contract behind. `[MON, SAT]` cannot
        // be honoured, and reading it as "Mondays only" would charge that
        // person four days of leave a week they never owed.
        assert!(WorkSchedule::fixed(&[MON, SAT]).is_none());
    }

    #[test]
    fn a_contract_with_no_weekday_pool_still_carries_every_day_of_leave() {
        // An irregular contract (`workdays_per_week` of zero) has no target —
        // `potential_workdays_per_week` answers zero — but the app charges every
        // calendar day of an absence to it. Reading the empty pool as "no
        // working days" made all of that leave free.
        let irregular = WorkSchedule::without_fixed_days(0);
        assert_eq!(irregular.days_per_week(), 0, "no target pool");
        assert_eq!(irregular.day_minutes(40.0), 0, "and so no day is worth anything");
        for offset in 0..7i64 {
            assert!(
                irregular.covers(day(2026, 5, 4) + Duration::days(offset)),
                "every calendar day can hold an absence, offset {offset}"
            );
        }
        assert_eq!(irregular.weekly_leave_cap(), None, "and no quota caps it");
    }

    #[test]
    fn only_a_contract_without_recorded_days_is_capped() {
        assert_eq!(sabine().weekly_leave_cap(), None, "a pattern needs no quota");
        assert_eq!(full_time().weekly_leave_cap(), None);
        for count in 1..=5i16 {
            assert_eq!(
                WorkSchedule::without_fixed_days(count).weekly_leave_cap(),
                Some(count),
                "{count} recorded as a count alone stays a quota"
            );
        }
    }

    /// The promise `WorkScheduleHistory`'s fallback makes: somebody whose
    /// working days were never recorded is charged exactly what the app charges
    /// them today. Without the quota a three-day contract paid five days for
    /// one week away, and an irregular one paid nothing at all.
    #[test]
    fn a_contract_without_recorded_days_is_charged_exactly_as_today() {
        let holidays: HashSet<NaiveDate> =
            [day(2026, 5, 1), day(2026, 5, 14)].into_iter().collect();
        for count in [0i16, 1, 2, 3, 4, 5, 6, 7] {
            let history =
                WorkScheduleHistory::without_history(WorkSchedule::without_fixed_days(count));
            for start_offset in 0..40i64 {
                for length in 0..12i64 {
                    let from = day(2026, 4, 27) + Duration::days(start_offset);
                    let to = from + Duration::days(length);
                    let window_start = day(2026, 4, 27);
                    let window_end = day(2026, 7, 31);
                    let charged = scheduled_leave_days(
                        &history,
                        &[(from, to)],
                        day(2000, 1, 1),
                        window_start,
                        window_end,
                        &holidays,
                    );
                    let today = counted_workdays(
                        &[(from, to)],
                        window_start,
                        window_end,
                        &holidays,
                        count,
                    );
                    assert_eq!(charged, today, "{count} days a week, {from}..{to}");
                }
            }
        }
    }

    /// A month must not be built by summing whole weeks: the week straddling
    /// its end belongs to two months, and each would ask for it in full.
    #[test]
    fn a_months_target_counts_each_day_once() {
        // August 2026 ends on a Monday, so its last week runs into September.
        let history = WorkScheduleHistory::new(
            vec![(day(2020, 1, 1), sabine())],
            WorkSchedule::without_fixed_days(4),
        );
        let no_holidays = HashSet::new();
        let no_absence = HashSet::new();
        let year: i64 = (1..=12u32)
            .map(|month| {
                let first = NaiveDate::from_ymd_opt(2026, month, 1).unwrap();
                let last =
                    NaiveDate::from_ymd_opt(2026, month, last_day_of_month(2026, month)).unwrap();
                span_target_min(
                    &history,
                    first,
                    last,
                    day(2000, 1, 1),
                    &no_holidays,
                    &no_absence,
                    23.4,
                )
            })
            .sum();
        let whole = span_target_min(
            &history,
            day(2026, 1, 1),
            day(2026, 12, 31),
            day(2000, 1, 1),
            &no_holidays,
            &no_absence,
            23.4,
        );
        assert_eq!(year, whole, "every month together is the year, day for day");

        // And the week version is exactly seven days of the same function.
        let monday = day(2026, 8, 31);
        assert_eq!(
            week_target_min(
                &history,
                monday,
                day(2000, 1, 1),
                &no_holidays,
                &no_absence,
                23.4
            ),
            span_target_min(
                &history,
                monday,
                monday + Duration::days(6),
                day(2000, 1, 1),
                &no_holidays,
                &no_absence,
                23.4
            )
        );
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
            week_target_min(
                &always(sabine()), monday, start, &HashSet::new(), &HashSet::new(), 23.4),
            2 * 351,
            "Wednesday and Thursday are the working days she has that week"
        );
    }

    #[test]
    fn a_scheduled_week_entirely_before_the_start_date_is_empty() {
        assert_eq!(
            week_target_min(
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
        let target_before = week_target_min(
            &before, january, day(2026, 7, 1),
            &HashSet::new(), &HashSet::new(), 23.4);
        let target_after = week_target_min(
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
        let target = week_target_min(
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
