use crate::error::{AppError, AppResult};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};

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
        assert!(parse_hhmm_or_hhmmss("25:00").is_none());  // out-of-range hour
        assert!(parse_hhmm_or_hhmmss("08-30").is_none());  // wrong separator
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
}
