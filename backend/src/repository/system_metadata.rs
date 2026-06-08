use crate::db::DatabasePool;
use crate::error::AppResult;

#[derive(Clone)]
pub struct SystemMetadataDb {
    pool: DatabasePool,
}

impl SystemMetadataDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    pub async fn max_successful_migration_version(&self) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn users_exist(&self) -> AppResult<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn record_runtime_metadata(
        &self,
        created_git_commit: &str,
        created_migration_version: &str,
        runtime_git_commit: &str,
        runtime_migration_version: &str,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        Self::insert_if_missing(&mut tx, "database_created_git_commit", created_git_commit).await?;
        Self::insert_if_missing(
            &mut tx,
            "database_created_migration_version",
            created_migration_version,
        )
        .await?;
        Self::upsert(&mut tx, "runtime_git_commit", runtime_git_commit).await?;
        Self::upsert(
            &mut tx,
            "runtime_migration_version",
            runtime_migration_version,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn insert_if_missing(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        key: &str,
        value: &str,
    ) -> AppResult<()> {
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

    async fn upsert(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        key: &str,
        value: &str,
    ) -> AppResult<()> {
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
}
