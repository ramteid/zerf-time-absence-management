use crate::error::AppResult;
use crate::repository::AppLogEntry;
use crate::AppState;
use serde::Serialize;

/// One UI page of captured warn/error log entries.
#[derive(Serialize)]
pub struct AppLogPage {
    pub entries: Vec<AppLogEntry>,
    pub total: i64,
}

/// List captured application logs, newest first.
pub async fn list_page(
    app_state: &AppState,
    limit: Option<i64>,
    offset: Option<i64>,
) -> AppResult<AppLogPage> {
    let (limit, offset) = super::clamp_page(limit, offset);
    let (entries, total) = app_state.db.app_logs.list_page(limit, offset).await?;
    Ok(AppLogPage { entries, total })
}
