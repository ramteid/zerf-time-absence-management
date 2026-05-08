use crate::db::DatabasePool;
use crate::error::AppResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, Postgres, QueryBuilder};

#[derive(FromRow, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub user_id: i64,
    pub action: String,
    pub table_name: String,
    pub record_id: i64,
    pub before_data: Option<String>,
    pub after_data: Option<String>,
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
        let before_json = before.map(|v| v.to_string());
        let after_json = after.map(|v| v.to_string());
        let _ = sqlx::query(
            "INSERT INTO audit_log(user_id, action, table_name, record_id, before_data, after_data) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(user_id)
        .bind(action)
        .bind(table_name)
        .bind(record_id)
        .bind(before_json)
        .bind(after_json)
        .execute(&self.pool)
        .await;
    }

    /// Query the audit log with optional filters.
    pub async fn list(
        &self,
        table_name: Option<String>,
        record_id: Option<i64>,
        user_id: Option<i64>,
    ) -> AppResult<Vec<LogEntry>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT id, user_id, action, table_name, record_id, \
             before_data, after_data, occurred_at FROM audit_log WHERE TRUE",
        );
        if table_name.is_some() {
            builder.push(" AND table_name = ").push_bind(table_name);
        }
        if record_id.is_some() {
            builder.push(" AND record_id = ").push_bind(record_id);
        }
        if user_id.is_some() {
            builder.push(" AND user_id = ").push_bind(user_id);
        }
        builder.push(" ORDER BY occurred_at DESC LIMIT 500");
        Ok(builder
            .build_query_as::<LogEntry>()
            .fetch_all(&self.pool)
            .await?)
    }
}
