pub mod absences;
pub mod audit;
pub mod categories;
pub mod holidays;
pub mod notifications;
pub mod reopen_requests;
pub mod reports;
pub mod sessions;
pub mod settings;
pub mod time_entries;
pub mod users;

pub use absences::{Absence, AbsenceDb, CalendarEntry};
pub use audit::{AuditDb, LogEntry};
pub use categories::{Category, CategoryDb};
pub use holidays::{Holiday, HolidayDb, PreparedHoliday};
pub use notifications::{
    new_broadcaster, NotificationBroadcaster, NotificationDb, NotificationSignal,
};
pub use reopen_requests::{ReopenRequest, ReopenRequestDb};
pub use reports::ReportDb;
pub use sessions::SessionDb;
pub use settings::SettingsDb;
pub use time_entries::{NewEntryData, TimeEntry, TimeEntryDb};
pub use users::{User, UserDb};

use crate::db::DatabasePool;

/// Central façade: the only type that holds `DatabasePool` references across
/// the whole application.  All SQL is executed through the sub-repositories
/// it owns; no other module imports `sqlx` directly.
#[derive(Clone)]
pub struct Db {
    pub sessions: SessionDb,
    pub users: UserDb,
    pub time_entries: TimeEntryDb,
    pub absences: AbsenceDb,
    pub reopen_requests: ReopenRequestDb,
    pub categories: CategoryDb,
    pub holidays: HolidayDb,
    pub notifications: NotificationDb,
    pub audit: AuditDb,
    pub settings: SettingsDb,
    pub reports: ReportDb,
}

impl Db {
    pub fn new(pool: DatabasePool, broadcaster: NotificationBroadcaster) -> Self {
        Db {
            sessions: SessionDb::new(pool.clone()),
            users: UserDb::new(pool.clone()),
            time_entries: TimeEntryDb::new(pool.clone()),
            absences: AbsenceDb::new(pool.clone()),
            reopen_requests: ReopenRequestDb::new(pool.clone()),
            categories: CategoryDb::new(pool.clone()),
            holidays: HolidayDb::new(pool.clone()),
            notifications: NotificationDb::new(pool.clone(), broadcaster),
            audit: AuditDb::new(pool.clone()),
            settings: SettingsDb::new(pool.clone()),
            reports: ReportDb::new(pool),
        }
    }
}
