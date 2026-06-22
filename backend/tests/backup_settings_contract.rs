//! Contract test: verifies that every `backup_*` and `backup_upload_*` key constant
//! defined in `services::settings` also appears as a literal string in `scripts/backup.sh`.
//!
//! This test runs without a database and catches key-name drift between the Rust
//! constants (which the Admin UI reads/writes) and the shell script (which reads
//! them at runtime via psql). Renaming a key on either side ⇒ this test fails.

use zerf::services::settings;

/// All setting key constants that `backup.sh` must reference as literal strings.
const BACKUP_KEYS: &[(&str, &str)] = &[
    ("BACKUP_INTERVAL_SECONDS_KEY", settings::BACKUP_INTERVAL_SECONDS_KEY),
    ("BACKUP_RETENTION_DAYS_KEY", settings::BACKUP_RETENTION_DAYS_KEY),
    ("BACKUP_UPLOAD_ENABLED_KEY", settings::BACKUP_UPLOAD_ENABLED_KEY),
    ("BACKUP_UPLOAD_URL_KEY", settings::BACKUP_UPLOAD_URL_KEY),
    ("BACKUP_UPLOAD_PASSWORD_KEY", settings::BACKUP_UPLOAD_PASSWORD_KEY),
];

#[test]
fn backup_sh_contains_all_backup_setting_keys() {
    // Find the script relative to this test file's manifest directory.
    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/backup.sh");
    let script = std::fs::read_to_string(&script_path)
        .expect("scripts/backup.sh must be readable from the repo root");

    let mut missing = Vec::new();
    for (const_name, key_value) in BACKUP_KEYS {
        if !script.contains(key_value) {
            missing.push(format!("  {const_name} = \"{key_value}\""));
        }
    }

    if !missing.is_empty() {
        panic!(
            "The following backup setting keys are defined in services/settings.rs \
             but NOT found as literal strings in scripts/backup.sh.\n\
             This means the shell script will silently fail to load these settings \
             at runtime. Update backup.sh to reference the correct key name(s):\n{}",
            missing.join("\n")
        );
    }
}

/// Sanity-check: verify the constant values are exactly what we expect so that
/// an accidental rename in services/settings.rs is also caught here.
#[test]
fn backup_key_constant_values_are_correct() {
    assert_eq!(settings::BACKUP_INTERVAL_SECONDS_KEY, "backup_interval_seconds");
    assert_eq!(settings::BACKUP_RETENTION_DAYS_KEY, "backup_retention_days");
    assert_eq!(settings::BACKUP_UPLOAD_ENABLED_KEY, "backup_upload_enabled");
    assert_eq!(settings::BACKUP_UPLOAD_URL_KEY, "backup_upload_url");
    assert_eq!(settings::BACKUP_UPLOAD_PASSWORD_KEY, "backup_upload_password");
}
