//! Determines whether a continuous run of sick-like absences has reached the
//! admin-configured threshold that requires a medical certificate ("AU" -
//! Arbeitsunfähigkeitsbescheinigung).
//!
//! Two absence requests count as the same continuous illness period when the
//! only calendar days between them are weekends or public holidays — an
//! illness does not pause over the weekend, so e.g. Thursday + Friday sick,
//! then Monday sick again (with no workday in between) is one five-day
//! period, not two separate one/two-day ones. Any other calendar day without
//! a covering absence breaks the chain.
//!
//! The verdict is always computed fresh from the current absence data and
//! the current threshold setting (never stored), so it stays correct even
//! when a later request retroactively extends an earlier period past the
//! threshold.

use crate::error::{AppError, AppResult};
use crate::services::settings;
use crate::AppState;
use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::{HashMap, HashSet};

/// One absence's identity within a chain: its id and date range.
type Range = (i64, NaiveDate, NaiveDate);

pub async fn load_threshold_days(pool: &crate::db::DatabasePool) -> AppResult<i64> {
    let raw = settings::load_setting(
        pool,
        settings::MEDICAL_CERTIFICATE_THRESHOLD_DAYS_KEY,
        &settings::DEFAULT_MEDICAL_CERTIFICATE_THRESHOLD_DAYS.to_string(),
    )
    .await?;
    Ok(raw
        .parse::<i64>()
        .unwrap_or(settings::DEFAULT_MEDICAL_CERTIFICATE_THRESHOLD_DAYS as i64)
        .max(1))
}

/// True for a calendar day that does not interrupt an illness period even
/// without a covering absence: weekends and public holidays.
fn is_bridgeable_day(date: NaiveDate, holidays: &HashSet<NaiveDate>) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun) || holidays.contains(&date)
}

/// True when every calendar day strictly between two absence ranges is
/// bridgeable (or there is no gap at all), i.e. the two ranges belong to the
/// same continuous illness period.
fn bridges(previous_end: NaiveDate, next_start: NaiveDate, holidays: &HashSet<NaiveDate>) -> bool {
    if next_start <= previous_end {
        return true; // adjacent/overlapping
    }
    let mut day = previous_end;
    while let Some(next_day) = day.succ_opt() {
        if next_day >= next_start {
            break;
        }
        if !is_bridgeable_day(next_day, holidays) {
            return false;
        }
        day = next_day;
    }
    true
}

/// Group ranges (any order) into continuous illness chains, ordered by start
/// date within each chain and across chains.
///
/// Tracks the running *maximum* end date seen in the current chain (not just
/// the most recently appended range's end date): for real, non-overlapping
/// absences those are always the same thing (the app rejects overlaps, so
/// sorting by start date also sorts end dates), but a live preview can
/// briefly contain an unvalidated, overlapping hypothetical range — e.g. one
/// range's span fully containing a later-starting one. Using the true running
/// maximum keeps the chain from being incorrectly split in that case.
fn build_chains(ranges: &[Range], holidays: &HashSet<NaiveDate>) -> Vec<Vec<Range>> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|(_, start, _)| *start);
    let mut chains: Vec<Vec<Range>> = Vec::new();
    let mut chain_max_end: Option<NaiveDate> = None;
    for range in sorted {
        let continues_last_chain =
            chain_max_end.is_some_and(|max_end| bridges(max_end, range.1, holidays));
        if continues_last_chain {
            chains.last_mut().unwrap().push(range);
            chain_max_end = chain_max_end.map(|max_end| max_end.max(range.2));
        } else {
            chains.push(vec![range]);
            chain_max_end = Some(range.2);
        }
    }
    chains
}

/// Total calendar-day span of a chain (inclusive), bridging any weekend/
/// holiday gaps between its absences.
fn chain_span_days(chain: &[Range]) -> i64 {
    let start = chain.iter().map(|(_, s, _)| *s).min().expect("non-empty chain");
    let end = chain.iter().map(|(_, _, e)| *e).max().expect("non-empty chain");
    (end - start).num_days() + 1
}

