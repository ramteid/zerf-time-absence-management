//! Settings business logic: loading/saving app configuration, timezone helpers,
//! SMTP config, date utilities used throughout the application.

use crate::config::SmtpConfig;
use crate::error::AppResult;
use crate::repository::SettingsDb;

pub const TIMEZONE_KEY: &str = "timezone";
pub const SUBMISSION_REMINDERS_ENABLED_KEY: &str = "submission_reminders_enabled";
pub const APPROVAL_REMINDERS_ENABLED_KEY: &str = "approval_reminders_enabled";
pub const DEFAULT_TIMEZONE: &str = "Europe/Berlin";
pub const AUTO_BREAK_ENABLED_KEY: &str = "auto_break_enabled";
pub const AUTO_BREAK_THRESHOLD_HOURS_KEY: &str = "auto_break_threshold_hours";
pub const AUTO_BREAK_DEDUCTION_MINUTES_KEY: &str = "auto_break_deduction_minutes";
pub const AUTO_BREAK_THRESHOLD_HOURS_2_KEY: &str = "auto_break_threshold_hours_2";
pub const AUTO_BREAK_DEDUCTION_MINUTES_2_KEY: &str = "auto_break_deduction_minutes_2";

/// When TRUE, non-admin team leads may create and manage "assistant" (Aushilfe)
/// users that are assigned to them as approver. On by default; only an admin
/// can change this (the setting lives in the admin-only settings endpoint).
pub const ALLOW_TEAM_LEAD_MANAGE_ASSISTANTS_KEY: &str = "allow_team_lead_manage_assistants";

pub const UI_LANGUAGE_KEY: &str = "ui_language";
pub const TIME_FORMAT_KEY: &str = "time_format";
pub const COUNTRY_KEY: &str = "country";
pub const REGION_KEY: &str = "region";
pub const DEFAULT_WEEKLY_HOURS_KEY: &str = "default_weekly_hours";
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
pub const SUBMISSION_DEADLINE_DAY_KEY: &str = "submission_deadline_day";
pub const ORGANIZATION_NAME_KEY: &str = "organization_name";

/// Number of consecutive calendar days of illness (see
/// `services::medical_certificate`) after which a medical certificate (AU) is
/// considered required. Matches the common German statutory default of the
/// 4th calendar day (§ 5 EFZG).
pub const MEDICAL_CERTIFICATE_THRESHOLD_DAYS_KEY: &str = "medical_certificate_threshold_days";
pub const DEFAULT_MEDICAL_CERTIFICATE_THRESHOLD_DAYS: u32 = 4;

// Nextcloud upload — report PDF export (app reads/writes these).
pub const REPORT_UPLOAD_ENABLED_KEY: &str = "report_upload_enabled";
pub const REPORT_UPLOAD_URL_KEY: &str = "report_upload_url";
pub const REPORT_UPLOAD_PASSWORD_KEY: &str = "report_upload_password";
pub const REPORT_UPLOAD_DAY_OF_MONTH_KEY: &str = "report_upload_day_of_month";
/// Period for which the export queue was last populated, stored as "YYYY-MM".
/// Prevents re-populating the queue after all entries have been processed.
pub const REPORT_UPLOAD_QUEUE_PERIOD_KEY: &str = "report_upload_queue_period";

// Monthly payroll report email (tax office / payroll accountant).
pub const PAYROLL_REPORT_ENABLED_KEY: &str = "payroll_report_enabled";
/// Comma-separated recipient addresses. A single address is a valid
/// one-element list, so the singular key name still applies.
pub const PAYROLL_REPORT_RECIPIENT_KEY: &str = "payroll_report_recipient";
pub const PAYROLL_REPORT_DAY_OF_MONTH_KEY: &str = "payroll_report_day_of_month";
pub const PAYROLL_REPORT_ASSISTANT_HOURS_KEY: &str = "payroll_report_include_assistant_hours";
pub const PAYROLL_REPORT_EMPLOYEE_HOURS_KEY: &str = "payroll_report_include_employee_hours";
/// Comma-separated user IDs that are left out of the payroll report entirely —
/// they neither appear in the document nor hold its delivery up. Admins are
/// excluded unconditionally and are never part of this list.
pub const PAYROLL_REPORT_EXCLUDED_USERS_KEY: &str = "payroll_report_excluded_users";
/// Period for which the payroll report queue was last populated ("YYYY-MM").
/// Together with the queue table this also answers "has this month already
/// been delivered?" for the dashboard card — see
/// `services::payroll_report::build_status`.
pub const PAYROLL_REPORT_QUEUE_PERIOD_KEY: &str = "payroll_report_queue_period";

