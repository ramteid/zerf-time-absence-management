//! HTTP handlers for application settings (public, admin, SMTP).

use crate::audit;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::User;
use crate::services::settings::{
    self, load_admin_settings, load_all_public_settings, load_setting, normalize_language,
    normalize_time_format, normalize_timezone, save_setting_tx, setting_value_changed,
    smtp_config_from_update, AdminSettingsData, PublicSettingsData, APPROVAL_REMINDERS_ENABLED_KEY,
    AUTO_BREAK_DEDUCTION_MINUTES_2_KEY, AUTO_BREAK_THRESHOLD_HOURS_2_KEY,
    SUBMISSION_REMINDERS_ENABLED_KEY, TIMEZONE_KEY,
};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use lettre::message::Mailbox;
use serde::Deserialize;

// All setting key constants are used via `settings::` module — no re-imports needed.

/// Persist one optional settings field inside an open transaction.
/// `None` means "the request did not touch this field", so the stored value is
/// kept. The three arms cover the value shapes the settings tabs submit:
/// strings (default), booleans, and numbers.
macro_rules! save_if_some {
    ($transaction:expr, $key:expr, $value:expr) => {
        if let Some(ref value) = $value {
            save_setting_tx(&mut $transaction, $key, value).await?;
        }
    };
    ($transaction:expr, $key:expr, $value:expr, bool) => {
        if let Some(value) = $value {
            save_setting_tx(
                &mut $transaction,
                $key,
                if value { "true" } else { "false" },
            )
            .await?;
        }
    };
    ($transaction:expr, $key:expr, $value:expr, num) => {
        if let Some(value) = $value {
            save_setting_tx(&mut $transaction, $key, &value.to_string()).await?;
        }
    };
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub ui_language: String,
    pub time_format: String,
    pub timezone: Option<String>,
    pub country: String,
    pub region: String,
    pub default_weekly_hours: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_double_option_u8")]
    pub submission_deadline_day: Option<Option<u8>>,
    pub organization_name: Option<String>,
    pub auto_break_enabled: Option<bool>,
    pub auto_break_threshold_hours: Option<f64>,
    pub auto_break_deduction_minutes: Option<i32>,
    pub auto_break_threshold_hours_2: Option<f64>,
    pub auto_break_deduction_minutes_2: Option<i32>,
    pub allow_team_lead_manage_assistants: Option<bool>,
}

