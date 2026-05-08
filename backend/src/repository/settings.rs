use crate::config::SmtpConfig;
use crate::db::DatabasePool;
use crate::error::AppResult;

#[derive(Clone)]
pub struct SettingsDb {
    pool: DatabasePool,
}

impl SettingsDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    pub async fn load_setting(&self, key: &str, default: &str) -> AppResult<String> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.unwrap_or_else(|| default.to_string()))
    }

    /// Upsert a setting and return the saved value.
    pub async fn save_setting(&self, key: &str, value: &str) -> AppResult<String> {
        let saved: String = sqlx::query_scalar(
            "INSERT INTO app_settings(key, value) VALUES ($1, $2) \
             ON CONFLICT (key) DO UPDATE \
             SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP \
             RETURNING value",
        )
        .bind(key)
        .bind(value)
        .fetch_one(&self.pool)
        .await?;
        Ok(saved)
    }

    /// Upsert a setting within an existing transaction.
    pub async fn save_setting_tx(
        tx: &mut sqlx::PgConnection,
        key: &str,
        value: &str,
    ) -> AppResult<String> {
        let saved: String = sqlx::query_scalar(
            "INSERT INTO app_settings(key, value) VALUES ($1, $2) \
             ON CONFLICT (key) DO UPDATE \
             SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP \
             RETURNING value",
        )
        .bind(key)
        .bind(value)
        .fetch_one(tx)
        .await?;
        Ok(saved)
    }

    /// Begin a new transaction on the underlying pool.
    pub async fn begin(&self) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        Ok(self.pool.begin().await?)
    }

    /// Load the active SMTP configuration. Returns `None` when SMTP is
    /// disabled or not fully configured.
    pub async fn load_smtp_config(&self) -> Option<SmtpConfig> {
        let enabled = self.load_setting("smtp_enabled", "false").await.ok()?;
        if enabled != "true" {
            return None;
        }
        let host = self.load_setting("smtp_host", "").await.ok()?;
        let from = self.load_setting("smtp_from", "").await.ok()?;
        if host.is_empty() || from.is_empty() {
            return None;
        }
        let port: u16 = self
            .load_setting("smtp_port", "587")
            .await
            .ok()?
            .parse()
            .unwrap_or(587);
        let username = self.load_setting("smtp_username", "").await.ok()?;
        let password = self.load_setting("smtp_password", "").await.ok()?;
        let encryption = self
            .load_setting("smtp_encryption", "starttls")
            .await
            .ok()?;
        Some(SmtpConfig {
            host,
            port,
            username: if username.is_empty() { None } else { Some(username) },
            password: if password.is_empty() { None } else { Some(password) },
            from,
            encryption,
        })
    }

    /// Load the UI language code from app_settings, defaulting to "en".
    pub async fn load_ui_language_code(&self) -> String {
        self.load_setting("ui_language", "en")
            .await
            .unwrap_or_else(|_| "en".to_string())
    }

    /// Read the previous value of a setting key (returns `None` if not set).
    pub async fn get_raw(&self, key: &str) -> AppResult<Option<String>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(value)
    }
}
