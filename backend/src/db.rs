use crate::config::Config;
use anyhow::Result;
use std::borrow::Cow;
#[cfg(not(feature = "test-sqlite"))]
use std::time::Duration;

#[cfg(not(feature = "test-sqlite"))]
pub type DatabasePool = sqlx::PgPool;
#[cfg(feature = "test-sqlite")]
pub type DatabasePool = sqlx::SqlitePool;

#[cfg(not(feature = "test-sqlite"))]
pub type DbType = sqlx::Postgres;
#[cfg(feature = "test-sqlite")]
pub type DbType = sqlx::Sqlite;

#[cfg(not(feature = "test-sqlite"))]
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// Translate Postgres-style `$N` placeholders to SQLite-style `?N`.
///
/// In production builds (Postgres) this is a no-op returning a borrowed
/// reference. In test-sqlite builds it replaces `$1`, `$2`, ... with
/// `?1`, `?2`, ... so that the same query strings work against SQLite.
#[cfg(not(feature = "test-sqlite"))]
#[inline]
pub fn sql(query: &'static str) -> Cow<'static, str> {
    Cow::Borrowed(query)
}

#[cfg(feature = "test-sqlite")]
pub fn sql(query: &'static str) -> Cow<'static, str> {
    if !query.contains('$') {
        return Cow::Borrowed(query);
    }
    let mut result = query.to_string();
    // Replace in reverse order so $10 is handled before $1.
    for i in (1..=30).rev() {
        result = result.replace(&format!("${i}"), &format!("?{i}"));
    }
    Cow::Owned(result)
}

#[cfg(not(feature = "test-sqlite"))]
pub async fn init(cfg: &Config) -> Result<DatabasePool> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .test_before_acquire(true)
        .connect(&cfg.database_url)
        .await?;

    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

#[cfg(feature = "test-sqlite")]
pub async fn init(_cfg: &Config) -> Result<DatabasePool> {
    init_sqlite().await
}

/// Create an in-memory SQLite pool and apply the SQLite-specific schema.
#[cfg(feature = "test-sqlite")]
pub async fn init_sqlite() -> Result<DatabasePool> {
    use sqlx::sqlite::SqlitePoolOptions;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;

    let schema = include_str!("../migrations/sqlite/001_initial.sql");
    // Execute each statement separately (SQLite does not support multi-statement exec
    // in a single query call via sqlx).
    for stmt in schema.split(';') {
        let trimmed = stmt.trim();
        if trimmed.is_empty() {
            continue;
        }
        sqlx::query(trimmed).execute(&pool).await?;
    }
    Ok(pool)
}
