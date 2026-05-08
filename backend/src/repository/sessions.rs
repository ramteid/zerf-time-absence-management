use crate::db::DatabasePool;
use crate::error::AppResult;
use chrono::{DateTime, Utc};

/// Minimal session info returned by `get_session_info`.
pub struct SessionInfo {
    pub user_id: i64,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub csrf_token: String,
}

#[derive(Clone)]
pub struct SessionDb {
    pool: DatabasePool,
}

impl SessionDb {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    // ── Login rate-limiting ────────────────────────────────────────────────

    pub async fn count_recent_failures(
        &self,
        email: &str,
        since: DateTime<Utc>,
    ) -> AppResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_attempts \
             WHERE email = $1 AND success = FALSE AND attempted_at > $2",
        )
        .bind(email)
        .bind(since)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn record_attempt(&self, email: &str, success: bool) -> AppResult<()> {
        sqlx::query("INSERT INTO login_attempts(email, success) VALUES ($1, $2)")
            .bind(email)
            .bind(success)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Session management ─────────────────────────────────────────────────

    pub async fn create(
        &self,
        token_hash: &str,
        user_id: i64,
        csrf_token: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO sessions(token, user_id, csrf_token) VALUES ($1, $2, $3)",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(csrf_token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_user_id(&self, token_hash: &str) -> AppResult<Option<i64>> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn delete_for_user(&self, user_id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_for_user_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
    ) -> AppResult<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(tx)
            .await?;
        Ok(())
    }

    pub async fn get_csrf_token(&self, token_hash: &str) -> AppResult<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = $1")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn get_session_info(&self, token_hash: &str) -> AppResult<Option<SessionInfo>> {
        let row: Option<(i64, DateTime<Utc>, DateTime<Utc>, String)> = sqlx::query_as(
            "SELECT user_id, created_at, last_active_at, csrf_token \
             FROM sessions WHERE token = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(user_id, created_at, last_active_at, csrf_token)| SessionInfo {
            user_id,
            created_at,
            last_active_at,
            csrf_token,
        }))
    }

    pub async fn delete(&self, token_hash: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM sessions WHERE token=$1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn touch(&self, token_hash: &str) -> AppResult<()> {
        sqlx::query("UPDATE sessions SET last_active_at=CURRENT_TIMESTAMP WHERE token=$1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_except(&self, user_id: i64, keep_hash: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND token<>$2")
            .bind(user_id)
            .bind(keep_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_except_tx(
        tx: &mut sqlx::PgConnection,
        user_id: i64,
        keep_hash: &str,
    ) -> AppResult<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND token<>$2")
            .bind(user_id)
            .bind(keep_hash)
            .execute(tx)
            .await?;
        Ok(())
    }

    // ── Background cleanup ─────────────────────────────────────────────────

    pub async fn cleanup_expired_sessions(&self, idle_hours: i64, absolute_hours: i64) {
        let sql = format!(
            "DELETE FROM sessions \
             WHERE last_active_at < CURRENT_TIMESTAMP - INTERVAL '{idle_hours} hours' \
                OR created_at < CURRENT_TIMESTAMP - INTERVAL '{absolute_hours} hours'"
        );
        if let Err(e) = sqlx::query(&sql).execute(&self.pool).await {
            tracing::warn!(target: "zerf::cleanup", "session cleanup failed: {e}");
        }
    }

    pub async fn cleanup_login_attempts(&self) {
        if let Err(e) = sqlx::query(
            "DELETE FROM login_attempts \
             WHERE attempted_at < CURRENT_TIMESTAMP - INTERVAL '1 day'",
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(target: "zerf::cleanup", "login_attempts cleanup failed: {e}");
        }
    }

    pub async fn cleanup_reset_tokens(&self) {
        if let Err(e) = sqlx::query(
            "DELETE FROM password_reset_tokens WHERE expires_at <= CURRENT_TIMESTAMP",
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(target: "zerf::cleanup", "password_reset_tokens cleanup failed: {e}");
        }
    }

    // ── Password reset ─────────────────────────────────────────────────────

    pub async fn count_reset_attempts(
        &self,
        rate_limit_key: &str,
        since: DateTime<Utc>,
    ) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_attempts \
             WHERE email = $1 AND success = FALSE AND attempted_at > $2",
        )
        .bind(rate_limit_key)
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0)
    }

    pub async fn record_reset_attempt(&self, rate_limit_key: &str) {
        let _ =
            sqlx::query("INSERT INTO login_attempts(email, success) VALUES ($1, FALSE)")
                .bind(rate_limit_key)
                .execute(&self.pool)
                .await;
    }

    /// Look up an active user by email for the password-reset flow.
    pub async fn get_active_user_by_email(
        &self,
        email: &str,
    ) -> AppResult<Option<(i64, String)>> {
        Ok(
            sqlx::query_as::<_, (i64, String)>(
                "SELECT id, email FROM users WHERE lower(email)=$1 AND active=TRUE",
            )
            .bind(email)
            .fetch_optional(&self.pool)
            .await?,
        )
    }

    pub async fn upsert_reset_token(
        &self,
        token_hash: &str,
        user_id: i64,
        expires_at: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO password_reset_tokens(token_hash, user_id, expires_at) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (user_id) DO UPDATE SET \
                 token_hash = EXCLUDED.token_hash, \
                 expires_at = EXCLUDED.expires_at, \
                 created_at = CURRENT_TIMESTAMP",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically validate and consume a reset token, then update the user's
    /// password and revoke all their sessions.  Returns a descriptive error
    /// string on failure (`"reset_token_expired"` / `"reset_token_invalid"`).
    ///
    /// `new_hash` is the already-hashed new password; the caller is
    /// responsible for strength validation before calling this method.
    pub async fn consume_reset_token_and_update_password(
        &self,
        token_hash: &str,
        new_hash: &str,
    ) -> AppResult<()> {
        self.consume_reset_token_and_update_password_checked(
            token_hash,
            new_hash,
            None,
        )
        .await
    }

    /// Like `consume_reset_token_and_update_password` but also checks that the
    /// new password doesn't match the current hash. If `verify_not_reused` is
    /// provided, it is called with the current password_hash to check reuse.
    /// Returns `AppError::BadRequest` if reuse is detected.
    pub async fn consume_reset_token_and_update_password_checked(
        &self,
        token_hash: &str,
        new_hash: &str,
        verify_not_reused: Option<&(dyn Fn(&str) -> bool + Send + Sync)>,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;

        // Try to delete an expired token first (gives a meaningful error).
        let expired: Option<i64> = sqlx::query_scalar(
            "DELETE FROM password_reset_tokens \
             WHERE token_hash=$1 AND expires_at <= CURRENT_TIMESTAMP \
             RETURNING user_id",
        )
        .bind(token_hash)
        .fetch_optional(&mut *tx)
        .await?;
        if expired.is_some() {
            tx.commit().await?;
            return Err(crate::error::AppError::BadRequest(
                "reset_token_expired".into(),
            ));
        }

        let user_id: Option<i64> = sqlx::query_scalar(
            "DELETE FROM password_reset_tokens \
             WHERE token_hash=$1 AND expires_at > CURRENT_TIMESTAMP \
             RETURNING user_id",
        )
        .bind(token_hash)
        .fetch_optional(&mut *tx)
        .await?;
        let user_id = match user_id {
            Some(id) => id,
            None => {
                return Err(crate::error::AppError::BadRequest(
                    "reset_token_invalid".into(),
                ))
            }
        };

        // Lock the user row and fetch current hash for reuse check.
        let current_hash: Option<String> = sqlx::query_scalar(
            "SELECT password_hash FROM users WHERE id=$1 AND active=TRUE FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current_hash) = current_hash else {
            tx.commit().await?;
            return Err(crate::error::AppError::BadRequest(
                "reset_token_invalid".into(),
            ));
        };

        // Check password reuse if a verifier is provided.
        if let Some(check_reuse) = verify_not_reused {
            if check_reuse(&current_hash) {
                tx.rollback().await?;
                return Err(crate::error::AppError::BadRequest(
                    "New password must differ from the current one.".into(),
                ));
            }
        }

        let rows = sqlx::query(
            "UPDATE users SET password_hash=$1, must_change_password=FALSE \
             WHERE id=$2 AND active=TRUE",
        )
        .bind(new_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows != 1 {
            tx.commit().await?;
            return Err(crate::error::AppError::BadRequest(
                "reset_token_invalid".into(),
            ));
        }

        sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }
}
