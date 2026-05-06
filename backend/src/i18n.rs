//! Backend translations for server-rendered messages.
//!
//! All language-specific data lives in the `LANGUAGES` table below.
//! To add a new language, append one entry to `LANGUAGES` -- no other
//! constants, functions, or enum variants need to change.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::db::DatabasePool;

const UI_LANGUAGE_KEY: &str = "ui_language";
const DEFAULT_LANGUAGE_CODE: &str = "en";

// -- Language definition table ------------------------------------------------
// Each row contains all data needed for one language.
// `translations` is a flat slice of (key, template) pairs.

struct LangDef {
    code: &'static str,
    name: &'static str,
    date_format: &'static str,
    translations: &'static [(&'static str, &'static str)],
}

static LANGUAGES: &[LangDef] = &[
    LangDef {
        code: "en",
        name: "English",
        date_format: "%m/%d/%Y",
        translations: &[
            ("entry_singular", "1 entry"),
            ("entry_plural", "{count} entries"),
            ("month_1", "January"), ("month_2", "February"), ("month_3", "March"),
            ("month_4", "April"), ("month_5", "May"), ("month_6", "June"),
            ("month_7", "July"), ("month_8", "August"), ("month_9", "September"),
            ("month_10", "October"), ("month_11", "November"), ("month_12", "December"),
            ("reopen_auto_approved_title", "Week reopened for editing"),
            ("reopen_auto_approved_body", "The week starting {week_start} was reopened for editing ({entry_count})."),
            ("reopen_auto_approved_notice_title", "Week reopen auto-approved"),
            ("reopen_auto_approved_notice_body", "{requester_name}'s week reopen for the week starting {week_start} was auto-approved ({entry_count})."),
            ("reopen_request_created_title", "New week reopen request"),
            ("reopen_request_created_body", "{requester_name} wants to edit the week starting {week_start}."),
            ("reopen_approved_title", "Week reopen approved"),
            ("reopen_approved_body", "Your week starting {week_start} was reopened for editing."),
            ("reopen_approved_by_admin_title", "Week reopen approved by admin"),
            ("reopen_approved_by_admin_body", "The week reopen request from {requester_name} for the week starting {week_start} was approved by an admin."),
            ("reopen_rejected_title", "Week reopen rejected"),
            ("reopen_rejected_body", "Your request to edit the week starting {week_start} was rejected: {reason}"),
            ("reopen_rejected_by_admin_title", "Week reopen rejected by admin"),
            ("reopen_rejected_by_admin_body", "The week reopen request from {requester_name} for the week starting {week_start} was rejected by an admin: {reason}"),
            ("absence_requested_title", "New absence request"),
            ("absence_requested_body", "{requester_name} requested an absence from {start_date} to {end_date}."),
            ("absence_approved_title", "Absence approved"),
            ("absence_approved_body", "Your absence ({start_date} to {end_date}) has been approved."),
            ("absence_rejected_title", "Absence rejected"),
            ("absence_rejected_body", "Your absence ({start_date} to {end_date}) was rejected: {reason}"),
            ("absence_revoked_title", "Absence revoked"),
            ("absence_revoked_body", "Your absence ({start_date} to {end_date}) has been revoked by an administrator."),
            ("change_request_created_title", "New change request"),
            ("change_request_created_body", "{requester_name} requested a change for the time entry on {entry_date}."),
            ("timesheet_submitted_title", "{submitter_name} submitted a timesheet"),
            ("timesheet_submitted_body", "{entry_count} submitted for approval"),
            ("timesheet_approved_title", "Timesheet approved"),
            ("timesheet_approved_body", "Your timesheet entry for {entry_date} has been approved."),
            ("timesheet_rejected_title", "Timesheet rejected"),
            ("timesheet_rejected_body", "Your timesheet entry for {entry_date} was rejected: {reason}"),
            ("submission_reminder_title", "Time entries not yet submitted"),
            ("submission_reminder_body", "You still have unsubmitted time entries for the following months: {months}"),
            ("submission_reminder_email_body", "Hello,\n\nyou still have unsubmitted time entries for the following months:\n\n{months}\n\nPlease log in and submit your time entries:\n{app_url}\n"),
        ],
    },
    LangDef {
        code: "de",
        name: "Deutsch",
        date_format: "%d.%m.%Y",
        translations: &[
            ("entry_singular", "1 Eintrag"),
            ("entry_plural", "{count} Eintr\u{00e4}ge"),
            ("month_1", "Januar"), ("month_2", "Februar"), ("month_3", "M\u{00e4}rz"),
            ("month_4", "April"), ("month_5", "Mai"), ("month_6", "Juni"),
            ("month_7", "Juli"), ("month_8", "August"), ("month_9", "September"),
            ("month_10", "Oktober"), ("month_11", "November"), ("month_12", "Dezember"),
            ("reopen_auto_approved_title", "Woche zur Bearbeitung freigegeben"),
            ("reopen_auto_approved_body", "Die Woche ab {week_start} wurde wieder zur Bearbeitung freigegeben ({entry_count})."),
            ("reopen_auto_approved_notice_title", "Wochenfreigabe automatisch genehmigt"),
            ("reopen_auto_approved_notice_body", "Die Wiederfreigabe von {requester_name} f\u{00fc}r die Woche ab {week_start} wurde automatisch genehmigt ({entry_count})."),
            ("reopen_request_created_title", "Neue Anfrage zur Wochenfreigabe"),
            ("reopen_request_created_body", "{requester_name} m\u{00f6}chte die Woche ab {week_start} wieder bearbeiten."),
            ("reopen_approved_title", "Wochenfreigabe genehmigt"),
            ("reopen_approved_body", "Ihre Woche ab {week_start} wurde zur Bearbeitung freigegeben."),
            ("reopen_approved_by_admin_title", "Wochenfreigabe durch Admin genehmigt"),
            ("reopen_approved_by_admin_body", "Die Wiederfreigabe-Anfrage von {requester_name} f\u{00fc}r die Woche ab {week_start} wurde von einem Admin genehmigt."),
            ("reopen_rejected_title", "Wochenfreigabe abgelehnt"),
            ("reopen_rejected_body", "Ihre Anfrage zur Bearbeitung der Woche ab {week_start} wurde abgelehnt: {reason}"),
            ("reopen_rejected_by_admin_title", "Wochenfreigabe durch Admin abgelehnt"),
            ("reopen_rejected_by_admin_body", "Die Wiederfreigabe-Anfrage von {requester_name} f\u{00fc}r die Woche ab {week_start} wurde von einem Admin abgelehnt: {reason}"),
            ("absence_requested_title", "Neue Abwesenheitsanfrage"),
            ("absence_requested_body", "{requester_name} hat eine Abwesenheit von {start_date} bis {end_date} beantragt."),
            ("absence_approved_title", "Abwesenheit genehmigt"),
            ("absence_approved_body", "Ihre Abwesenheit ({start_date} bis {end_date}) wurde genehmigt."),
            ("absence_rejected_title", "Abwesenheit abgelehnt"),
            ("absence_rejected_body", "Ihre Abwesenheit ({start_date} bis {end_date}) wurde abgelehnt: {reason}"),
            ("absence_revoked_title", "Abwesenheit widerrufen"),
            ("absence_revoked_body", "Ihre Abwesenheit ({start_date} bis {end_date}) wurde von einem Administrator widerrufen."),
            ("change_request_created_title", "Neue Änderungsanfrage"),
            ("change_request_created_body", "{requester_name} hat eine Änderung fuer den Zeiteintrag am {entry_date} beantragt."),
            ("timesheet_submitted_title", "{submitter_name} hat eine Zeiterfassung eingereicht"),
            ("timesheet_submitted_body", "{entry_count} zur Genehmigung eingereicht"),
            ("timesheet_approved_title", "Zeiterfassung genehmigt"),
            ("timesheet_approved_body", "Ihr Zeiterfassungseintrag f\u{00fc}r {entry_date} wurde genehmigt."),
            ("timesheet_rejected_title", "Zeiterfassung abgelehnt"),
            ("timesheet_rejected_body", "Ihr Zeiterfassungseintrag f\u{00fc}r {entry_date} wurde abgelehnt: {reason}"),
            ("submission_reminder_title", "Arbeitszeiten noch nicht eingereicht"),
            ("submission_reminder_body", "Sie haben noch nicht eingereichte Arbeitszeiten f\u{00fc}r folgende Monate: {months}"),
            ("submission_reminder_email_body", "Hallo,\n\nf\u{00fc}r folgende Monate wurden Ihre Arbeitszeiten noch nicht eingereicht:\n\n{months}\n\nBitte melden Sie sich an und reichen Sie Ihre Zeiten ein:\n{app_url}\n"),
        ],
    },
];

