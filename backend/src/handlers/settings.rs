//! HTTP handlers for application settings (public, admin, SMTP).

use crate::audit;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::services::settings::{
    self, load_admin_settings, load_all_public_settings, load_setting, normalize_language,
    normalize_time_format, normalize_timezone, save_setting_tx, setting_value_changed,
    smtp_config_from_update, AdminSettingsData, PublicSettingsData,
    APPROVAL_REMINDERS_ENABLED_KEY, SUBMISSION_REMINDERS_ENABLED_KEY, TIMEZONE_KEY,
};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use lettre::message::Mailbox;
use serde::Deserialize;

// All setting key constants are used via `settings::` module — no re-imports needed.

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
    pub time_format: String,
    pub timezone: Option<String>,
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
    pub approval_reminders_enabled: Option<bool>,
}

pub async fn public_settings(
    State(app_state): State<AppState>,
) -> AppResult<Json<PublicSettingsData>> {
    Ok(Json(load_all_public_settings(&app_state.pool).await?))
}

pub async fn admin_settings(
    State(app_state): State<AppState>,
    user: User,
) -> AppResult<Json<AdminSettingsData>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(load_admin_settings(&app_state.pool).await?))
}

pub async fn update_admin_settings(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdateSettings>,
) -> AppResult<Json<AdminSettingsData>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let language = normalize_language(&body.ui_language)?;
    let time_format = normalize_time_format(&body.time_format)?;
    let timezone = if let Some(tz) = body.timezone.as_deref() {
        normalize_timezone(tz)?
    } else {
        let stored = app_state.db.settings.get_raw(TIMEZONE_KEY).await?;
        normalize_timezone(stored.as_deref().unwrap_or(settings::DEFAULT_TIMEZONE))?
    };
    let country = body.country.trim().to_uppercase();
    let region = body.region.trim().to_string();
    let previous_country = app_state.db.settings.get_raw("country").await?;
    let previous_region = app_state.db.settings.get_raw("region").await?;

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
            // Validate that the MM-DD combination exists on a calendar.
            // Use a leap year baseline so 02-29 can be configured.
            if chrono::NaiveDate::from_ymd_opt(2024, month, day).is_none() {
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

    // Refresh holidays when the country/region changes.
    let prepared_holidays = if setting_value_changed(previous_country.as_deref(), &country)
        || setting_value_changed(previous_region.as_deref(), &region)
    {
        Some(
            crate::services::holidays::prepare_holiday_refresh(&app_state.pool, &country, &region).await?,
        )
    } else {
        None
    };

    // Save all settings atomically within a transaction.
    let mut transaction = app_state.db.settings.begin().await?;

    let carryover_date_to_store = validated_carryover_date.as_deref().unwrap_or("");
    save_setting_tx(&mut transaction, "carryover_expiry_date", carryover_date_to_store).await?;

    if let Some(day) = body.submission_deadline_day {
        save_setting_tx(&mut transaction, "submission_deadline_day", &day.to_string()).await?;
    } else {
        save_setting_tx(&mut transaction, "submission_deadline_day", "").await?;
    }

    save_setting_tx(&mut transaction, "ui_language", &language).await?;
    save_setting_tx(&mut transaction, "time_format", time_format).await?;
    save_setting_tx(&mut transaction, "timezone", &timezone).await?;
    save_setting_tx(&mut transaction, "country", &country).await?;
    save_setting_tx(&mut transaction, "region", &region).await?;
    save_setting_tx(&mut transaction, "default_weekly_hours", &default_weekly_hours_str).await?;
    save_setting_tx(
        &mut transaction,
        "default_annual_leave_days",
        &default_annual_leave_days_str,
    )
    .await?;
    save_setting_tx(&mut transaction, "organization_name", &org_name).await?;

    if let Some(ref holidays) = prepared_holidays {
        crate::services::holidays::replace_auto_holidays_exec(&mut transaction, holidays).await?;
    }

    transaction.commit().await?;

    audit::log(
        &app_state.pool,
        user.id,
        "updated",
        "settings",
        0,
        None,
        Some(serde_json::json!({
            "ui_language": language,
            "time_format": time_format,
            "timezone": timezone,
            "country": country,
            "region": region,
        })),
    )
    .await;

    Ok(Json(load_admin_settings(&app_state.pool).await?))
}

