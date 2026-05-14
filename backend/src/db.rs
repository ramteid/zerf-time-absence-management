use crate::config::Config;
use anyhow::Result;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::time::Duration;

pub type DatabasePool = sqlx::PgPool;

static MIGRATOR: Migrator = sqlx::migrate!();
const UNKNOWN_GIT_COMMIT: &str = "unknown";

pub async fn init(cfg: &Config) -> Result<DatabasePool> {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .test_before_acquire(true)
        .connect(&cfg.database_url)
        .await?;

    MIGRATOR.run(&pool).await?;
    record_system_metadata(&pool, &cfg.git_commit).await?;
    Ok(pool)
}

async fn record_system_metadata(pool: &DatabasePool, git_commit: &str) -> Result<()> {
    let migration_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success")
            .fetch_one(pool)
            .await?;
    let migration_version = migration_version.to_string();
    let git_commit = normalize_git_commit(git_commit);
    let users_exist: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(pool)
        .await?;
    let created_git_commit = if users_exist {
        UNKNOWN_GIT_COMMIT
    } else {
        git_commit
    };
    let created_migration_version = if users_exist {
        UNKNOWN_GIT_COMMIT
    } else {
        &migration_version
    };

    let mut tx = pool.begin().await?;
    insert_metadata_if_missing(&mut tx, "database_created_git_commit", created_git_commit).await?;
    insert_metadata_if_missing(
        &mut tx,
        "database_created_migration_version",
        created_migration_version,
    )
    .await?;
    upsert_metadata(&mut tx, "runtime_git_commit", git_commit).await?;
    upsert_metadata(&mut tx, "runtime_migration_version", &migration_version).await?;
    tx.commit().await?;

    tracing::info!(
        "Database metadata recorded: git_commit={}, migration_version={}",
        git_commit,
        migration_version
    );
    Ok(())
}

fn normalize_git_commit(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        UNKNOWN_GIT_COMMIT
    } else {
        trimmed
    }
}

async fn insert_metadata_if_missing(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
    value: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO system_metadata(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO NOTHING",
    )
    .bind(key)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_metadata(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
    value: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO system_metadata(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE \
         SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(key)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