/// For one user, decide per medical-certificate-relevant absence whether the
/// continuous illness period it belongs to has reached the configured
/// threshold. Keyed by absence id; absences whose category is not flagged
/// `medical_certificate_relevant` never appear in the result.
pub async fn required_map_for_user(
    app_state: &AppState,
    user_id: i64,
) -> AppResult<HashMap<i64, bool>> {
    let ranges = app_state
        .db
        .absences
        .medical_certificate_relevant_absences_for_user(user_id)
        .await?;
    if ranges.is_empty() {
        return Ok(HashMap::new());
    }
    let threshold_days = load_threshold_days(&app_state.pool).await?;
    let earliest = ranges.iter().map(|(_, s, _)| *s).min().unwrap();
    let latest = ranges.iter().map(|(_, _, e)| *e).max().unwrap();
    let holidays = app_state.db.reports.holiday_set(earliest, latest).await?;

    let mut result = HashMap::with_capacity(ranges.len());
    for chain in build_chains(&ranges, &holidays) {
        let required = chain_span_days(&chain) >= threshold_days;
        for (id, _, _) in chain {
            result.insert(id, required);
        }
    }
    Ok(result)
}

/// Live preview of what a (not-yet-saved, or being-edited) absence request
/// would do to the requester's AU chain — used by the request dialog so the
/// employee sees the consequence before submitting.
#[derive(serde::Serialize)]
pub struct MedicalCertificatePreview {
    /// False when the category is not `medical_certificate_relevant` at all;
    /// the other fields are meaningless in that case.
    pub relevant: bool,
    /// Total consecutive calendar days of the illness period this request
    /// would belong to, including this request itself.
    pub chain_days: i64,
    pub required: bool,
    /// Currently configured threshold, so the UI can explain the verdict
    /// without hardcoding it.
    pub threshold_days: i64,
}

/// Sentinel id for the hypothetical range being previewed. Real absence ids
/// are always positive (bigserial), so this never collides.
const PREVIEW_SENTINEL_ID: i64 = 0;

