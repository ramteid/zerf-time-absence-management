pub mod absence_balance;
pub mod absence_categories;
pub mod absences;
pub mod app_logs;
pub mod audit_log;
pub mod auth;
pub mod categories;
pub mod holidays;
pub mod nextcloud;
pub mod notifications;
pub mod payroll_report;
pub mod reopen_requests;
pub mod reports;
pub mod settings;
pub mod time_entries;
pub mod users;

/// Default page size used by paginated log/audit listings.
const DEFAULT_PAGE_SIZE: i64 = 100;
/// Hard ceiling for a single page, regardless of what the client requests.
const MAX_PAGE_SIZE: i64 = 500;

/// Clamp client-supplied pagination parameters to sane bounds.
pub(crate) fn clamp_page(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE),
        offset.unwrap_or(0).max(0),
    )
}
