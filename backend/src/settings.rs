use crate::auth::User;
use crate::config::SmtpConfig;
use crate::error::{AppError, AppResult};
use crate::holidays;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

const UI_LANGUAGE_KEY: &str = "ui_language";
const TIME_FORMAT_KEY: &str = "time_format";
const COUNTRY_KEY: &str = "country";
const REGION_KEY: &str = "region";
const DEFAULT_WEEKLY_HOURS_KEY: &str = "default_weekly_hours";
const DEFAULT_ANNUAL_LEAVE_DAYS_KEY: &str = "default_annual_leave_days";
const CARRYOVER_EXPIRY_DATE_KEY: &str = "carryover_expiry_date";
const SMTP_ENABLED_KEY: &str = "smtp_enabled";
const SMTP_HOST_KEY: &str = "smtp_host";
const SMTP_PORT_KEY: &str = "smtp_port";
const SMTP_USERNAME_KEY: &str = "smtp_username";
const SMTP_PASSWORD_KEY: &str = "smtp_password";
const SMTP_FROM_KEY: &str = "smtp_from";
const SMTP_ENCRYPTION_KEY: &str = "smtp_encryption";
const DEFAULT_UI_LANGUAGE: &str = "en";
const DEFAULT_TIME_FORMAT: &str = "24h";
const DEFAULT_COUNTRY: &str = "";
const DEFAULT_REGION: &str = "";
const DEFAULT_CARRYOVER_EXPIRY_DATE: &str = "03-31";

#[derive(Serialize)]
pub struct PublicSettings {
    pub ui_language: String,
    pub time_format: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
    pub carryover_expiry_date: String,
}

#[derive(Serialize)]
pub struct AdminSettingsResponse {
    #[serde(flatten)]
    pub base: PublicSettings,
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_from: String,
    pub smtp_encryption: String,
    /// True when a password is stored (never returned in cleartext).
    pub smtp_password_set: bool,
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
    pub time_format: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
    pub carryover_expiry_date: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateSmtpSettings {
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: Option<u16>,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_from: String,
    pub smtp_encryption: Option<String>,
}

fn normalize_language(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "en" => Ok("en"),
        "de" => Ok("de"),
        _ => Err(AppError::BadRequest("Invalid language.".into())),
    }
}

fn normalize_time_format(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "24h" => Ok("24h"),
        "12h" => Ok("12h"),
        _ => Err(AppError::BadRequest("Invalid time format.".into())),
    }
}

pub async fn load_setting(
    pool: &crate::db::DatabasePool,
    key: &str,
    default: &str,
) -> AppResult<String> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value.unwrap_or_else(|| default.to_string()))
}

async fn save_setting(pool: &crate::db::DatabasePool, key: &str, value: &str) -> AppResult<String> {
    let saved: String = sqlx::query_scalar(
        "INSERT INTO app_settings(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP \
         RETURNING value",
    )
    .bind(key)
    .bind(value)
    .fetch_one(pool)
    .await?;
    Ok(saved)
}

async fn load_all_settings(pool: &crate::db::DatabasePool) -> AppResult<PublicSettings> {
    let dwh = load_setting(pool, DEFAULT_WEEKLY_HOURS_KEY, "").await?;
    let dal = load_setting(pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, "").await?;
    Ok(PublicSettings {
        ui_language: load_setting(pool, UI_LANGUAGE_KEY, DEFAULT_UI_LANGUAGE).await?,
        time_format: load_setting(pool, TIME_FORMAT_KEY, DEFAULT_TIME_FORMAT).await?,
        country: load_setting(pool, COUNTRY_KEY, DEFAULT_COUNTRY).await?,
        region: load_setting(pool, REGION_KEY, DEFAULT_REGION).await?,
        default_weekly_hours: dwh.parse().ok(),
        default_annual_leave_days: dal.parse().ok(),
        carryover_expiry_date: load_setting(pool, CARRYOVER_EXPIRY_DATE_KEY, DEFAULT_CARRYOVER_EXPIRY_DATE).await?,
    })
}

pub async fn public_settings(State(s): State<AppState>) -> AppResult<Json<PublicSettings>> {
    Ok(Json(load_all_settings(&s.pool).await?))
}

