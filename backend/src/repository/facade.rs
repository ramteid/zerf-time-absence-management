use super::{
    AbsenceCategoryDb, AbsenceDb, AuditDb, CategoryDb, HolidayDb, NotificationBroadcaster,
    NotificationDb, ReopenRequestDb, ReportDb, SessionDb, SettingsDb, TimeEntryDb,
    TimesheetExportQueueDb, UserDb,
};
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
    pub absence_categories: AbsenceCategoryDb,
    pub reopen_requests: ReopenRequestDb,
    pub categories: CategoryDb,
    pub holidays: HolidayDb,
    pub notifications: NotificationDb,
    pub audit: AuditDb,
    pub settings: SettingsDb,
    pub reports: ReportDb,
    pub export_queue: TimesheetExportQueueDb,
}

impl Db {
    pub fn new(pool: DatabasePool, broadcaster: NotificationBroadcaster) -> Self {
        Db {
            sessions: SessionDb::new(pool.clone()),
            users: UserDb::new(pool.clone()),
            time_entries: TimeEntryDb::new(pool.clone()),
            absences: AbsenceDb::new(pool.clone()),
            absence_categories: AbsenceCategoryDb::new(pool.clone()),
            reopen_requests: ReopenRequestDb::new(pool.clone()),
            categories: CategoryDb::new(pool.clone()),
            holidays: HolidayDb::new(pool.clone()),
            notifications: NotificationDb::new(pool.clone(), broadcaster),
            audit: AuditDb::new(pool.clone()),
            settings: SettingsDb::new(pool.clone()),
            reports: ReportDb::new(pool.clone()),
            export_queue: TimesheetExportQueueDb::new(pool),
        }
    }
}