fn deserialize_double_option_u8<'de, D>(deserializer: D) -> Result<Option<Option<u8>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Option<u8> will be None for null, Some for number. We wrap it in Some to indicate field presence.
    let opt = Option::<u8>::deserialize(deserializer)?;
    Ok(Some(opt))
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
    if let Some(Some(day)) = body.submission_deadline_day {
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

    // Validate automatic break deduction settings only when the flag is explicitly provided.
    if let Some(enabled) = body.auto_break_enabled {
        if enabled {
            let threshold1 = body.auto_break_threshold_hours.ok_or_else(|| {
                AppError::BadRequest(
                    "auto_break_threshold_hours is required when auto_break_enabled.".into(),
                )
            })?;
            let deduction1 = body.auto_break_deduction_minutes.ok_or_else(|| {
                AppError::BadRequest(
                    "auto_break_deduction_minutes is required when auto_break_enabled.".into(),
                )
            })?;
            if threshold1 <= 0.0 || threshold1 > 24.0 {
                return Err(AppError::BadRequest(
                    "auto_break_threshold_hours must be between 0 and 24.".into(),
                ));
            }
            if deduction1 <= 0 || deduction1 > 480 {
                return Err(AppError::BadRequest(
                    "auto_break_deduction_minutes must be between 1 and 480.".into(),
                ));
            }
            let has_tier2_threshold = body.auto_break_threshold_hours_2.is_some();
            let has_tier2_deduction = body.auto_break_deduction_minutes_2.is_some();
            if has_tier2_threshold != has_tier2_deduction {
                return Err(AppError::BadRequest(
                    "Both auto_break_threshold_hours_2 and auto_break_deduction_minutes_2 must be provided together.".into(),
                ));
            }
            if has_tier2_threshold {
                let threshold2 = body.auto_break_threshold_hours_2.unwrap();
                let deduction2 = body.auto_break_deduction_minutes_2.unwrap();
                if threshold2 <= threshold1 {
                    return Err(AppError::BadRequest(
                        "auto_break_threshold_hours_2 must be greater than auto_break_threshold_hours."
                            .into(),
                    ));
                }
                if threshold2 > 24.0 {
                    return Err(AppError::BadRequest(
                        "auto_break_threshold_hours_2 must be between 0 and 24.".into(),
                    ));
                }
                if deduction2 <= 0 || deduction2 > 480 {
                    return Err(AppError::BadRequest(
                        "auto_break_deduction_minutes_2 must be between 1 and 480.".into(),
                    ));
                }
            }
        }
    }

    // Refresh holidays when the country/region changes.
    let prepared_holidays = if setting_value_changed(previous_country.as_deref(), &country)
        || setting_value_changed(previous_region.as_deref(), &region)
    {
        Some(
            crate::services::holidays::prepare_holiday_refresh(&app_state.pool, &country, &region)
                .await?,
        )
    } else {
        None
    };

    // Save settings atomically – only touch optional fields when they are explicitly provided (boyscout: partial update must not wipe).
    let mut transaction = app_state.db.settings.begin().await?;

    save_setting_tx(&mut transaction, "ui_language", &language).await?;
    save_setting_tx(&mut transaction, "time_format", time_format).await?;
    save_setting_tx(&mut transaction, "timezone", &timezone).await?;
    save_setting_tx(&mut transaction, "country", &country).await?;
    save_setting_tx(&mut transaction, "region", &region).await?;

    save_if_some!(
        transaction,
        "default_weekly_hours",
        body.default_weekly_hours.map(|v| v.to_string())
    );
    if let Some(inner) = body.submission_deadline_day {
        match inner {
            Some(v) => {
                save_setting_tx(&mut transaction, "submission_deadline_day", &v.to_string())
                    .await?;
            }
            None => {
                save_setting_tx(&mut transaction, "submission_deadline_day", "").await?;
            }
        }
    }
    save_if_some!(
        transaction,
        "organization_name",
        body.organization_name.map(|v| v.trim().to_string())
    );

    // Auto break: only save when flag is provided; otherwise keep existing values.
    if let Some(enabled) = body.auto_break_enabled {
        save_setting_tx(
            &mut transaction,
            "auto_break_enabled",
            if enabled { "true" } else { "false" },
        )
        .await?;
        if enabled {
            save_if_some!(
                transaction,
                "auto_break_threshold_hours",
                body.auto_break_threshold_hours.map(|v| v.to_string())
            );
            save_if_some!(
                transaction,
                "auto_break_deduction_minutes",
                body.auto_break_deduction_minutes.map(|v| v.to_string())
            );
            save_if_some!(
                transaction,
                AUTO_BREAK_THRESHOLD_HOURS_2_KEY,
                body.auto_break_threshold_hours_2.map(|v| v.to_string())
            );
            save_if_some!(
                transaction,
                AUTO_BREAK_DEDUCTION_MINUTES_2_KEY,
                body.auto_break_deduction_minutes_2.map(|v| v.to_string())
            );
        } else {
            save_setting_tx(&mut transaction, "auto_break_threshold_hours", "").await?;
            save_setting_tx(&mut transaction, "auto_break_deduction_minutes", "").await?;
            save_setting_tx(&mut transaction, AUTO_BREAK_THRESHOLD_HOURS_2_KEY, "").await?;
            save_setting_tx(&mut transaction, AUTO_BREAK_DEDUCTION_MINUTES_2_KEY, "").await?;
        }
    }

    if let Some(ref holidays) = prepared_holidays {
        crate::services::holidays::replace_auto_holidays_exec(&mut transaction, holidays).await?;
    }

    save_if_some!(
        transaction,
        settings::ALLOW_TEAM_LEAD_MANAGE_ASSISTANTS_KEY,
        body.allow_team_lead_manage_assistants.map(|v| if v { "true" } else { "false" }.to_string())
    );

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

    let current_sub =
        load_setting(&app_state.pool, SUBMISSION_REMINDERS_ENABLED_KEY, "true").await? != "false";
    let sub_enabled = body.submission_reminders_enabled.unwrap_or(current_sub);
    save_setting_tx(
        &mut transaction,
        "submission_reminders_enabled",
        if sub_enabled { "true" } else { "false" },
    )
    .await?;

    let current_appr =
        load_setting(&app_state.pool, APPROVAL_REMINDERS_ENABLED_KEY, "true").await? != "false";
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

/// Request body for updating Nextcloud upload settings.
#[derive(Deserialize)]
pub struct UpdateUploadSettings {
    // Report PDF upload
    pub report_upload_enabled: Option<bool>,
    pub report_upload_url: Option<String>,
    /// `None` = keep stored password; `Some("")` = clear password; `Some("...")` = update.
    pub report_upload_password: Option<String>,
    pub report_upload_day_of_month: Option<u8>,
    // DB backup upload
    pub backup_upload_enabled: Option<bool>,
    pub backup_upload_url: Option<String>,
    pub backup_upload_password: Option<String>,
    pub backup_interval_days: Option<u32>,
}

pub async fn update_upload_settings(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdateUploadSettings>,
) -> AppResult<Json<AdminSettingsData>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    // Validate share URLs when provided and non-empty.
    if let Some(ref url) = body.report_upload_url {
        if !url.is_empty() {
            crate::services::nextcloud::parse_share_url(url)?;
        }
    }
    if let Some(ref url) = body.backup_upload_url {
        if !url.is_empty() {
            crate::services::nextcloud::parse_share_url(url)?;
        }
    }
    if let Some(day) = body.report_upload_day_of_month {
        if !(1..=28).contains(&day) {
            return Err(AppError::BadRequest(
                "report_upload_day_of_month must be between 1 and 28.".into(),
            ));
        }
    }
    if let Some(days) = body.backup_interval_days {
        if days == 0 {
            return Err(AppError::BadRequest(
                "backup_interval_days must be at least 1.".into(),
            ));
        }
    }

    // Cross-field: backup upload enabled requires URL.
    let effective_backup_upload_enabled = match body.backup_upload_enabled {
        Some(v) => v,
        None => {
            load_setting(
                &app_state.pool,
                settings::BACKUP_UPLOAD_ENABLED_KEY,
                "false",
            )
            .await?
                == "true"
        }
    };
    if effective_backup_upload_enabled {
        let effective_backup_upload_url = match &body.backup_upload_url {
            Some(v) => v.clone(),
            None => load_setting(&app_state.pool, settings::BACKUP_UPLOAD_URL_KEY, "").await?,
        };
        if effective_backup_upload_url.trim().is_empty() {
            return Err(AppError::BadRequest(
                "A Nextcloud share URL is required to enable database backup upload.".into(),
            ));
        }
        crate::services::nextcloud::parse_share_url(&effective_backup_upload_url)?;
    }
    // Same validation for report upload (previously missing – admin could enable without URL).
    let effective_report_upload_enabled = match body.report_upload_enabled {
        Some(v) => v,
        None => {
            load_setting(
                &app_state.pool,
                settings::REPORT_UPLOAD_ENABLED_KEY,
                "false",
            )
            .await?
                == "true"
        }
    };
    if effective_report_upload_enabled {
        let effective_report_upload_url = match &body.report_upload_url {
            Some(v) => v.clone(),
            None => load_setting(&app_state.pool, settings::REPORT_UPLOAD_URL_KEY, "").await?,
        };
        if effective_report_upload_url.trim().is_empty() {
            return Err(AppError::BadRequest(
                "A Nextcloud share URL is required to enable report PDF upload.".into(),
            ));
        }
        crate::services::nextcloud::parse_share_url(&effective_report_upload_url)?;
    }

    let mut transaction = app_state.db.settings.begin().await?;

    save_if_some!(
        transaction,
        settings::REPORT_UPLOAD_ENABLED_KEY,
        body.report_upload_enabled,
        bool
    );
    save_if_some!(
        transaction,
        settings::REPORT_UPLOAD_URL_KEY,
        body.report_upload_url
    );
    // Password: None = keep, Some("") = clear, Some("...") = update.
    if let Some(ref pw) = body.report_upload_password {
        save_setting_tx(&mut transaction, settings::REPORT_UPLOAD_PASSWORD_KEY, pw).await?;
    }
    save_if_some!(
        transaction,
        settings::REPORT_UPLOAD_DAY_OF_MONTH_KEY,
        body.report_upload_day_of_month,
        num
    );
    save_if_some!(
        transaction,
        settings::BACKUP_UPLOAD_ENABLED_KEY,
        body.backup_upload_enabled,
        bool
    );
    save_if_some!(
        transaction,
        settings::BACKUP_UPLOAD_URL_KEY,
        body.backup_upload_url
    );
    if let Some(ref pw) = body.backup_upload_password {
        save_setting_tx(&mut transaction, settings::BACKUP_UPLOAD_PASSWORD_KEY, pw).await?;
    }
    save_if_some!(
        transaction,
        settings::BACKUP_INTERVAL_DAYS_KEY,
        body.backup_interval_days,
        num
    );

    transaction.commit().await?;

    audit::log(
        &app_state.pool,
        user.id,
        "updated",
        "upload_settings",
        0,
        None,
        Some(serde_json::json!({
            "report_upload_enabled": body.report_upload_enabled,
            "backup_upload_enabled": body.backup_upload_enabled,
        })),
    )
    .await;

    Ok(Json(load_admin_settings(&app_state.pool).await?))
}