pub async fn admin_settings(
    State(s): State<AppState>,
    user: User,
) -> AppResult<Json<AdminSettingsResponse>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    let base = load_all_settings(&s.pool).await?;
    let smtp = load_smtp_admin(&s.pool).await?;
    Ok(Json(AdminSettingsResponse {
        base,
        smtp_enabled: smtp.0,
        smtp_host: smtp.1,
        smtp_port: smtp.2,
        smtp_username: smtp.3,
        smtp_from: smtp.4,
        smtp_encryption: smtp.5,
        smtp_password_set: smtp.6,
    }))
}

async fn load_smtp_admin(
    pool: &crate::db::DatabasePool,
) -> AppResult<(bool, String, u16, String, String, String, bool)> {
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
    Ok((enabled, host, port, username, from, encryption, password_set))
}

/// Load the active SMTP config from the database. Returns `None` when SMTP
/// is disabled or not fully configured.
pub async fn load_smtp_config(pool: &crate::db::DatabasePool) -> Option<SmtpConfig> {
    let enabled = load_setting(pool, SMTP_ENABLED_KEY, "false").await.ok()?;
    if enabled != "true" {
        return None;
    }
    let host = load_setting(pool, SMTP_HOST_KEY, "").await.ok()?;
    let from = load_setting(pool, SMTP_FROM_KEY, "").await.ok()?;
    if host.is_empty() || from.is_empty() {
        return None;
    }
    let port: u16 = load_setting(pool, SMTP_PORT_KEY, "587")
        .await
        .ok()?
        .parse()
        .unwrap_or(587);
    let username = load_setting(pool, SMTP_USERNAME_KEY, "").await.ok()?;
    let password = load_setting(pool, SMTP_PASSWORD_KEY, "").await.ok()?;
    let encryption = load_setting(pool, SMTP_ENCRYPTION_KEY, "starttls")
        .await
        .ok()?;
    Some(SmtpConfig {
        host,
        port,
        username: if username.is_empty() {
            None
        } else {
            Some(username)
        },
        password: if password.is_empty() {
            None
        } else {
            Some(password)
        },
        from,
        encryption,
    })
}

pub async fn update_smtp_settings(
    State(s): State<AppState>,
    user: User,
    Json(body): Json<UpdateSmtpSettings>,
) -> AppResult<Json<AdminSettingsResponse>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let host = body.smtp_host.trim().to_string();
    let from = body.smtp_from.trim().to_string();
    let encryption = body
        .smtp_encryption
        .as_deref()
        .unwrap_or("starttls")
        .trim()
        .to_lowercase();
    if !matches!(encryption.as_str(), "starttls" | "tls" | "none") {
        return Err(AppError::BadRequest(
            "smtp_encryption must be starttls, tls, or none.".into(),
        ));
    }
    let port = body.smtp_port.unwrap_or(587);
    let username = body.smtp_username.as_deref().unwrap_or("").trim().to_string();

    if body.smtp_enabled {
        if host.is_empty() {
            return Err(AppError::BadRequest("SMTP host is required.".into()));
        }
        if from.is_empty() {
            return Err(AppError::BadRequest("SMTP from address is required.".into()));
        }
        // Validate from address is parseable as a mailbox.
        use lettre::message::Mailbox;
        from.parse::<Mailbox>().map_err(|_| {
            AppError::BadRequest("Invalid SMTP from address.".into())
        })?;
    }

    save_setting(&s.pool, SMTP_HOST_KEY, &host).await?;
    save_setting(&s.pool, SMTP_PORT_KEY, &port.to_string()).await?;
    save_setting(&s.pool, SMTP_USERNAME_KEY, &username).await?;
    save_setting(&s.pool, SMTP_FROM_KEY, &from).await?;
    save_setting(&s.pool, SMTP_ENCRYPTION_KEY, &encryption).await?;

    // Only overwrite password when explicitly provided (non-empty).
    if let Some(ref pw) = body.smtp_password {
        if !pw.is_empty() {
            save_setting(&s.pool, SMTP_PASSWORD_KEY, pw).await?;
        }
    }

    // Test connection before enabling.
    if body.smtp_enabled {
        let password = if body.smtp_password.as_ref().is_some_and(|p| !p.is_empty()) {
            body.smtp_password.clone()
        } else {
            let stored = load_setting(&s.pool, SMTP_PASSWORD_KEY, "").await?;
            if stored.is_empty() { None } else { Some(stored) }
        };

        let test_cfg = SmtpConfig {
            host: host.clone(),
            port,
            username: if username.is_empty() {
                None
            } else {
                Some(username.clone())
            },
            password,
            from: from.clone(),
            encryption: encryption.clone(),
        };
        crate::email::test_connection(&test_cfg).await.map_err(|e| {
            AppError::BadRequest(format!("SMTP connection test failed: {e}"))
        })?;
    }

    save_setting(
        &s.pool,
        SMTP_ENABLED_KEY,
        if body.smtp_enabled { "true" } else { "false" },
    )
    .await?;

    // Return full admin settings.
    let base = load_all_settings(&s.pool).await?;
    let smtp = load_smtp_admin(&s.pool).await?;
    Ok(Json(AdminSettingsResponse {
        base,
        smtp_enabled: smtp.0,
        smtp_host: smtp.1,
        smtp_port: smtp.2,
        smtp_username: smtp.3,
        smtp_from: smtp.4,
        smtp_encryption: smtp.5,
        smtp_password_set: smtp.6,
    }))
}