/// Compute [`MedicalCertificatePreview`] for a hypothetical `[start_date,
/// end_date]` absence in `category_id`, as if it were saved alongside the
/// user's existing medical-certificate-relevant absences. `exclude_absence_id`
/// omits the absence being edited so it isn't counted twice.
pub async fn preview_for_range(
    app_state: &AppState,
    user_id: i64,
    category_id: i64,
    start_date: NaiveDate,
    end_date: NaiveDate,
    exclude_absence_id: Option<i64>,
) -> AppResult<MedicalCertificatePreview> {
    if end_date < start_date {
        return Err(AppError::BadRequest(
            "end_date must be >= start_date.".into(),
        ));
    }
    let threshold_days = load_threshold_days(&app_state.pool).await?;
    let category = app_state
        .db
        .absence_categories
        .find_by_id(category_id)
        .await?
        .ok_or_else(|| AppError::BadRequest("Unknown absence category.".into()))?;
    if !category.medical_certificate_relevant {
        return Ok(MedicalCertificatePreview {
            relevant: false,
            chain_days: 0,
            required: false,
            threshold_days,
        });
    }

    let mut ranges = app_state
        .db
        .absences
        .medical_certificate_relevant_absences_for_user(user_id)
        .await?;
    if let Some(exclude_id) = exclude_absence_id {
        ranges.retain(|(id, _, _)| *id != exclude_id);
    }
    ranges.push((PREVIEW_SENTINEL_ID, start_date, end_date));

    let earliest = ranges.iter().map(|(_, s, _)| *s).min().unwrap();
    let latest = ranges.iter().map(|(_, _, e)| *e).max().unwrap();
    let holidays = app_state.db.reports.holiday_set(earliest, latest).await?;

    let chain = build_chains(&ranges, &holidays)
        .into_iter()
        .find(|chain| chain.iter().any(|(id, _, _)| *id == PREVIEW_SENTINEL_ID))
        .expect("the hypothetical range is always placed in some chain");
    let chain_days = chain_span_days(&chain);

    Ok(MedicalCertificatePreview {
        relevant: true,
        chain_days,
        required: chain_days >= threshold_days,
        threshold_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn no_holidays() -> HashSet<NaiveDate> {
        HashSet::new()
    }

    #[test]
    fn bridges_true_for_adjacent_days() {
        // Wednesday -> Thursday, no gap at all.
        assert!(bridges(date(2026, 6, 3), date(2026, 6, 4), &no_holidays()));
    }

    #[test]
    fn bridges_over_a_weekend() {
        // Friday end -> Monday start, weekend in between.
        assert!(bridges(date(2026, 6, 5), date(2026, 6, 8), &no_holidays()));
    }

    #[test]
    fn does_not_bridge_over_a_real_workday_gap() {
        // Monday end -> Wednesday start, Tuesday (a workday) in between with no absence.
        assert!(!bridges(date(2026, 6, 1), date(2026, 6, 3), &no_holidays()));
    }

    #[test]
    fn bridges_over_a_holiday() {
        let mut holidays = HashSet::new();
        holidays.insert(date(2026, 6, 2)); // Tuesday holiday
        assert!(bridges(date(2026, 6, 1), date(2026, 6, 3), &holidays));
    }

    #[test]
    fn build_chains_merges_weekend_bridged_absences_into_one() {
        // Thu 06-04, Fri 06-05, then Mon 06-08 — same continuous period.
        let ranges = vec![
            (1, date(2026, 6, 4), date(2026, 6, 5)),
            (2, date(2026, 6, 8), date(2026, 6, 8)),
        ];
        let chains = build_chains(&ranges, &no_holidays());
        assert_eq!(chains.len(), 1);
        assert_eq!(chain_span_days(&chains[0]), 5); // Thu..Mon inclusive
    }

    #[test]
    fn build_chains_splits_on_a_real_gap() {
        let ranges = vec![
            (1, date(2026, 6, 1), date(2026, 6, 1)), // Monday
            (2, date(2026, 6, 3), date(2026, 6, 3)), // Wednesday, Tuesday gap
        ];
        let chains = build_chains(&ranges, &no_holidays());
        assert_eq!(chains.len(), 2);
    }

    /// Regression test: a range that fully contains a later-starting,
    /// earlier-ending range must not cause the chain to split on the
    /// contained range's real gap to whatever comes after it. This can only
    /// happen with an unvalidated (overlapping) hypothetical preview range —
    /// real absences never overlap — but the chain builder must still handle
    /// it without breaking the invariant that its containing range's end date
    /// is what subsequent gaps are measured against.
    #[test]
    fn build_chains_uses_true_running_max_end_not_last_elements_end() {
        let ranges = vec![
            // Sorts first (earliest start) and spans the whole period.
            (1, date(2026, 6, 1), date(2026, 6, 30)),
            // Starts after range 1 but ends well before it — sorts second.
            (2, date(2026, 6, 10), date(2026, 6, 12)),
            // Starts after range 2 with a real (non-bridgeable) workday gap
            // from range 2's end, but is still within range 1's true span.
            (3, date(2026, 6, 20), date(2026, 6, 20)),
        ];
        let chains = build_chains(&ranges, &no_holidays());
        assert_eq!(chains.len(), 1, "all three ranges belong to one chain");
        assert_eq!(chain_span_days(&chains[0]), 30);
    }

    /// The example from the feature request: three separate 1-day requests
    /// taken on consecutive workdays must be recognised as one 3-day period.
    #[test]
    fn three_consecutive_single_day_requests_form_one_chain() {
        let ranges = vec![
            (1, date(2026, 6, 1), date(2026, 6, 1)),
            (2, date(2026, 6, 2), date(2026, 6, 2)),
            (3, date(2026, 6, 3), date(2026, 6, 3)),
        ];
        let chains = build_chains(&ranges, &no_holidays());
        assert_eq!(chains.len(), 1);
        assert_eq!(chain_span_days(&chains[0]), 3);
    }
}
