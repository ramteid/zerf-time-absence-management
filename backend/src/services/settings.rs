//! Settings business logic: loading/saving app configuration, timezone helpers,
//! SMTP config, date utilities used throughout the application.

use crate::config::SmtpConfig;
use crate::error::AppResult;
use crate::repository::SettingsDb;

pub const TIMEZONE_KEY: &str = "timezone";
pub const SUBMISSION_REMINDERS_ENABLED_KEY: &str = "submission_reminders_enabled";
pub const APPROVAL_REMINDERS_ENABLED_KEY: &str = "approval_reminders_enabled";
pub const DEFAULT_TIMEZONE: &str = "Europe/Berlin";

pub const UI_LANGUAGE_KEY: &str = "ui_language";
pub const TIME_FORMAT_KEY: &str = "time_format";
pub const COUNTRY_KEY: &str = "country";
pub const REGION_KEY: &str = "region";
pub const DEFAULT_WEEKLY_HOURS_KEY: &str = "default_weekly_hours";
pub const DEFAULT_ANNUAL_LEAVE_DAYS_KEY: &str = "default_annual_leave_days";
pub const CARRYOVER_EXPIRY_DATE_KEY: &str = "carryover_expiry_date";
pub const SMTP_ENABLED_KEY: &str = "smtp_enabled";
pub const SMTP_HOST_KEY: &str = "smtp_host";
pub const SMTP_PORT_KEY: &str = "smtp_port";
pub const SMTP_USERNAME_KEY: &str = "smtp_username";
pub const SMTP_PASSWORD_KEY: &str = "smtp_password";
pub const SMTP_FROM_KEY: &str = "smtp_from";
pub const SMTP_ENCRYPTION_KEY: &str = "smtp_encryption";
pub const DEFAULT_UI_LANGUAGE: &str = "en";
const DEFAULT_TIME_FORMAT: &str = "24h";
const DEFAULT_COUNTRY: &str = "DE";
const DEFAULT_REGION: &str = "";
const DEFAULT_CARRYOVER_EXPIRY_DATE: &str = "03-31";
pub const SUBMISSION_DEADLINE_DAY_KEY: &str = "submission_deadline_day";
pub const ORGANIZATION_NAME_KEY: &str = "organization_name";

pub async fn load_setting(
    pool: &crate::db::DatabasePool,
    key: &str,
    default: &str,
) -> AppResult<String> {
    let db = SettingsDb::new(pool.clone());
    db.load_setting(key, default).await
}

pub async fn load_app_timezone(pool: &crate::db::DatabasePool) -> chrono_tz::Tz {
    let raw = load_setting(pool, TIMEZONE_KEY, DEFAULT_TIMEZONE)
        .await
        .unwrap_or_else(|_| DEFAULT_TIMEZONE.to_string());
    raw.parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::Europe::Berlin)
}

