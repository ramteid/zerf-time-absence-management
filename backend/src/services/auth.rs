//! Auth business logic: password hashing, token generation, approval helpers,
//! session cleanup loop.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::{ABSOLUTE_TIMEOUT_HOURS, IDLE_TIMEOUT_HOURS, User};
use crate::repository::{SessionDb, UserDb};
use crate::AppState;
use argon2::password_hash::{
    rand_core::{OsRng, RngCore},
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::{Algorithm, Argon2, Params, Version};

const MIN_PW_LEN: usize = 12;

pub fn argon2_instance() -> Argon2<'static> {
    // OWASP-recommended Argon2id parameters (memory=19 MiB, t=2, p=1).
    let params = Params::new(19456, 2, 1, None).expect("argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2_instance()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    if let Ok(parsed) = PasswordHash::new(hash) {
        argon2_instance()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    } else {
        false
    }
}

/// Async wrapper: offloads Argon2 hashing to a blocking thread so the Tokio
/// runtime is not starved during CPU-intensive work (especially important when
/// many integration tests run in parallel, each making concurrent requests).
pub async fn hash_password_async(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| AppError::Internal("password hash task panicked".into()))?
}

/// Async wrapper: offloads Argon2 verification to a blocking thread.
pub async fn verify_password_async(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .unwrap_or(false)
}

/// Reject obviously weak passwords (length, character classes).
/// Spec asks for "stark gehasht" + admin-controlled passwords; we still
/// enforce a sensible minimum policy to protect users when they self-service.
pub fn validate_password_strength(pw: &str) -> AppResult<()> {
    if pw.len() < MIN_PW_LEN {
        return Err(AppError::BadRequest(format!(
            "Password must be at least {MIN_PW_LEN} characters."
        )));
    }
    if pw.len() > 256 {
        return Err(AppError::BadRequest(
            "Password is too long (max 256 chars).".into(),
        ));
    }
    let classes = [
        pw.chars().any(|c| c.is_ascii_lowercase()),
        pw.chars().any(|c| c.is_ascii_uppercase()),
        pw.chars().any(|c| c.is_ascii_digit()),
        pw.chars().any(|c| !c.is_ascii_alphanumeric()),
    ]
    .iter()
    .filter(|&&present| present)
    .count();
    if classes < 3 {
        return Err(AppError::BadRequest(
            "Password must include at least 3 of: lowercase, uppercase, digit, symbol.".into(),
        ));
    }
    Ok(())
}

pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Periodic cleanup of expired sessions and old login attempts.
/// Matches the timeout policy enforced in `auth_middleware`:
/// idle > IDLE_TIMEOUT_HOURS OR absolute age > ABSOLUTE_TIMEOUT_HOURS.
pub async fn cleanup_loop(pool: crate::db::DatabasePool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
    let sessions = crate::repository::SessionDb::new(pool);
    loop {
        interval.tick().await;
        sessions
            .cleanup_expired_sessions(IDLE_TIMEOUT_HOURS, ABSOLUTE_TIMEOUT_HOURS)
            .await;
        sessions.cleanup_login_attempts().await;
        sessions.cleanup_reset_tokens().await;
    }
}

/// Fetch all explicitly assigned approvers for a user from user_approvers.
///
/// Notification recipients must be explicit assignments. Global admin fallback
/// is intentionally not used for notifications.
pub async fn user_approver_ids(pool: &crate::db::DatabasePool, user_id: i64) -> Vec<i64> {
    let db = UserDb::new(pool.clone());
    db.get_approver_ids(user_id).await.unwrap_or_default()
}

pub async fn lock_user_graph(tx: &mut crate::db::PgConnection) -> AppResult<()> {
    UserDb::lock_user_graph_tx(tx).await
}

/// Fetch all active notification recipients for approval workflows.
/// Recipients are always the user's explicitly assigned approvers.
pub async fn approval_recipient_ids(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    user_approver_ids(pool, requester.id).await
}

/// Fetch approval notification recipients and enforce that non-admin users
/// always have at least one effective approver.
pub async fn required_approval_recipient_ids(
    pool: &crate::db::DatabasePool,
    requester: &User,
) -> AppResult<Vec<i64>> {
    let mut recipient_ids = approval_recipient_ids(pool, requester).await;
    if !requester.is_admin() {
        // Legacy safety: non-admin users must never route approval notifications
        // to themselves, even if stale user_approvers rows exist.
        recipient_ids.retain(|recipient_id| *recipient_id != requester.id);
    }
    if !requester.is_admin() && recipient_ids.is_empty() {
        return Err(AppError::Conflict(
            "No valid approver is available for this request.".into(),
        ));
    }
    Ok(recipient_ids)
}

