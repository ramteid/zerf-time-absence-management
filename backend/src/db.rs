use crate::config::Config;
use crate::repository::SystemMetadataDb;
use anyhow::Result;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::time::Duration;

pub type DatabasePool = sqlx::PgPool;
pub type PgConnection = sqlx::PgConnection;
pub type PgTransaction<'a> = sqlx::Transaction<'a, sqlx::Postgres>;
pub type SqlxError = sqlx::Error;

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
    let metadata_db = SystemMetadataDb::new(pool.clone());
    let migration_version = metadata_db.max_successful_migration_version().await?;
    let migration_version = migration_version.to_string();
    let git_commit = normalize_git_commit(git_commit);
    let users_exist = metadata_db.users_exist().await?;
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

    metadata_db
        .record_runtime_metadata(
            created_git_commit,
            created_migration_version,
            git_commit,
            &migration_version,
        )
        .await?;

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
