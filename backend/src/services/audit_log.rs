use crate::error::AppResult;
use crate::repository::LogEntry;
use crate::AppState;
use serde::Serialize;

/// One UI page of audit log entries.
#[derive(Serialize)]
pub struct AuditLogPage {
    pub entries: Vec<LogEntry>,
    pub total: i64,
}

/// List audit log entries, newest first, with optional filters.
pub async fn list_page(
    app_state: &AppState,
    table_name: Option<String>,
    record_id: Option<i64>,
    user_id: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> AppResult<AuditLogPage> {
    let (limit, offset) = super::clamp_page(limit, offset);
    let (entries, total) = app_state
        .db
        .audit
        .list_page(table_name, record_id, user_id, limit, offset)
        .await?;
    Ok(AuditLogPage { entries, total })
}