pub async fn change_password(
    app_state: &AppState,
    user: &User,
    raw_token: &str,
    current_password: Option<String>,
    new_password: String,
) -> AppResult<()> {
    if !user.must_change_password {
        let current_password = current_password
            .as_deref()
            .filter(|password| !password.is_empty())
            .ok_or_else(|| AppError::BadRequest("Current password required.".into()))?;
        if !verify_password_async(current_password.to_string(), user.password_hash.clone()).await {
            return Err(AppError::BadRequest(
                "Current password is incorrect.".into(),
            ));
        }
    }
    validate_password_strength(&new_password)?;
    if verify_password_async(new_password.clone(), user.password_hash.clone()).await {
        return Err(AppError::BadRequest(
            "New password must differ from the current one.".into(),
        ));
    }
    let new_password_hash = hash_password_async(new_password).await?;
    let current_token_hash = crate::middleware::auth::hash_token(raw_token);
    let mut transaction = app_state.db.users.begin().await?;
    UserDb::update_password(&mut transaction, user.id, &new_password_hash, false).await?;
    SessionDb::delete_except_tx(&mut transaction, user.id, &current_token_hash).await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn create_initial_admin(
    app_state: &AppState,
    email: String,
    password: String,
    first_name: String,
    last_name: String,
    tracks_time: bool,
) -> AppResult<()> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || email.len() > 254 || !email.contains('@') {
        return Err(AppError::BadRequest("Invalid email address.".into()));
    }
    let first_name = first_name.trim().to_string();
    let last_name = last_name.trim().to_string();
    if first_name.is_empty() || last_name.is_empty() {
        return Err(AppError::BadRequest(
            "First name and last name are required.".into(),
        ));
    }
    if first_name.len() > 200 || last_name.len() > 200 {
        return Err(AppError::BadRequest("Name too long.".into()));
    }
    validate_password_strength(&password)?;

    let password_hash = hash_password_async(password).await?;
    let today = crate::services::settings::app_today(&app_state.pool).await;
    let mut transaction = app_state.db.users.begin().await?;
    UserDb::lock_user_graph_tx(&mut transaction).await?;
    let existing_user_count = UserDb::count_tx(&mut transaction).await?;
    if existing_user_count > 0 {
        tracing::warn!(target: "zerf::auth", "POST /auth/setup called after initial setup is already complete - possible probing");
        return Err(AppError::BadRequest(
            "Setup has already been completed.".into(),
        ));
    }
    let new_user_id = UserDb::create_initial_admin(
        &mut transaction,
        &email,
        &password_hash,
        &first_name,
        &last_name,
        today,
        tracks_time,
    )
    .await?;
    let current_year = crate::services::settings::app_current_year(&app_state.pool).await;
    let default_leave_days = UserDb::get_default_leave_days_tx(&mut transaction).await?;
    UserDb::set_leave_days_tx(
        &mut transaction,
        new_user_id,
        current_year,
        default_leave_days,
    )
    .await?;
    UserDb::set_leave_days_tx(
        &mut transaction,
        new_user_id,
        current_year + 1,
        default_leave_days,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_strength_policy_enforces_length_and_character_classes() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("alllowercase123").is_err());
        assert!(validate_password_strength("NOLOWERCASE123").is_err());
        assert!(validate_password_strength("NoDigitsOnly!").is_ok());
        assert!(validate_password_strength("StrongPass123!").is_ok());

        let too_long = "A1!".repeat(90);
        assert!(validate_password_strength(&too_long).is_err());
    }

    #[test]
    fn token_helpers_generate_hex_and_hash_is_deterministic() {
        use crate::middleware::auth::hash_token;
        let t1 = new_token();
        let t2 = new_token();
        assert_eq!(t1.len(), 64);
        assert_eq!(t2.len(), 64);
        assert_ne!(t1, t2);
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));

        let h1 = hash_token("abc");
        let h2 = hash_token("abc");
        let h3 = hash_token("xyz");
        assert_eq!(h1.len(), 64);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn password_hash_and_verify_roundtrip() {
        let password = "StrongPass123!";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash));
        assert!(!verify_password("WrongPass123!", &hash));
        assert!(!verify_password(password, "not-a-valid-hash"));
    }
}
