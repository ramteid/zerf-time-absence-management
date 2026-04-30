use crate::config::Config;
use anyhow::Result;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::time::Duration;

pub type DatabasePool = sqlx::PgPool;

static MIGRATOR: Migrator = sqlx::migrate!();

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
    Ok(pool)
}