/// Request body for the monthly payroll report settings. Every field is
/// optional so the tab can save individual changes; omitted fields keep their
/// stored value.
#[derive(Deserialize)]
pub struct UpdatePayrollReportSettings {
    pub payroll_report_enabled: Option<bool>,
    /// Recipient addresses; all are equal (everyone goes in `To`, none is
    /// primary/CC).
    pub payroll_report_recipients: Option<Vec<String>>,
    pub payroll_report_day_of_month: Option<u8>,
    pub payroll_report_include_assistant_hours: Option<bool>,
    pub payroll_report_include_employee_hours: Option<bool>,
    /// People to leave out of the report entirely. An empty list means
    /// everybody (except admins, who are never included) is covered.
    pub payroll_report_excluded_user_ids: Option<Vec<i64>>,
}

pub async fn update_payroll_report_settings(
    State(app_state): State<AppState>,
    user: User,
    Json(body): Json<UpdatePayrollReportSettings>,
) -> AppResult<Json<AdminSettingsData>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }

    if let Some(day) = body.payroll_report_day_of_month {
        if !(1..=28).contains(&day) {
            return Err(AppError::BadRequest(
                "payroll_report_day_of_month must be between 1 and 28.".into(),
            ));
        }
    }

    // Every recipient must be a valid address; duplicates are folded
    // case-insensitively so the same person entered twice doesn't get the
    // report twice.
    let recipients = match body.payroll_report_recipients {
        Some(ref addresses) => {
            let normalized = crate::services::payroll_report::format_recipient_list(addresses);
            for address in crate::services::payroll_report::parse_recipient_list(&normalized) {
                address.parse::<Mailbox>().map_err(|_| {
                    AppError::BadRequest("Invalid payroll report recipient.".into())
                })?;
            }
            Some(normalized)
        }
        None => None,
    };

    // Everything the report needs must be present once it is switched on:
    // no recipient or a report with no sections could never be sent.
    // Each field saves independently, so compute the effective end state by
    // falling back to what is stored for the fields this request omits.
    let stored = crate::services::payroll_report::load_config(&app_state.pool).await?;
    let effective = crate::services::payroll_report::PayrollReportConfig {
        enabled: body.payroll_report_enabled.unwrap_or(stored.enabled),
        recipients: recipients
            .as_deref()
            .map(crate::services::payroll_report::parse_recipient_list)
            .unwrap_or(stored.recipients),
        day_of_month: body
            .payroll_report_day_of_month
            .unwrap_or(stored.day_of_month),
        include_assistant_hours: body
            .payroll_report_include_assistant_hours
            .unwrap_or(stored.include_assistant_hours),
        include_employee_hours: body
            .payroll_report_include_employee_hours
            .unwrap_or(stored.include_employee_hours),
        excluded_user_ids: body
            .payroll_report_excluded_user_ids
            .clone()
            .unwrap_or(stored.excluded_user_ids),
    };
    if effective.enabled {
        if effective.recipients.is_empty() {
            return Err(AppError::BadRequest(
                "A recipient address is required to enable the payroll report.".into(),
            ));
        }
        let relevant_categories =
            crate::services::payroll_report::payroll_relevant_categories(&app_state.pool).await?;
        if effective.has_no_content(&relevant_categories) {
            return Err(AppError::BadRequest(
                "Select at least one section for the payroll report.".into(),
            ));
        }
    }

    let mut transaction = app_state.db.settings.begin().await?;
    save_if_some!(
        transaction,
        settings::PAYROLL_REPORT_ENABLED_KEY,
        body.payroll_report_enabled,
        bool
    );
    save_if_some!(
        transaction,
        settings::PAYROLL_REPORT_RECIPIENT_KEY,
        recipients
    );
    save_if_some!(
        transaction,
        settings::PAYROLL_REPORT_DAY_OF_MONTH_KEY,
        body.payroll_report_day_of_month,
        num
    );
    save_if_some!(
        transaction,
        settings::PAYROLL_REPORT_ASSISTANT_HOURS_KEY,
        body.payroll_report_include_assistant_hours,
        bool
    );
    save_if_some!(
        transaction,
        settings::PAYROLL_REPORT_EMPLOYEE_HOURS_KEY,
        body.payroll_report_include_employee_hours,
        bool
    );
    save_if_some!(
        transaction,
        settings::PAYROLL_REPORT_EXCLUDED_USERS_KEY,
        body.payroll_report_excluded_user_ids
            .as_deref()
            .map(crate::services::payroll_report::format_excluded_ids)
    );
    transaction.commit().await?;

    audit::log(
        &app_state.pool,
        user.id,
        "updated",
        "payroll_report_settings",
        0,
        None,
        Some(serde_json::json!({
            "payroll_report_enabled": body.payroll_report_enabled,
            "payroll_report_day_of_month": body.payroll_report_day_of_month,
            "payroll_report_excluded_user_ids": body.payroll_report_excluded_user_ids,
        })),
    )
    .await;

    Ok(Json(load_admin_settings(&app_state.pool).await?))
}

