use crate::db::DatabasePool;
use crate::error::AppResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, Postgres, QueryBuilder};

#[derive(FromRow, Serialize)]
pub struct LogEntry {
    pub id: i64,
    // Nullable: set to NULL by the database (ON DELETE SET NULL) when the acting user is deleted.
    pub user_id: Option<i64>,
    pub action: String,
    pub table_name: String,
    pub record_id: i64,
    pub before_data: Option<serde_json::Value>,
    pub after_data: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AuditDb {
    pool: DatabasePool,
}

impl AuditDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Insert an audit log row. Failures are logged but not propagated.
    pub async fn log(
        &self,
        user_id: i64,
        action: &str,
        table_name: &str,
        record_id: i64,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) {
        let _ = sqlx::query(
            "INSERT INTO audit_log(user_id, action, table_name, record_id, before_data, after_data) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(user_id)
        .bind(action)
        .bind(table_name)
        .bind(record_id)
        .bind(before)
        .bind(after)
        .execute(&self.pool)
        .await;
    }

    /// Delete audit log entries older than 10 years (background cleanup).
    pub async fn cleanup_old(&self) {
        let _ = sqlx::query(
            "DELETE FROM audit_log WHERE occurred_at < CURRENT_TIMESTAMP - INTERVAL '10 years'",
        )
        .execute(&self.pool)
        .await;
    }

    /// One page of audit log entries (newest first) matching the optional
    /// filters, plus the total number of matching rows for pagination.
    pub async fn list_page(
        &self,
        table_name: Option<String>,
        record_id: Option<i64>,
        user_id: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> AppResult<(Vec<LogEntry>, i64)> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT id, user_id, action, table_name, record_id, \
             before_data, after_data, occurred_at FROM audit_log WHERE TRUE",
        );
        push_audit_filters(&mut builder, &table_name, record_id, user_id);
        builder
            .push(" ORDER BY occurred_at DESC, id DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let entries = builder
            .build_query_as::<LogEntry>()
            .fetch_all(&self.pool)
            .await?;

        let mut count_builder =
            QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM audit_log WHERE TRUE");
        push_audit_filters(&mut count_builder, &table_name, record_id, user_id);
        let total: i64 = count_builder
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;

        Ok((entries, total))
    }
}

/// Append the shared WHERE conditions so the page query and its COUNT query
/// can never drift apart.
fn push_audit_filters(
    builder: &mut QueryBuilder<Postgres>,
    table_name: &Option<String>,
    record_id: Option<i64>,
    user_id: Option<i64>,
) {
    if let Some(table_name) = table_name {
        builder
            .push(" AND table_name = ")
            .push_bind(table_name.clone());
    }
    if let Some(record_id) = record_id {
        builder.push(" AND record_id = ").push_bind(record_id);
    }
    if let Some(user_id) = user_id {
        builder.push(" AND user_id = ").push_bind(user_id);
    }
}