/// Returns the pinned test date from `TEST_REFERENCE_DATE` if set.
/// In production the env var is absent and this returns `None`.
pub fn pinned_test_date() -> Option<chrono::NaiveDate> {
    std::env::var("TEST_REFERENCE_DATE")
        .ok()
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

pub async fn app_today(pool: &crate::db::DatabasePool) -> chrono::NaiveDate {
    if let Some(d) = pinned_test_date() {
        return d;
    }
    chrono::Utc::now()
        .with_timezone(&load_app_timezone(pool).await)
        .date_naive()
}

pub async fn app_current_year(pool: &crate::db::DatabasePool) -> i32 {
    use chrono::Datelike;
    if let Some(d) = pinned_test_date() {
        return d.year();
    }
    chrono::Utc::now()
        .with_timezone(&load_app_timezone(pool).await)
        .year()
}

pub async fn save_setting_tx(
    tx: &mut crate::db::PgConnection,
    key: &str,
    value: &str,
) -> AppResult<String> {
    SettingsDb::save_setting_tx(tx, key, value).await
}

/// Load settings that are shown in the public (unauthenticated) settings response.
pub async fn load_all_public_settings(
    pool: &crate::db::DatabasePool,
) -> AppResult<PublicSettingsData> {
    let default_weekly_hours_str = load_setting(pool, DEFAULT_WEEKLY_HOURS_KEY, "").await?;
    let default_annual_leave_days_str =
        load_setting(pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, "").await?;
    let submission_deadline_day_str = load_setting(pool, SUBMISSION_DEADLINE_DAY_KEY, "").await?;
    Ok(PublicSettingsData {
        ui_language: load_setting(pool, UI_LANGUAGE_KEY, DEFAULT_UI_LANGUAGE).await?,
        time_format: load_setting(pool, TIME_FORMAT_KEY, DEFAULT_TIME_FORMAT).await?,
        timezone: load_setting(pool, TIMEZONE_KEY, DEFAULT_TIMEZONE).await?,
        country: load_setting(pool, COUNTRY_KEY, DEFAULT_COUNTRY).await?,
        region: load_setting(pool, REGION_KEY, DEFAULT_REGION).await?,
        default_weekly_hours: default_weekly_hours_str.parse().ok(),
        default_annual_leave_days: default_annual_leave_days_str.parse().ok(),
        carryover_expiry_date: load_setting(
            pool,
            CARRYOVER_EXPIRY_DATE_KEY,
            DEFAULT_CARRYOVER_EXPIRY_DATE,
        )
        .await?,
        submission_deadline_day: submission_deadline_day_str.parse().ok(),
        organization_name: load_setting(pool, ORGANIZATION_NAME_KEY, "").await?,
    })
}

/// Load the full admin settings response (public settings + SMTP + reminders).
pub async fn load_admin_settings(pool: &crate::db::DatabasePool) -> AppResult<AdminSettingsData> {
    let base = load_all_public_settings(pool).await?;
    let enabled = load_setting(pool, SMTP_ENABLED_KEY, "false").await? == "true";
    let host = load_setting(pool, SMTP_HOST_KEY, "").await?;
    let port: u16 = load_setting(pool, SMTP_PORT_KEY, "587")
        .await?
        .parse()
        .unwrap_or(587);
    let username = load_setting(pool, SMTP_USERNAME_KEY, "").await?;
    let from = load_setting(pool, SMTP_FROM_KEY, "").await?;
    let encryption = load_setting(pool, SMTP_ENCRYPTION_KEY, "starttls").await?;
    let password_set = !load_setting(pool, SMTP_PASSWORD_KEY, "").await?.is_empty();
    let submission_reminders_enabled =
        load_setting(pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true").await? != "false";
    let approval_reminders_enabled =
        load_setting(pool, APPROVAL_REMINDERS_ENABLED_KEY, "true").await? != "false";
    Ok(AdminSettingsData {
        base,
        smtp_enabled: enabled,
        smtp_host: host,
        smtp_port: port,
        smtp_username: username,
        smtp_from: from,
        smtp_encryption: encryption,
        smtp_password_set: password_set,
        submission_reminders_enabled,
        approval_reminders_enabled,
    })
}

/// Build an [`SmtpConfig`] from request fields, using the stored password
/// when none is supplied in the body.
pub async fn smtp_config_from_update(
    pool: &crate::db::DatabasePool,
    host: &str,
    port: u16,
    username: &str,
    password: Option<&str>,
    from: &str,
    encryption: &str,
) -> AppResult<SmtpConfig> {
    let resolved_password = match password {
        Some("") => None,
        Some(pw) => Some(pw.to_string()),
        None => {
            let stored = load_setting(pool, SMTP_PASSWORD_KEY, "").await?;
            if stored.is_empty() {
                None
            } else {
                Some(stored)
            }
        }
    };
    Ok(SmtpConfig {
        host: host.to_string(),
        port,
        username: if username.is_empty() {
            None
        } else {
            Some(username.to_string())
        },
        password: resolved_password,
        from: from.to_string(),
        encryption: encryption.to_string(),
    })
}

/// Load the active SMTP config from the database. Returns `None` when SMTP
/// is disabled or not fully configured.
pub async fn load_smtp_config(pool: &crate::db::DatabasePool) -> Option<SmtpConfig> {
    let db = SettingsDb::new(pool.clone());
    db.load_smtp_config().await
}

pub fn setting_value_changed(previous: Option<&str>, next: &str) -> bool {
    previous != Some(next)
}

pub fn holiday_location_changed(
    previous_country: Option<&str>,
    previous_region: Option<&str>,
    next_country: &str,
    next_region: &str,
) -> bool {
    setting_value_changed(previous_country, next_country)
        || setting_value_changed(previous_region, next_region)
}

pub fn normalize_language(value: &str) -> AppResult<String> {
    crate::i18n::normalize_language_code(value)
        .ok_or_else(|| crate::error::AppError::BadRequest("Invalid language.".into()))
}

pub fn normalize_time_format(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "24h" => Ok("24h"),
        "12h" => Ok("12h"),
        _ => Err(crate::error::AppError::BadRequest(
            "Invalid time format.".into(),
        )),
    }
}

pub fn normalize_timezone(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(crate::error::AppError::BadRequest(
            "Timezone is required.".into(),
        ));
    }
    let parsed = trimmed.parse::<chrono_tz::Tz>().map_err(|_| {
        crate::error::AppError::BadRequest(
            "Invalid timezone. Use an IANA timezone like Europe/Berlin.".into(),
        )
    })?;
    Ok(parsed.to_string())
}

/// Data returned by the public (unauthenticated) settings endpoint.
/// Also embedded in the admin settings response.
#[derive(serde::Serialize)]
pub struct PublicSettingsData {
    pub ui_language: String,
    pub time_format: String,
    pub timezone: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
    pub carryover_expiry_date: String,
    pub submission_deadline_day: Option<u8>,
    pub organization_name: String,
}