/// Trigger an immediate payroll report run: queue the previous month
/// (idempotent) and send every queued month that is ready.
/// Does not affect the scheduled monthly run.
pub async fn run_payroll_report_now(
    State(app_state): State<AppState>,
    user: User,
) -> AppResult<Json<serde_json::Value>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    let summary = crate::background::payroll_report::run_now(&app_state).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "sent": summary.sent,
        "pending": summary.pending,
    })))
}

/// Trigger an immediate report upload: populate the queue for the previous
/// month (idempotent) and process all pending entries.
/// Does not affect the scheduled monthly run.
pub async fn run_report_upload_now(
    State(app_state): State<AppState>,
    user: User,
) -> AppResult<Json<serde_json::Value>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    crate::background::report_upload::run_now(&app_state).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Request an immediate database backup. The app cannot perform the backup
/// itself (the backup container deliberately holds the only copy of
/// `ZERF_DB_ENCRYPTION_KEY` and is network-isolated from `app` — see
/// docker-compose); this just records the request in `app_settings` for the
/// backup container's polling loop to pick up, typically within ~20s. Does
/// not affect the scheduled interval-based backup.
pub async fn run_backup_now(
    State(app_state): State<AppState>,
    user: User,
) -> AppResult<Json<serde_json::Value>> {
    if !user.is_admin() {
        return Err(AppError::Forbidden);
    }
    settings::request_backup_now(&app_state.pool).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
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