// -- Lazy index for O(1) lookup by language code ------------------------------

struct LangIndex {
    by_code: HashMap<&'static str, usize>,
}

static INDEX: LazyLock<LangIndex> = LazyLock::new(|| {
    let mut by_code = HashMap::new();
    for (i, lang) in LANGUAGES.iter().enumerate() {
        by_code.insert(lang.code, i);
    }
    LangIndex { by_code }
});

fn lang_def(language: &Language) -> &'static LangDef {
    &LANGUAGES[language.0]
}

// -- Public Language handle ---------------------------------------------------

/// Opaque handle to a supported language. Wraps an index into `LANGUAGES`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Language(usize);

impl Default for Language {
    fn default() -> Self {
        Self(INDEX.by_code[DEFAULT_LANGUAGE_CODE])
    }
}

impl Language {
    pub fn from_setting(value: &str) -> Self {
        let normalized = value.trim().to_ascii_lowercase();
        INDEX
            .by_code
            .get(normalized.as_str())
            .map(|&i| Self(i))
            .unwrap_or_default()
    }

    pub fn code(self) -> &'static str {
        lang_def(&self).code
    }

    pub fn name(self) -> &'static str {
        lang_def(&self).name
    }
}


// -- Validation ---------------------------------------------------------------

/// Validates and normalises a language code string. Accepts any well-formed
/// BCP 47-like code (2-3 letter primary subtag, optional subtags separated
/// by hyphens). Returns the lowercased code, or `None` when invalid.
pub fn normalize_language_code(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() < 2 {
        return None;
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let primary = trimmed.split('-').next().unwrap_or("");
    if primary.len() < 2
        || primary.len() > 3
        || !primary.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

// -- Database -----------------------------------------------------------------

pub async fn load_ui_language(pool: &DatabasePool) -> Result<Language, sqlx::Error> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
        .bind(UI_LANGUAGE_KEY)
        .fetch_optional(pool)
        .await?;

    Ok(value
        .as_deref()
        .map(Language::from_setting)
        .unwrap_or_default())
}

// -- Formatting helpers -------------------------------------------------------

pub fn format_date(language: &Language, date: chrono::NaiveDate) -> String {
    date.format(lang_def(language).date_format).to_string()
}

pub fn format_month(language: &Language, year: i32, month: u32) -> String {
    let key = format!("month_{month}");
    let name = translate(language, &key, &[]);
    if name == key {
        format!("{year}-{month:02}")
    } else {
        format!("{name} {year}")
    }
}

pub fn entry_count(language: &Language, count: i64) -> String {
    if count == 1 {
        translate(language, "entry_singular", &[])
    } else {
        translate(language, "entry_plural", &[("count", count.to_string())])
    }
}

/// Prefer `local_name` when available; fall back to the English `name`.
pub fn holiday_display_name(
    _language: &Language,
    name: String,
    local_name: Option<String>,
) -> String {
    local_name
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(name)
}

// -- Translation lookup -------------------------------------------------------

pub fn translate(language: &Language, key: &str, params: &[(&str, String)]) -> String {
    let def = lang_def(language);
    let template = def
        .translations
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(key);
    render(template, params)
}

fn render(template: &str, params: &[(&str, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in params {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_language_codes_without_enumerating_supported_languages() {
        assert_eq!(normalize_language_code("de"), Some("de".to_string()));
        assert_eq!(normalize_language_code("pt-BR"), Some("pt-br".to_string()));
        assert_eq!(
            normalize_language_code("zh-Hant"),
            Some("zh-hant".to_string())
        );
    }

    #[test]
    fn rejects_invalid_language_codes() {
        assert_eq!(normalize_language_code(""), None);
        assert_eq!(normalize_language_code("english"), None);
        assert_eq!(normalize_language_code("en_US"), None);
        assert_eq!(normalize_language_code("e"), None);
    }

    #[test]
    fn translates_with_parameters() {
        let language = Language::from_setting("de");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        let text = translate(
            &language,
            "reopen_auto_approved_body",
            &[
                ("week_start", format_date(&language, date)),
                ("entry_count", entry_count(&language, 1)),
            ],
        );

        assert_eq!(
            text,
            "Die Woche ab 27.04.2026 wurde wieder zur Bearbeitung freigegeben (1 Eintrag)."
        );
    }

    #[test]
    fn format_date_english() {
        let language = Language::from_setting("en");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(format_date(&language, date), "04/27/2026");
    }

    #[test]
    fn format_date_german() {
        let language = Language::from_setting("de");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(format_date(&language, date), "27.04.2026");
    }

    #[test]
    fn defaults_unknown_backend_template_language_to_english() {
        let language = Language::from_setting("pt-BR");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(format_date(&language, date), "04/27/2026");
        assert_eq!(entry_count(&language, 2), "2 entries");
    }

    #[test]
    fn format_month_english() {
        let language = Language::from_setting("en");
        assert_eq!(format_month(&language, 2026, 3), "March 2026");
    }

    #[test]
    fn format_month_german() {
        let language = Language::from_setting("de");
        assert_eq!(format_month(&language, 2026, 3), "M\u{00e4}rz 2026");
    }

    #[test]
    fn holiday_name_uses_local_names_for_non_english_languages() {
        let language = Language::from_setting("de");
        assert_eq!(
            holiday_display_name(
                &language,
                "Labor Day".to_string(),
                Some("Tag der Arbeit".into())
            ),
            "Tag der Arbeit"
        );
    }
}