/// Full admin settings (public settings + SMTP config + reminder flags).
#[derive(serde::Serialize)]
pub struct AdminSettingsData {
    #[serde(flatten)]
    pub base: PublicSettingsData,
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_from: String,
    pub smtp_encryption: String,
    /// True when a password is stored (never returned in cleartext).
    pub smtp_password_set: bool,
    pub submission_reminders_enabled: bool,
    pub approval_reminders_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_test_reference_date<F: FnOnce()>(value: Option<&str>, test: F) {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let previous = std::env::var("TEST_REFERENCE_DATE").ok();
        match value {
            Some(v) => std::env::set_var("TEST_REFERENCE_DATE", v),
            None => std::env::remove_var("TEST_REFERENCE_DATE"),
        }
        test();
        match previous {
            Some(v) => std::env::set_var("TEST_REFERENCE_DATE", v),
            None => std::env::remove_var("TEST_REFERENCE_DATE"),
        }
    }

    #[test]
    fn holiday_location_changed_treats_missing_rows_as_changes() {
        assert!(holiday_location_changed(None, None, "DE", ""));
        assert!(holiday_location_changed(Some("DE"), None, "DE", ""));
        assert!(holiday_location_changed(None, Some("DE-BW"), "DE", "DE-BW"));
    }

    #[test]
    fn holiday_location_changed_ignores_unchanged_stored_values() {
        assert!(!holiday_location_changed(
            Some("DE"),
            Some("DE-BW"),
            "DE",
            "DE-BW",
        ));
        assert!(holiday_location_changed(
            Some("DE"),
            Some("DE-BW"),
            "AT",
            "",
        ));
    }

    #[test]
    fn normalize_time_format_accepts_only_supported_values() {
        assert_eq!(normalize_time_format("24h").unwrap(), "24h");
        assert_eq!(normalize_time_format("12h").unwrap(), "12h");
        assert!(normalize_time_format(" 13h ").is_err());
    }

    #[test]
    fn normalize_timezone_validates_iana_identifiers() {
        assert_eq!(
            normalize_timezone("Europe/Berlin").unwrap(),
            "Europe/Berlin"
        );
        assert!(normalize_timezone(" ").is_err());
        assert!(normalize_timezone("Mars/Olympus").is_err());
    }

    #[test]
    fn normalize_language_accepts_supported_and_locale_forms() {
        assert_eq!(normalize_language("de").unwrap(), "de");
        assert_eq!(normalize_language("en-US").unwrap(), "en-us");
        assert_eq!(normalize_language("zz").unwrap(), "zz");
        assert!(normalize_language(" ").is_err());
    }

    #[test]
    fn setting_value_changed_detects_exact_changes() {
        assert!(!setting_value_changed(Some("value"), "value"));
        assert!(setting_value_changed(Some("value"), "other"));
        assert!(setting_value_changed(None, "value"));
    }

    #[test]
    fn pinned_test_date_parses_valid_iso_date_only() {
        with_test_reference_date(Some("2026-05-19"), || {
            assert_eq!(
                pinned_test_date().unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 5, 19).unwrap()
            );
        });
        with_test_reference_date(Some("19-05-2026"), || {
            assert!(pinned_test_date().is_none());
        });
        with_test_reference_date(None, || {
            assert!(pinned_test_date().is_none());
        });
    }

    #[test]
    fn pinned_reference_date_drives_today_and_year_helpers() {
        use chrono::Datelike;
        with_test_reference_date(Some("2024-02-29"), || {
            assert_eq!(pinned_test_date().unwrap().year(), 2024);
            assert_eq!(pinned_test_date().unwrap().day(), 29);
        });
    }

    /// When an explicit non-empty password is provided it must be used as-is,
    /// without touching the database.
    #[tokio::test]
    async fn smtp_config_from_update_uses_provided_password() {
        // Build a fake pool — it will not be queried because password=Some("pw")
        // short-circuits the DB lookup.
        let pool = sqlx::Pool::connect_lazy("postgres://localhost/unused").unwrap();
        let config = smtp_config_from_update(
            &pool,
            "smtp.example.com",
            587,
            "user@example.com",
            Some("secretpw"),
            "noreply@example.com",
            "starttls",
        )
        .await
        .unwrap();
        assert_eq!(config.host, "smtp.example.com");
        assert_eq!(config.port, 587);
        assert_eq!(config.username.as_deref(), Some("user@example.com"));
        assert_eq!(config.password.as_deref(), Some("secretpw"));
        assert_eq!(config.from, "noreply@example.com");
        assert_eq!(config.encryption, "starttls");
    }

    /// An empty string password clears the stored password (no DB lookup).
    #[tokio::test]
    async fn smtp_config_from_update_clears_password_when_empty_string_provided() {
        let pool = sqlx::Pool::connect_lazy("postgres://localhost/unused").unwrap();
        let config = smtp_config_from_update(&pool, "host", 25, "", Some(""), "from@x.com", "none")
            .await
            .unwrap();
        assert!(config.password.is_none());
        assert!(config.username.is_none()); // empty username becomes None
    }
}
