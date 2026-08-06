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
    // tests against — not each block's own duration).
    let worked_minutes: i64 = blocks.iter().map(|(s, e)| (*e - *s).num_minutes()).sum();

    // Wall-clock span from the first entry's start to the last entry's end, minus the
    // worked time, is the total real rest time already taken between blocks today.
    // Safe to unwrap: `blocks` is non-empty because `entries` was checked non-empty above.
    let first_start = blocks.first().unwrap().0;
    let last_end = blocks.last().unwrap().1;
    let taken_minutes = ((last_end - first_start).num_minutes() - worked_minutes).max(0);

    // Highest applicable rule wins; 0 when no rule threshold is strictly exceeded by the
    // day's total worked time.
    let required_minutes = rules
        .iter()
        .filter(|(threshold, _)| worked_minutes > *threshold)
        .map(|(_, deduction)| *deduction)
        .max()
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

/// Count effective workdays in `[from, to]`, excluding public holidays,
/// without forcing fixed weekdays for 1-5 day contracts.
///
/// The range is split by ISO week; each week's effective days are capped by
/// the configured `workdays_per_week`. This preserves the weekly day quota
/// while allowing flexible distribution across the week's potential day pool.
pub fn count_workdays(
    from: NaiveDate,
    to: NaiveDate,
    holidays: &std::collections::HashSet<NaiveDate>,
    workdays_per_week: i16,
) -> f64 {
    if to < from || workdays_per_week <= 0 {
        return 0.0;
    }

    let mut effective_days_by_week: std::collections::HashMap<NaiveDate, i16> =
        std::collections::HashMap::new();
    let mut date = from;
    while date <= to {
        if is_potential_workday(date, workdays_per_week) && !holidays.contains(&date) {
            let monday = week_monday(date);
            *effective_days_by_week.entry(monday).or_insert(0) += 1;
        }
        date += Duration::days(1);
    }

    effective_days_by_week
        .into_values()
        .map(|days| i16::min(days, workdays_per_week) as f64)
        .sum()
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
}
