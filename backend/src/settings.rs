use crate::auth::User;
use crate::config::SmtpConfig;
use crate::error::{AppError, AppResult};
use crate::holidays;
use crate::i18n;
use crate::repository::SettingsDb;
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
pub const SUBMISSION_REMINDERS_ENABLED_KEY: &str = "submission_reminders_enabled";
const DEFAULT_UI_LANGUAGE: &str = "en";
const DEFAULT_TIME_FORMAT: &str = "24h";
const DEFAULT_COUNTRY: &str = "DE";
const DEFAULT_REGION: &str = "";
const DEFAULT_CARRYOVER_EXPIRY_DATE: &str = "03-31";
const SUBMISSION_DEADLINE_DAY_KEY: &str = "submission_deadline_day";
const ORGANIZATION_NAME_KEY: &str = "organization_name";

#[derive(Serialize)]
pub struct PublicSettings {
    pub ui_language: String,
    pub time_format: String,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    pub default_annual_leave_days: Option<i32>,
    pub carryover_expiry_date: String,
    pub submission_deadline_day: Option<u8>,
    pub organization_name: String,
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
    pub submission_reminders_enabled: bool,
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
    pub submission_deadline_day: Option<u8>,
    pub organization_name: Option<String>,
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
    pub submission_reminders_enabled: Option<bool>,
}

fn normalize_language(value: &str) -> AppResult<String> {
    i18n::normalize_language_code(value)
        .ok_or_else(|| AppError::BadRequest("Invalid language.".into()))
}

fn normalize_time_format(value: &str) -> AppResult<&'static str> {
    match value.trim() {
        "24h" => Ok("24h"),
        "12h" => Ok("12h"),
        _ => Err(AppError::BadRequest("Invalid time format.".into())),
    }
}

fn setting_value_changed(previous: Option<&str>, next: &str) -> bool {
    previous != Some(next)
}

fn holiday_location_changed(
    previous_country: Option<&str>,
    previous_region: Option<&str>,
    next_country: &str,
    next_region: &str,
) -> bool {
    setting_value_changed(previous_country, next_country)
        || setting_value_changed(previous_region, next_region)
}

pub async fn load_setting(
    pool: &crate::db::DatabasePool,
    key: &str,
    default: &str,
) -> AppResult<String> {
    let db = SettingsDb::new(pool.clone());
    db.load_setting(key, default).await
}

async fn save_setting_exec(
    tx: &mut sqlx::PgConnection,
    key: &str,
    value: &str,
) -> AppResult<String> {
    SettingsDb::save_setting_tx(tx, key, value).await
}

