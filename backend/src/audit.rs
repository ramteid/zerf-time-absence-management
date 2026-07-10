//! Audit log dispatch helper used by services after successful mutations.
//! Reading the audit log lives in `handlers::audit_log` / `services::audit_log`.

pub async fn log(
    pool: &crate::db::DatabasePool,
    user_id: i64,
    action: &str,
    table_name: &str,
    record_id: i64,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
) {
    let db = crate::repository::AuditDb::new(pool.clone());
    db.log(user_id, action, table_name, record_id, before, after)
        .await;
}