pub async fn update_smtp_settings(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdateSmtpSettings>,
) -> AppResult<Json<AdminSettingsData>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    let smtp_encryption = body
        .smtp_encryption
        .as_deref()
        .unwrap_or("starttls")
        .trim()
        .to_lowercase();
    if !matches!(smtp_encryption.as_str(), "starttls" | "tls" | "none") {
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
        from.parse::<Mailbox>()
            .map_err(|_| AppError::BadRequest("Invalid SMTP from address.".into()))?;

        // Test connection before saving when enabling.
        let test_config = smtp_config_from_update(
            &app_state.pool,
            body.smtp_host.trim(),
            body.smtp_port.unwrap_or(587),
            body.smtp_username.as_deref().unwrap_or("").trim(),
            body.smtp_password.as_deref(),
            body.smtp_from.trim(),
            &smtp_encryption,
        )
        .await?;
        crate::email::test_connection(&test_config)
            .await
            .map_err(|e| AppError::BadRequest(format!("SMTP_CONNECTION_FAILED:{e}")))?;
    }

    let smtp_config = smtp_config_from_update(
        &app_state.pool,
        body.smtp_host.trim(),
        body.smtp_port.unwrap_or(587),
        body.smtp_username.as_deref().unwrap_or("").trim(),
        body.smtp_password.as_deref(),
        body.smtp_from.trim(),
        &smtp_encryption,
    )
    .await?;

    // Save all SMTP settings atomically within a transaction.
    let mut transaction = app_state.db.settings.begin().await?;

    save_setting_tx(&mut transaction, "smtp_host", &smtp_config.host).await?;
    save_setting_tx(&mut transaction, "smtp_port", &smtp_config.port.to_string()).await?;
    save_setting_tx(
        &mut transaction,
        "smtp_username",
        smtp_config.username.as_deref().unwrap_or(""),
    )
    .await?;
    save_setting_tx(&mut transaction, "smtp_from", &smtp_config.from).await?;
    save_setting_tx(&mut transaction, "smtp_encryption", &smtp_config.encryption).await?;

    // Overwrite or clear the stored password when explicitly provided.
    if let Some(ref password) = body.smtp_password {
        save_setting_tx(&mut transaction, "smtp_password", password).await?;
    }

    save_setting_tx(
        &mut transaction,
        "smtp_enabled",
        if body.smtp_enabled { "true" } else { "false" },
    )
    .await?;

    let current_sub = load_setting(&app_state.pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true")
        .await?
        != "false";
    let sub_enabled = body.submission_reminders_enabled.unwrap_or(current_sub);
    save_setting_tx(
        &mut transaction,
        "submission_reminders_enabled",
        if sub_enabled { "true" } else { "false" },
    )
    .await?;

    let current_appr = load_setting(&app_state.pool, APPROVAL_REMINDERS_ENABLED_KEY, "true")
        .await?
        != "false";
    let appr_enabled = body.approval_reminders_enabled.unwrap_or(current_appr);
    save_setting_tx(
        &mut transaction,
        "approval_reminders_enabled",
        if appr_enabled { "true" } else { "false" },
    )
    .await?;

    transaction.commit().await?;

    audit::log(
        &app_state.pool,
        user.id,
        "updated",
        "smtp_settings",
        0,
        None,
        Some(serde_json::json!({
            "smtp_enabled": body.smtp_enabled,
            "smtp_host": smtp_config.host,
            "smtp_encryption": smtp_config.encryption,
        })),
    )
    .await;

    Ok(Json(load_admin_settings(&app_state.pool).await?))
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
    from.parse::<Mailbox>()
        .map_err(|_| AppError::BadRequest("Invalid SMTP from address.".into()))?;

    let smtp_encryption = body
        .smtp_encryption
        .as_deref()
        .unwrap_or("starttls")
        .trim()
        .to_lowercase();
    let test_config = smtp_config_from_update(
        &app_state.pool,
        host,
        body.smtp_port.unwrap_or(587),
        body.smtp_username.as_deref().unwrap_or("").trim(),
        body.smtp_password.as_deref(),
        from,
        &smtp_encryption,
    )
    .await?;
    crate::email::test_connection(&test_config)
        .await
        .map_err(|e| AppError::BadRequest(format!("SMTP_CONNECTION_FAILED:{e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