async fn load_all_settings(pool: &crate::db::DatabasePool) -> AppResult<PublicSettings> {
    let default_weekly_hours_str = load_setting(pool, DEFAULT_WEEKLY_HOURS_KEY, "").await?;
    let default_annual_leave_days_str =
        load_setting(pool, DEFAULT_ANNUAL_LEAVE_DAYS_KEY, "").await?;
    let submission_deadline_day_str = load_setting(pool, SUBMISSION_DEADLINE_DAY_KEY, "").await?;
    Ok(PublicSettings {
        ui_language: load_setting(pool, UI_LANGUAGE_KEY, DEFAULT_UI_LANGUAGE).await?,
        time_format: load_setting(pool, TIME_FORMAT_KEY, DEFAULT_TIME_FORMAT).await?,
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

pub async fn public_settings(State(app_state): State<AppState>) -> AppResult<Json<PublicSettings>> {
    Ok(Json(load_all_settings(&app_state.pool).await?))
}

pub async fn admin_settings(
    State(app_state): State<AppState>,
    user: User,
) -> AppResult<Json<AdminSettingsResponse>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(load_admin_settings_response(&app_state.pool).await?))
}

async fn load_admin_settings_response(
    pool: &crate::db::DatabasePool,
) -> AppResult<AdminSettingsResponse> {
    let base = load_all_settings(pool).await?;
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
    let reminders_enabled =
        load_setting(pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true").await? != "false";
    Ok(AdminSettingsResponse {
        base,
        smtp_enabled: enabled,
        smtp_host: host,
        smtp_port: port,
        smtp_username: username,
        smtp_from: from,
        smtp_encryption: encryption,
        smtp_password_set: password_set,
        submission_reminders_enabled: reminders_enabled,
    })
}

/// Build an [`SmtpConfig`] from the fields of an [`UpdateSmtpSettings`] request,
/// using the stored password when none is supplied in the body.
async fn smtp_config_from_body(
    pool: &crate::db::DatabasePool,
    body: &UpdateSmtpSettings,
) -> AppResult<SmtpConfig> {
    let host = body.smtp_host.trim().to_string();
    let from = body.smtp_from.trim().to_string();
    let encryption = body
        .smtp_encryption
        .as_deref()
        .unwrap_or("starttls")
        .trim()
        .to_lowercase();
    let port = body.smtp_port.unwrap_or(587);
    let username = body
        .smtp_username
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let password = if body.smtp_password.as_ref().is_some_and(|p| !p.is_empty()) {
        body.smtp_password.clone()
    } else {
        let stored = load_setting(pool, SMTP_PASSWORD_KEY, "").await?;
        if stored.is_empty() {
            None
        } else {
            Some(stored)
        }
    };
    Ok(SmtpConfig {
        host,
        port,
        username: if username.is_empty() {
            None
        } else {
            Some(username)
        },
        password,
        from,
        encryption,
    })
}

/// Load the active SMTP config from the database. Returns `None` when SMTP
/// is disabled or not fully configured.
pub async fn load_smtp_config(pool: &crate::db::DatabasePool) -> Option<SmtpConfig> {
    let db = SettingsDb::new(pool.clone());
    db.load_smtp_config().await
}

pub async fn update_smtp_settings(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdateSmtpSettings>,
) -> AppResult<Json<AdminSettingsResponse>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    if !matches!(
        body.smtp_encryption.as_deref().unwrap_or("starttls").trim(),
        "starttls" | "tls" | "none"
    ) {
        return Err(AppError::BadRequest(
            "smtp_encryption must be starttls, tls, or none.".into(),
        ));
    }

    if body.smtp_enabled {
        let host = body.smtp_host.trim();
        let from = body.smtp_from.trim();
        if host.is_empty() {
            return Err(AppError::BadRequest("SMTP host is required.".into()));
        }
        if from.is_empty() {
            return Err(AppError::BadRequest(
                "SMTP from address is required.".into(),
            ));
        }
        // Validate from address is parseable as a mailbox.
        use lettre::message::Mailbox;
        from.parse::<Mailbox>()
            .map_err(|_| AppError::BadRequest("Invalid SMTP from address.".into()))?;

        // Test connection before saving anything when enabling.
        let test_config = smtp_config_from_body(&app_state.pool, &body).await?;
        crate::email::test_connection(&test_config)
            .await
            .map_err(|e| AppError::BadRequest(format!("SMTP_CONNECTION_FAILED:{e}")))?;
    }

    let smtp_config = smtp_config_from_body(&app_state.pool, &body).await?;

    // Save all SMTP settings atomically within a transaction.
    let mut transaction = app_state.pool.begin().await?;

    save_setting_exec(&mut *transaction, SMTP_HOST_KEY, &smtp_config.host).await?;
    save_setting_exec(
        &mut *transaction,
        SMTP_PORT_KEY,
        &smtp_config.port.to_string(),
    )
    .await?;
    save_setting_exec(
        &mut *transaction,
        SMTP_USERNAME_KEY,
        smtp_config.username.as_deref().unwrap_or(""),
    )
    .await?;
    save_setting_exec(&mut *transaction, SMTP_FROM_KEY, &smtp_config.from).await?;
    save_setting_exec(
        &mut *transaction,
        SMTP_ENCRYPTION_KEY,
        &smtp_config.encryption,
    )
    .await?;

    // Overwrite password when explicitly provided.
    if let Some(ref password) = body.smtp_password {
        if !password.is_empty() {
            save_setting_exec(&mut *transaction, SMTP_PASSWORD_KEY, password).await?;
        }
    }

    save_setting_exec(
        &mut *transaction,
        SMTP_ENABLED_KEY,
        if body.smtp_enabled { "true" } else { "false" },
    )
    .await?;

    let reminders_enabled = body.submission_reminders_enabled.unwrap_or(true);
    save_setting_exec(
        &mut *transaction,
        SUBMISSION_REMINDERS_ENABLED_KEY,
        if reminders_enabled { "true" } else { "false" },
    )
    .await?;

    transaction.commit().await?;

    Ok(Json(load_admin_settings_response(&app_state.pool).await?))
}

/// Test SMTP connection without saving. Builds a temporary SmtpConfig from
/// the request body and attempts to connect.
pub async fn test_smtp_connection(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdateSmtpSettings>,
) -> AppResult<Json<serde_json::Value>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let host = body.smtp_host.trim();
    let from = body.smtp_from.trim();
    if host.is_empty() {
        return Err(AppError::BadRequest("SMTP host is required.".into()));
    }
    if from.is_empty() {
        return Err(AppError::BadRequest(
            "SMTP from address is required.".into(),
        ));
    }

    let test_config = smtp_config_from_body(&app_state.pool, &body).await?;
    crate::email::test_connection(&test_config)
        .await
        .map_err(|e| AppError::BadRequest(format!("SMTP_CONNECTION_FAILED:{e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn update_admin_settings(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdateSettings>,
) -> AppResult<Json<AdminSettingsResponse>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let language = normalize_language(&body.ui_language)?;
    let time_format = normalize_time_format(&body.time_format)?;
    let country = body.country.trim().to_uppercase();
    let region = body.region.trim().to_string();
    let previous_country = app_state.db.settings.get_raw(COUNTRY_KEY).await?;
    let previous_region = app_state.db.settings.get_raw(REGION_KEY).await?;

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

    // Validate carryover expiry date (MM-DD format).
    let validated_carryover_date: Option<String> =
        if let Some(ref carryover_date) = body.carryover_expiry_date {
            let carryover_date = carryover_date.trim();
            let parts: Vec<&str> = carryover_date.split('-').collect();
            if parts.len() != 2 {
                return Err(AppError::BadRequest(
                    "carryover_expiry_date must be MM-DD.".into(),
                ));
            }
            let month: u32 = parts[0].parse().map_err(|_| {
                AppError::BadRequest("Invalid month in carryover_expiry_date.".into())
            })?;
            let day: u32 = parts[1].parse().map_err(|_| {
                AppError::BadRequest("Invalid day in carryover_expiry_date.".into())
            })?;
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return Err(AppError::BadRequest(
                    "Invalid carryover_expiry_date.".into(),
                ));
            }
            // Validate that the date actually exists (use a non-leap year to be strict).
            if chrono::NaiveDate::from_ymd_opt(2025, month, day).is_none() {
                return Err(AppError::BadRequest(
                    "Invalid carryover_expiry_date.".into(),
                ));
            }
            Some(carryover_date.to_string())
        } else {
            None
        };

    if let Some(day) = body.submission_deadline_day {
        if !(1..=28).contains(&day) {
            return Err(AppError::BadRequest(
                "submission_deadline_day must be between 1 and 28.".into(),
            ));
        }
    }

    let org_name = body
        .organization_name
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if org_name.chars().count() > 200 {
        return Err(AppError::BadRequest(
            "Organization name must be at most 200 characters.".into(),
        ));
    }

    let default_weekly_hours_str = body
        .default_weekly_hours
        .map(|v| v.to_string())
        .unwrap_or_default();
    let default_annual_leave_days_str = body
        .default_annual_leave_days
        .map(|v| v.to_string())
        .unwrap_or_default();

    let prepared_holidays = if holiday_location_changed(
        previous_country.as_deref(),
        previous_region.as_deref(),
        &country,
        &region,
    ) {
        Some(holidays::prepare_holiday_refresh(&country, &region).await?)
    } else {
        None
    };

    // Save all settings atomically within a transaction.
    let mut transaction = app_state.pool.begin().await?;

    if let Some(ref carryover_date) = validated_carryover_date {
        save_setting_exec(
            &mut *transaction,
            CARRYOVER_EXPIRY_DATE_KEY,
            carryover_date,
        )
        .await?;
    }

    if let Some(day) = body.submission_deadline_day {
        save_setting_exec(
            &mut *transaction,
            SUBMISSION_DEADLINE_DAY_KEY,
            &day.to_string(),
        )
        .await?;
    } else {
        save_setting_exec(&mut *transaction, SUBMISSION_DEADLINE_DAY_KEY, "").await?;
    }

    save_setting_exec(&mut *transaction, UI_LANGUAGE_KEY, &language).await?;
    save_setting_exec(&mut *transaction, TIME_FORMAT_KEY, time_format).await?;
    save_setting_exec(&mut *transaction, COUNTRY_KEY, &country).await?;
    save_setting_exec(&mut *transaction, REGION_KEY, &region).await?;
    save_setting_exec(
        &mut *transaction,
        DEFAULT_WEEKLY_HOURS_KEY,
        &default_weekly_hours_str,
    )
    .await?;
    save_setting_exec(
        &mut *transaction,
        DEFAULT_ANNUAL_LEAVE_DAYS_KEY,
        &default_annual_leave_days_str,
    )
    .await?;

    save_setting_exec(&mut *transaction, ORGANIZATION_NAME_KEY, &org_name).await?;

    if let Some(ref holidays) = prepared_holidays {
        crate::holidays::replace_auto_holidays_exec(&mut transaction, holidays).await?;
    }

    transaction.commit().await?;

    Ok(Json(load_admin_settings_response(&app_state.pool).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