/// Newest period ("YYYY-MM") whose held-back payroll report was already
/// reported to the administrators. The nightly loop re-reaches a blocked period
/// every night, so without this marker the same warning would be raised every
/// day until the month is finished — and a dismissed notification would let it
/// through again. Compared with `>=` so several blocked periods cannot flip the
/// marker back and forth between them.
pub const PAYROLL_REPORT_BLOCKED_NOTIFIED_KEY: &str = "payroll_report_blocked_notified_period";

// Nextcloud upload — DB backup (backup container reads these via psql; app writes them).
pub const BACKUP_UPLOAD_ENABLED_KEY: &str = "backup_upload_enabled";
pub const BACKUP_UPLOAD_URL_KEY: &str = "backup_upload_url";
pub const BACKUP_UPLOAD_PASSWORD_KEY: &str = "backup_upload_password";

// Backup scheduling — migrated from env vars into app_settings.
pub const BACKUP_INTERVAL_DAYS_KEY: &str = "backup_interval_days";
/// UTC timestamp written by backup.sh after every **scheduled** backup only.
/// Persists across container restarts so the interval is measured from the
/// last actual backup, not from when the container started. A manual backup
/// (see `BACKUP_REQUESTED_AT_KEY`) deliberately does NOT update this key:
/// doing so would postpone the next scheduled run, and repeated manual runs
/// could starve the schedule indefinitely.
pub const BACKUP_LAST_SUCCESS_AT_KEY: &str = "backup_last_success_at";
/// UTC timestamp written by the admin's "Back up now" button (see
/// `request_backup_now`). The backup container's polling loop treats a
/// non-empty value that it hasn't already handled as a request to back up
/// immediately, regardless of `BACKUP_INTERVAL_DAYS_KEY`. Never cleared by
/// the app — the backup container is solely responsible for tracking which
/// requests it has already handled (`backup_last_request_handled_at`, a
/// script-internal key with no Rust constant), so that a failed clear can
/// never re-trigger a loop of repeated backups.
pub const BACKUP_REQUESTED_AT_KEY: &str = "backup_requested_at";
/// UTC timestamp written by backup.sh after every **manual** backup
/// (triggered via `BACKUP_REQUESTED_AT_KEY`). Kept separate from
/// `BACKUP_LAST_SUCCESS_AT_KEY` for the reason documented there. The admin
/// settings UI shows the more recent of the two.
pub const BACKUP_LAST_MANUAL_AT_KEY: &str = "backup_last_manual_at";

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