pub async fn update_admin_settings(
    State(s): State<AppState>,
    user: User,
    Json(body): Json<UpdateSettings>,
) -> AppResult<Json<AdminSettingsResponse>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let language = normalize_language(&body.ui_language)?;
    let time_format = normalize_time_format(&body.time_format)?;
    let previous = load_all_settings(&s.pool).await?;
    let country = body.country.trim().to_uppercase();
    let region = body.region.trim().to_string();

    if !country.is_empty() && country.len() != 2 {
        return Err(AppError::BadRequest(
            "Country must be a 2-letter ISO code (or empty to clear).".into(),
        ));
    }
    if region.len() > 20 {
        return Err(AppError::BadRequest(
            "Region code must be at most 20 characters.".into(),
        ));
    }
    if let Some(dwh) = body.default_weekly_hours {
        if !(0.0..=168.0).contains(&dwh) {
            return Err(AppError::BadRequest("Invalid default_weekly_hours.".into()));
        }
    }
    if let Some(dal) = body.default_annual_leave_days {
        if !(0..=366).contains(&dal) {
            return Err(AppError::BadRequest(
                "Invalid default_annual_leave_days.".into(),
            ));
        }
    }

    // Validate and save carryover expiry date (MM-DD format).
    if let Some(ref ced) = body.carryover_expiry_date {
        let ced = ced.trim();
        let parts: Vec<&str> = ced.split('-').collect();
        if parts.len() != 2 {
            return Err(AppError::BadRequest("carryover_expiry_date must be MM-DD.".into()));
        }
        let month: u32 = parts[0].parse().map_err(|_| AppError::BadRequest("Invalid month in carryover_expiry_date.".into()))?;
        let day: u32 = parts[1].parse().map_err(|_| AppError::BadRequest("Invalid day in carryover_expiry_date.".into()))?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(AppError::BadRequest("Invalid carryover_expiry_date.".into()));
        }
        // Validate that the date actually exists (use a non-leap year to be strict).
        if chrono::NaiveDate::from_ymd_opt(2025, month, day).is_none() {
            return Err(AppError::BadRequest("Invalid carryover_expiry_date.".into()));
        }
        save_setting(&s.pool, CARRYOVER_EXPIRY_DATE_KEY, ced).await?;
    }

    save_setting(&s.pool, UI_LANGUAGE_KEY, language).await?;
    save_setting(&s.pool, TIME_FORMAT_KEY, time_format).await?;
    let saved_country = save_setting(&s.pool, COUNTRY_KEY, &country).await?;
    let saved_region = save_setting(&s.pool, REGION_KEY, &region).await?;

    let dwh_str = body
        .default_weekly_hours
        .map(|v| v.to_string())
        .unwrap_or_default();
    save_setting(&s.pool, DEFAULT_WEEKLY_HOURS_KEY, &dwh_str).await?;

    let dal_str = body
        .default_annual_leave_days
        .map(|v| v.to_string())
        .unwrap_or_default();
    save_setting(&s.pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, &dal_str).await?;

    if !saved_country.is_empty()
        && (previous.country != saved_country || previous.region != saved_region)
    {
        if let Err(e) = holidays::refresh_holidays(&s.pool, &saved_country, &saved_region).await {
            tracing::warn!("Failed to refresh holidays: {:?}", e);
        }
    }

    let base = load_all_settings(&s.pool).await?;
    let smtp = load_smtp_admin(&s.pool).await?;
    Ok(Json(AdminSettingsResponse {
        base,
        smtp_enabled: smtp.0,
        smtp_host: smtp.1,
        smtp_port: smtp.2,
        smtp_username: smtp.3,
        smtp_from: smtp.4,
        smtp_encryption: smtp.5,
        smtp_password_set: smtp.6,
    }))
}