/// Whether team leads (non-admin) are allowed to create/manage assistant users
/// assigned to them. See [`ALLOW_TEAM_LEAD_MANAGE_ASSISTANTS_KEY`].
pub async fn team_lead_assistant_management_enabled(
    pool: &crate::db::DatabasePool,
) -> AppResult<bool> {
    // Default is "false": scoped assistant management is an opt-in capability
    // (user-guide: "off by default"). An admin can enable it via the general
    // settings page; every /team-users* request is rejected until then.
    Ok(load_setting(pool, ALLOW_TEAM_LEAD_MANAGE_ASSISTANTS_KEY, "false").await? == "true")
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

/// Record an on-demand backup request for the backup container's polling loop
/// to pick up (see `BACKUP_REQUESTED_AT_KEY`). The value only needs to be a
/// unique, non-empty token — the shell script never parses it as a date, it
/// just compares it for equality against what it has already handled — so an
/// RFC 3339 timestamp is used purely for human-readable debugging when
/// inspecting `app_settings` directly.
pub async fn request_backup_now(pool: &crate::db::DatabasePool) -> AppResult<()> {
    let db = SettingsDb::new(pool.clone());
    let mut tx = db.begin().await?;
    let now = chrono::Utc::now().to_rfc3339();
    save_setting_tx(&mut tx, BACKUP_REQUESTED_AT_KEY, &now).await?;
    tx.commit().await?;
    Ok(())
}

/// Load settings that are shown in the public (unauthenticated) settings response.
pub async fn load_all_public_settings(
    pool: &crate::db::DatabasePool,
) -> AppResult<PublicSettingsData> {
    let default_weekly_hours_str = load_setting(pool, DEFAULT_WEEKLY_HOURS_KEY, "").await?;
    let submission_deadline_day_str = load_setting(pool, SUBMISSION_DEADLINE_DAY_KEY, "").await?;
    let auto_break_threshold_str = load_setting(pool, AUTO_BREAK_THRESHOLD_HOURS_KEY, "").await?;
    let auto_break_deduction_str = load_setting(pool, AUTO_BREAK_DEDUCTION_MINUTES_KEY, "").await?;
    let auto_break_threshold_2_str =
        load_setting(pool, AUTO_BREAK_THRESHOLD_HOURS_2_KEY, "").await?;
    let auto_break_deduction_2_str =
        load_setting(pool, AUTO_BREAK_DEDUCTION_MINUTES_2_KEY, "").await?;
    Ok(PublicSettingsData {
        ui_language: load_setting(pool, UI_LANGUAGE_KEY, DEFAULT_UI_LANGUAGE).await?,
        time_format: load_setting(pool, TIME_FORMAT_KEY, DEFAULT_TIME_FORMAT).await?,
        timezone: load_setting(pool, TIMEZONE_KEY, DEFAULT_TIMEZONE).await?,
        country: load_setting(pool, COUNTRY_KEY, DEFAULT_COUNTRY).await?,
        region: load_setting(pool, REGION_KEY, DEFAULT_REGION).await?,
        default_weekly_hours: default_weekly_hours_str.parse().ok(),
        submission_deadline_day: submission_deadline_day_str.parse().ok(),
        organization_name: load_setting(pool, ORGANIZATION_NAME_KEY, "").await?,
        auto_break_enabled: load_setting(pool, AUTO_BREAK_ENABLED_KEY, "false").await? == "true",
        auto_break_threshold_hours: auto_break_threshold_str.parse().ok(),
        auto_break_deduction_minutes: auto_break_deduction_str.parse().ok(),
        auto_break_threshold_hours_2: auto_break_threshold_2_str.parse().ok(),
        auto_break_deduction_minutes_2: auto_break_deduction_2_str.parse().ok(),
        smtp_enabled: load_setting(pool, SMTP_ENABLED_KEY, "false").await? == "true",
    })
}

/// `app_settings` stores "unset" as an empty string (the `load_setting`
/// default); convert that to `None` for API responses that should omit it.
fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Load the full admin settings response (public settings + SMTP + reminders + upload).
pub async fn load_admin_settings(pool: &crate::db::DatabasePool) -> AppResult<AdminSettingsData> {
    let base = load_all_public_settings(pool).await?;
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

    let report_upload_enabled =
        load_setting(pool, REPORT_UPLOAD_ENABLED_KEY, "false").await? == "true";
    let report_upload_url = load_setting(pool, REPORT_UPLOAD_URL_KEY, "").await?;
    let report_upload_password_set = !load_setting(pool, REPORT_UPLOAD_PASSWORD_KEY, "")
        .await?
        .is_empty();
    let report_upload_day_of_month: u8 = load_setting(pool, REPORT_UPLOAD_DAY_OF_MONTH_KEY, "5")
        .await?
        .parse()
        .unwrap_or(5);

    let backup_upload_enabled =
        load_setting(pool, BACKUP_UPLOAD_ENABLED_KEY, "false").await? == "true";
    let backup_upload_url = load_setting(pool, BACKUP_UPLOAD_URL_KEY, "").await?;
    let backup_upload_password_set = !load_setting(pool, BACKUP_UPLOAD_PASSWORD_KEY, "")
        .await?
        .is_empty();
    let backup_interval_days: u32 = load_setting(pool, BACKUP_INTERVAL_DAYS_KEY, "1")
        .await?
        .parse()
        .unwrap_or(1);
    let backup_last_success_at =
        non_empty(load_setting(pool, BACKUP_LAST_SUCCESS_AT_KEY, "").await?);
    let backup_last_manual_at = non_empty(load_setting(pool, BACKUP_LAST_MANUAL_AT_KEY, "").await?);

    let allow_team_lead_manage_assistants = team_lead_assistant_management_enabled(pool).await?;

    let medical_certificate_threshold_days: u32 = load_setting(
        pool,
        MEDICAL_CERTIFICATE_THRESHOLD_DAYS_KEY,
        &DEFAULT_MEDICAL_CERTIFICATE_THRESHOLD_DAYS.to_string(),
    )
    .await?
    .parse()
    .unwrap_or(DEFAULT_MEDICAL_CERTIFICATE_THRESHOLD_DAYS);

    let payroll_report = crate::services::payroll_report::load_config(pool).await?;
    let payroll_relevant_categories =
        crate::services::payroll_report::payroll_relevant_categories(pool).await?;

    Ok(AdminSettingsData {
        base,
        smtp_host: host,
        smtp_port: port,
        smtp_username: username,
        smtp_from: from,
        smtp_encryption: encryption,
        smtp_password_set: password_set,
        submission_reminders_enabled,
        approval_reminders_enabled,
        report_upload_enabled,
        report_upload_url,
        report_upload_password_set,
        report_upload_day_of_month,
        backup_upload_enabled,
        backup_upload_url,
        backup_upload_password_set,
        backup_interval_days,
        backup_last_success_at,
        backup_last_manual_at,
        allow_team_lead_manage_assistants,
        medical_certificate_threshold_days,
        payroll_report_enabled: payroll_report.enabled,
        payroll_report_recipients: payroll_report.recipients,
        payroll_report_day_of_month: payroll_report.day_of_month,
        payroll_report_absence_categories: payroll_relevant_categories
            .iter()
            .map(|category| category.slug.clone())
            .collect(),
        payroll_report_include_assistant_hours: payroll_report.include_assistant_hours,
        payroll_report_include_employee_hours: payroll_report.include_employee_hours,
        payroll_report_excluded_user_ids: payroll_report.excluded_user_ids,
        payroll_report_send_now_period: crate::services::payroll_report::manual_send_target(pool)
            .await?
            .period,
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
    pub submission_deadline_day: Option<u8>,
    pub organization_name: String,
    pub auto_break_enabled: bool,
    /// Tier-1: minimum consecutive hours worked before a break is deducted.
    pub auto_break_threshold_hours: Option<f64>,
    /// Tier-1: minutes deducted when the tier-1 threshold is the highest one reached.
    pub auto_break_deduction_minutes: Option<i32>,
    /// Tier-2 (optional): higher threshold that supersedes tier-1 when reached.
    pub auto_break_threshold_hours_2: Option<f64>,
    /// Tier-2 (optional): total minutes deducted when the tier-2 threshold is reached.
    pub auto_break_deduction_minutes_2: Option<i32>,
    /// Whether SMTP email delivery is configured and enabled.
    pub smtp_enabled: bool,
}

/// Full admin settings (public settings + SMTP config + reminder flags + upload settings).
#[derive(serde::Serialize)]
pub struct AdminSettingsData {
    #[serde(flatten)]
    pub base: PublicSettingsData,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_from: String,
    pub smtp_encryption: String,
    /// True when a password is stored (never returned in cleartext).
    pub smtp_password_set: bool,
    pub submission_reminders_enabled: bool,
    pub approval_reminders_enabled: bool,
    // --- Nextcloud upload: monthly timesheet PDF ---
    pub report_upload_enabled: bool,
    pub report_upload_url: String,
    /// True when a share password is stored (never returned in cleartext).
    pub report_upload_password_set: bool,
    pub report_upload_day_of_month: u8,
    // --- Nextcloud upload: DB backup ---
    pub backup_upload_enabled: bool,
    pub backup_upload_url: String,
    /// True when a share password is stored (never returned in cleartext).
    pub backup_upload_password_set: bool,
    /// Interval between backups in days (read by backup.sh from app_settings).
    pub backup_interval_days: u32,
    /// UTC timestamp of the last successful *scheduled* backup, or `None` if
    /// none has run yet. Does not reflect manual runs — see `backup_last_manual_at`.
    pub backup_last_success_at: Option<String>,
    /// UTC timestamp of the last successful *manual* ("Back up now") backup,
    /// or `None` if none has run yet.
    pub backup_last_manual_at: Option<String>,
    /// When TRUE, non-admin team leads may create/manage "assistant" users
    /// assigned to them (see `/team-users*`). On by default.
    pub allow_team_lead_manage_assistants: bool,
    /// Consecutive calendar days of illness after which a medical certificate
    /// (AU) is considered required — see `services::medical_certificate`.
    pub medical_certificate_threshold_days: u32,
    // --- Monthly payroll report email (tax office / payroll accountant) ---
    pub payroll_report_enabled: bool,
    /// Recipient addresses, all equal (no primary/CC distinction).
    pub payroll_report_recipients: Vec<String>,
    pub payroll_report_day_of_month: u8,
    /// Read-only: absence category slugs the report currently includes
    /// automatically (sick-like, or costing neither vacation nor flextime).
    /// Not admin-editable — see `AbsenceCategory::is_payroll_relevant`.
    pub payroll_report_absence_categories: Vec<String>,
    pub payroll_report_include_assistant_hours: bool,
    pub payroll_report_include_employee_hours: bool,
    /// User IDs left out of the report entirely. Admins are always left out and
    /// are never listed here.
    pub payroll_report_excluded_user_ids: Vec<i64>,
    /// Read-only: the month "Send now" would currently target, "YYYY-MM", so
    /// the button can name it. See
    /// `services::payroll_report::manual_send_target`.
    pub payroll_report_send_now_period: String,
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
