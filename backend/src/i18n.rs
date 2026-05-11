//! Backend translations for server-rendered messages.
//!
//! All language-specific data lives in the `LANGUAGES` table below.
//! To add a new language, append one entry to `LANGUAGES` -- no other
//! constants, functions, or enum variants need to change.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::db::DatabasePool;
use chrono::Datelike;

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
            ("week_singular", "1 week"),
            ("week_plural", "{count} weeks"),
            ("month_1", "January"), ("month_2", "February"), ("month_3", "March"),
            ("month_4", "April"), ("month_5", "May"), ("month_6", "June"),
            ("month_7", "July"), ("month_8", "August"), ("month_9", "September"),
            ("month_10", "October"), ("month_11", "November"), ("month_12", "December"),
            ("reopen_auto_approved_title", "Week reopened for editing"),
            ("reopen_auto_approved_body", "Your week has been reopened for editing automatically.\n\nWeek: {week_label}\n\nOpen change requests in this week were applied as part of the reopen process:\n{change_request_overview}\n\nPlease review your entries and submit the week again once everything is correct."),
            ("reopen_auto_approved_notice_title", "Week reopen auto-approved"),
            ("reopen_auto_approved_notice_body", "{requester_name}'s week reopen was auto-approved.\n\nWeek: {week_label}\n\nOpen change requests in that week were applied automatically:\n{change_request_overview}\n\nThis message is informational so approvers have full visibility."),
            ("reopen_request_created_title", "New week reopen request"),
            ("reopen_request_created_body", "{requester_name} requested to reopen a week.\n\nWeek: {week_label}\n\nPlease review the request and decide whether the week should be reopened.\nThe following change requests are currently open for this week:\n{change_request_overview}"),
            ("reopen_approved_title", "Week reopen approved"),
            ("reopen_approved_body", "Your week reopen request was approved.\n\nWeek: {week_label}\n\nAny open change requests for this week were applied automatically:\n{change_request_overview}\n\nYou can now edit your entries again. Please submit the week once your updates are complete."),
            ("reopen_approved_by_admin_title", "Week reopen approved by admin"),
            ("reopen_approved_by_admin_body", "The week reopen request from {requester_name} was approved by an admin.\n\nWeek: {week_label}\n\nThe following open change requests were applied automatically during approval:\n{change_request_overview}"),
            ("reopen_rejected_title", "Week reopen rejected"),
            ("reopen_rejected_body", "Your week reopen request was rejected.\n\nWeek: {week_label}\nReason: {reason}\n\nYou can review your submitted entries and create a new reopen request later if needed."),
            ("reopen_rejected_by_admin_title", "Week reopen rejected by admin"),
            ("reopen_rejected_by_admin_body", "The week reopen request from {requester_name} was rejected by an admin.\n\nWeek: {week_label}\nReason: {reason}\n\nThis message is informational so assigned approvers know the request was resolved."),
            ("absence_kind_vacation", "Vacation"),
            ("absence_kind_sick", "Sick"),
            ("absence_kind_training", "Training"),
            ("absence_kind_special_leave", "Special leave"),
            ("absence_kind_unpaid", "Unpaid"),
            ("absence_kind_general_absence", "General absence"),
            ("absence_kind_flextime_reduction", "Flextime Reduction"),
            ("absence_requested_title", "New absence request"),
            ("absence_requested_body", "{requester_name} requested a {kind} absence.\n\nPeriod: {start_date} to {end_date}\n\nPlease review the request and decide whether to approve or reject it."),
            ("absence_updated_title", "Absence request updated"),
            ("absence_updated_body", "{requester_name} updated their {kind} absence request.\n\nUpdated period: {start_date} to {end_date}\n\nPlease review the updated request before making a decision."),
            ("absence_approved_title", "Absence approved"),
            ("absence_approved_body", "Your {kind} absence has been approved.\n\nPeriod: {start_date} to {end_date}\n\nNo further action is required."),
            ("absence_rejected_title", "Absence rejected"),
            ("absence_rejected_body", "Your {kind} absence request was rejected.\n\nPeriod: {start_date} to {end_date}\nReason: {reason}\n\nPlease adjust your request and submit it again if needed."),
            ("absence_revoked_title", "Absence revoked"),
            ("absence_revoked_body", "Your {kind} absence was revoked by an administrator.\n\nPeriod: {start_date} to {end_date}\n\nPlease contact your approver if you need clarification."),
            ("absence_cancelled_title", "Absence request withdrawn"),
            ("absence_cancelled_body", "{requester_name} withdrew their {kind} absence request.\n\nPeriod: {start_date} to {end_date}\n\nThis message is informational only."),
            ("absence_cancellation_requested_title", "Absence cancellation requested"),
            ("absence_cancellation_requested_body", "{requester_name} requested cancellation of their {kind} absence.\n\nPeriod: {start_date} to {end_date}\n\nPlease review and decide whether to approve the cancellation."),
            ("absence_cancellation_approved_title", "Absence cancellation approved"),
            ("absence_cancellation_approved_body", "Your cancellation request was approved.\n\nAbsence: {kind}\nPeriod: {start_date} to {end_date}\n\nThe absence has been cancelled."),
            ("absence_cancellation_rejected_title", "Absence cancellation rejected"),
            ("absence_cancellation_rejected_body", "Your cancellation request was rejected.\n\nAbsence: {kind}\nPeriod: {start_date} to {end_date}\n\nThe absence remains approved."),
            ("change_request_created_title", "New change request"),
            ("change_request_created_body", "{requester_name} requested a time-entry change.\n\nWeek: {week_label}\nEntry: {entry_label}\nReason: {reason}\n\nRequested changes:\n{change_diff}\n\nPlease review this request and decide whether to approve or reject it."),
            ("change_request_approved_title", "Change request approved"),
            ("change_request_approved_body", "Your time-entry change request was approved.\n\nWeek: {week_label}\nEntry: {entry_label}\n\nApplied changes:\n{change_diff}\n\nPlease verify the updated entry in your week overview."),
            ("change_request_rejected_title", "Change request rejected"),
            ("change_request_rejected_body", "Your time-entry change request was rejected.\n\nWeek: {week_label}\nEntry: {entry_label}\nReason: {reason}\n\nRequested changes:\n{change_diff}\n\nYou can submit a revised change request if needed."),
            ("timesheet_submitted_title", "{submitter_name} submitted a timesheet"),
            ("timesheet_submitted_body", "A timesheet was submitted for approval.\n\nScope: {week_count}\n\nPlease review the submitted entries in the approval dashboard."),
            ("timesheet_approved_title", "Timesheet approved"),
            ("timesheet_approved_body", "Your timesheet was approved.\n\nAffected week includes: {entry_date}\n\nNo further action is required."),
            ("timesheet_batch_approved_body", "Your timesheets were approved in batch.\n\nScope: {week_count}\n\nPlease review your dashboard for details."),
            ("timesheet_rejected_title", "Timesheet rejected"),
            ("timesheet_rejected_body", "Your timesheet was rejected.\n\nAffected week includes: {entry_date}\nReason: {reason}\n\nPlease update your entries and submit the week again."),
            ("submission_reminder_title", "Weeks not yet submitted"),
            ("submission_reminder_body", "You still have weeks that are not submitted.\n\nMonths: {months}\n\nPlease submit the missing weeks as soon as possible."),
            ("submission_reminder_email_body", "Hello,\n\nyou still have weeks not submitted for the following months:\n\n{months}\n\nPlease log in and submit your weeks:\n{app_url}\n"),
            ("approval_reminder_title", "Pending approvals"),
            ("approval_reminder_body", "You have pending requests awaiting your approval.\n\nOpen items: {count}\n\nPlease review them in the dashboard."),
            ("approval_reminder_email_body", "Hello,\n\nyou have {count} pending request(s) awaiting your approval.\n\nPlease log in to review them:\n{app_url}\n"),
            ("password_reset_subject", "Reset your password"),
            ("password_reset_body", "Hello,\n\nYou requested a password reset.\n\nPlease click the link below (valid for 1 hour):\n\n{reset_link}\n\nIf you did not request this, you can safely ignore this email."),
            ("account_created_subject", "Welcome to Zerf"),
            ("account_created_body", "Hello {first_name} {last_name},\n\nYour account has been created.\n\nEmail:    {email}\nPassword: {password}{login_line}\nPlease log in and change your password immediately."),
        ],
    },
    LangDef {
        code: "de",
        name: "Deutsch",
        date_format: "%d.%m.%Y",
        translations: &[
            ("week_singular", "1 Woche"),
            ("week_plural", "{count} Wochen"),
            ("month_1", "Januar"), ("month_2", "Februar"), ("month_3", "M\u{00e4}rz"),
            ("month_4", "April"), ("month_5", "Mai"), ("month_6", "Juni"),
            ("month_7", "Juli"), ("month_8", "August"), ("month_9", "September"),
            ("month_10", "Oktober"), ("month_11", "November"), ("month_12", "Dezember"),
            ("reopen_auto_approved_title", "Woche zur Bearbeitung freigegeben"),
            ("reopen_auto_approved_body", "Ihre Woche wurde automatisch wieder zur Bearbeitung freigegeben.\n\nWoche: {week_label}\n\nOffene \u{00c4}nderungsantr\u{00e4}ge in dieser Woche wurden im Zuge der Wiederfreigabe direkt \u{00fc}bernommen:\n{change_request_overview}\n\nBitte pr\u{00fc}fen Sie Ihre Eintr\u{00e4}ge und reichen Sie die Woche danach erneut ein."),
            ("reopen_auto_approved_notice_title", "Wochenfreigabe automatisch genehmigt"),
            ("reopen_auto_approved_notice_body", "Die Wiederfreigabe von {requester_name} wurde automatisch genehmigt.\n\nWoche: {week_label}\n\nOffene \u{00c4}nderungsantr\u{00e4}ge in dieser Woche wurden automatisch \u{00fc}bernommen:\n{change_request_overview}\n\nDiese Nachricht dient zur Information der genehmigenden Personen."),
            ("reopen_request_created_title", "Neue Anfrage zur Wochenfreigabe"),
            ("reopen_request_created_body", "{requester_name} m\u{00f6}chte eine Woche wieder bearbeiten.\n\nWoche: {week_label}\n\nBitte pr\u{00fc}fen Sie die Anfrage und entscheiden Sie \u{00fc}ber die Wiederfreigabe.\nFolgende \u{00c4}nderungsantr\u{00e4}ge sind f\u{00fc}r diese Woche aktuell offen:\n{change_request_overview}"),
            ("reopen_approved_title", "Wochenfreigabe genehmigt"),
            ("reopen_approved_body", "Ihre Anfrage zur Wochenfreigabe wurde genehmigt.\n\nWoche: {week_label}\n\nOffene \u{00c4}nderungsantr\u{00e4}ge f\u{00fc}r diese Woche wurden automatisch \u{00fc}bernommen:\n{change_request_overview}\n\nSie k\u{00f6}nnen Ihre Eintr\u{00e4}ge jetzt wieder bearbeiten. Bitte reichen Sie die Woche danach erneut ein."),
            ("reopen_approved_by_admin_title", "Wochenfreigabe durch Admin genehmigt"),
            ("reopen_approved_by_admin_body", "Die Wiederfreigabe-Anfrage von {requester_name} wurde von einem Admin genehmigt.\n\nWoche: {week_label}\n\nFolgende offene \u{00c4}nderungsantr\u{00e4}ge wurden bei der Genehmigung automatisch \u{00fc}bernommen:\n{change_request_overview}"),
            ("reopen_rejected_title", "Wochenfreigabe abgelehnt"),
            ("reopen_rejected_body", "Ihre Anfrage zur Wochenfreigabe wurde abgelehnt.\n\nWoche: {week_label}\nGrund: {reason}\n\nSie k\u{00f6}nnen die Eintr\u{00e4}ge pr\u{00fc}fen und bei Bedarf sp\u{00e4}ter eine neue Anfrage stellen."),
            ("reopen_rejected_by_admin_title", "Wochenfreigabe durch Admin abgelehnt"),
            ("reopen_rejected_by_admin_body", "Die Wiederfreigabe-Anfrage von {requester_name} wurde von einem Admin abgelehnt.\n\nWoche: {week_label}\nGrund: {reason}\n\nDiese Nachricht dient zur Information, damit zugewiesene Approver den Abschluss sehen."),
            ("absence_kind_vacation", "Urlaub"),
            ("absence_kind_sick", "Krankmeldung"),
            ("absence_kind_training", "Fortbildung"),
            ("absence_kind_special_leave", "Sonderurlaub"),
            ("absence_kind_unpaid", "Unbezahlter Urlaub"),
            ("absence_kind_general_absence", "Allgemeine Abwesenheit"),
            ("absence_kind_flextime_reduction", "Gleitzeitabbau"),
            ("absence_requested_title", "Neue Abwesenheitsanfrage"),
            ("absence_requested_body", "{requester_name} hat eine Abwesenheit vom Typ {kind} beantragt.\n\nZeitraum: {start_date} bis {end_date}\n\nBitte pr\u{00fc}fen Sie die Anfrage und entscheiden Sie \u{00fc}ber Genehmigung oder Ablehnung."),
            ("absence_updated_title", "Abwesenheitsanfrage aktualisiert"),
            ("absence_updated_body", "{requester_name} hat die {kind}-Anfrage aktualisiert.\n\nAktualisierter Zeitraum: {start_date} bis {end_date}\n\nBitte ber\u{00fc}cksichtigen Sie die Aktualisierung bei Ihrer Entscheidung."),
            ("absence_approved_title", "Abwesenheit genehmigt"),
            ("absence_approved_body", "Ihre Abwesenheit vom Typ {kind} wurde genehmigt.\n\nZeitraum: {start_date} bis {end_date}\n\nEs ist keine weitere Aktion erforderlich."),
            ("absence_rejected_title", "Abwesenheit abgelehnt"),
            ("absence_rejected_body", "Ihre Abwesenheitsanfrage vom Typ {kind} wurde abgelehnt.\n\nZeitraum: {start_date} bis {end_date}\nGrund: {reason}\n\nSie k\u{00f6}nnen die Anfrage bei Bedarf angepasst erneut stellen."),
            ("absence_revoked_title", "Abwesenheit widerrufen"),
            ("absence_revoked_body", "Ihre Abwesenheit vom Typ {kind} wurde von einem Administrator widerrufen.\n\nZeitraum: {start_date} bis {end_date}\n\nBitte wenden Sie sich bei R\u{00fc}ckfragen an Ihre Genehmiger."),
            ("absence_cancelled_title", "Abwesenheitsantrag zur\u{00fc}ckgezogen"),
            ("absence_cancelled_body", "{requester_name} hat den Antrag auf {kind} zur\u{00fc}ckgezogen.\n\nZeitraum: {start_date} bis {end_date}\n\nDiese Nachricht dient nur der Information."),
            ("absence_cancellation_requested_title", "Stornierungsanfrage f\u{00fc}r Abwesenheit"),
            ("absence_cancellation_requested_body", "{requester_name} m\u{00f6}chte eine genehmigte Abwesenheit vom Typ {kind} stornieren.\n\nZeitraum: {start_date} bis {end_date}\n\nBitte pr\u{00fc}fen Sie die Stornierungsanfrage und entscheiden Sie \u{00fc}ber Genehmigung oder Ablehnung."),
            ("absence_cancellation_approved_title", "Stornierung genehmigt"),
            ("absence_cancellation_approved_body", "Die Stornierung Ihrer Abwesenheit wurde genehmigt.\n\nTyp: {kind}\nZeitraum: {start_date} bis {end_date}\n\nDie Abwesenheit wurde damit storniert."),
            ("absence_cancellation_rejected_title", "Stornierung abgelehnt"),
            ("absence_cancellation_rejected_body", "Die Stornierung Ihrer Abwesenheit wurde abgelehnt.\n\nTyp: {kind}\nZeitraum: {start_date} bis {end_date}\n\nDie Abwesenheit bleibt weiterhin genehmigt."),
            ("change_request_created_title", "Neue \u{00c4}nderungsanfrage"),
            ("change_request_created_body", "{requester_name} hat eine \u{00c4}nderung einer Zeitbuchung beantragt.\n\nWoche: {week_label}\nEintrag: {entry_label}\nBegr\u{00fc}ndung: {reason}\n\nBeantragte \u{00c4}nderungen:\n{change_diff}\n\nBitte pr\u{00fc}fen Sie die Anfrage und entscheiden Sie \u{00fc}ber Genehmigung oder Ablehnung."),
            ("change_request_approved_title", "\u{00c4}nderungsanfrage genehmigt"),
            ("change_request_approved_body", "Ihre \u{00c4}nderungsanfrage wurde genehmigt.\n\nWoche: {week_label}\nEintrag: {entry_label}\n\n\u{00dc}bernommene \u{00c4}nderungen:\n{change_diff}\n\nBitte pr\u{00fc}fen Sie den aktualisierten Eintrag in Ihrer Wochen\u{00fc}bersicht."),
            ("change_request_rejected_title", "\u{00c4}nderungsanfrage abgelehnt"),
            ("change_request_rejected_body", "Ihre \u{00c4}nderungsanfrage wurde abgelehnt.\n\nWoche: {week_label}\nEintrag: {entry_label}\nGrund: {reason}\n\nBeantragte \u{00c4}nderungen:\n{change_diff}\n\nSie k\u{00f6}nnen bei Bedarf eine angepasste Anfrage erneut stellen."),
            ("timesheet_submitted_title", "{submitter_name} hat eine Zeiterfassung eingereicht"),
            ("timesheet_submitted_body", "Eine Zeiterfassung wurde zur Genehmigung eingereicht.\n\nUmfang: {week_count}\n\nBitte pr\u{00fc}fen Sie die eingereichten Eintr\u{00e4}ge im Dashboard."),
            ("timesheet_approved_title", "Zeiterfassung genehmigt"),
            ("timesheet_approved_body", "Ihre Zeiterfassung wurde genehmigt.\n\nBetroffene Woche enth\u{00e4}lt: {entry_date}\n\nEs ist keine weitere Aktion erforderlich."),
            ("timesheet_batch_approved_body", "Ihre Zeiterfassungen wurden gesammelt genehmigt.\n\nUmfang: {week_count}\n\nBitte pr\u{00fc}fen Sie bei Bedarf die Details im Dashboard."),
            ("timesheet_rejected_title", "Zeiterfassung abgelehnt"),
            ("timesheet_rejected_body", "Ihre Zeiterfassung wurde abgelehnt.\n\nBetroffene Woche enth\u{00e4}lt: {entry_date}\nGrund: {reason}\n\nBitte passen Sie Ihre Eintr\u{00e4}ge an und reichen Sie die Woche erneut ein."),
            ("submission_reminder_title", "Arbeitszeiten noch nicht eingereicht"),
            ("submission_reminder_body", "Sie haben noch nicht eingereichte Wochen.\n\nMonate: {months}\n\nBitte reichen Sie die fehlenden Wochen zeitnah ein."),
            ("submission_reminder_email_body", "Hallo,\n\nf\u{00fc}r folgende Monate wurden Ihre Wochen noch nicht eingereicht:\n\n{months}\n\nBitte melden Sie sich an und reichen Sie Ihre Wochen ein:\n{app_url}\n"),
            ("approval_reminder_title", "Offene Genehmigungen"),
            ("approval_reminder_body", "Es gibt offene Anfragen, die Ihre Genehmigung erfordern.\n\nOffene Vorg\u{00e4}nge: {count}\n\nBitte pr\u{00fc}fen Sie diese im Dashboard."),
            ("approval_reminder_email_body", "Hallo,\n\nes gibt {count} Anfrage(n), die Ihre Genehmigung erfordern.\n\nBitte melden Sie sich an, um diese zu bearbeiten:\n{app_url}\n"),
            ("password_reset_subject", "Passwort zur\u{00fc}cksetzen"),
            ("password_reset_body", "Hallo,\n\nSie haben eine Anfrage zum Zur\u{00fc}cksetzen Ihres Passworts gestellt.\n\nBitte klicken Sie auf den folgenden Link (g\u{00fc}ltig f\u{00fc}r 1 Stunde):\n\n{reset_link}\n\nFalls Sie diese Anfrage nicht gestellt haben, k\u{00f6}nnen Sie diese E-Mail ignorieren."),
            ("account_created_subject", "Willkommen bei Zerf"),
            ("account_created_body", "Hallo {first_name} {last_name},\n\nIhr Konto wurde erstellt.\n\nE-Mail:   {email}\nPasswort: {password}{login_line}\nBitte melden Sie sich an und \u{00e4}ndern Sie Ihr Passwort umgehend."),
        ],
    },
];

// -- Lazy index for O(1) lookup by language code ------------------------------

struct LangIndex {
    by_code: HashMap<&'static str, usize>,
}

static INDEX: LazyLock<LangIndex> = LazyLock::new(|| {
    let mut language_index_by_code = HashMap::new();
    for (language_index, language_definition) in LANGUAGES.iter().enumerate() {
        language_index_by_code.insert(language_definition.code, language_index);
    }
    LangIndex {
        by_code: language_index_by_code,
    }
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
            .map(|&language_index| Self(language_index))
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
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    let primary = trimmed.split('-').next().unwrap_or("");
    if primary.len() < 2 || primary.len() > 3 || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

// -- Database -----------------------------------------------------------------

pub async fn load_ui_language(pool: &DatabasePool) -> Result<Language, crate::error::AppError> {
    let db = crate::repository::SettingsDb::new(pool.clone());
    let code = db.load_ui_language_code().await;
    Ok(Language::from_setting(&code))
}

// -- Formatting helpers -------------------------------------------------------

pub fn format_date(language: &Language, date: chrono::NaiveDate) -> String {
    date.format(lang_def(language).date_format).to_string()
}

pub fn format_datetime_in_timezone(
    language: &Language,
    value: chrono::DateTime<chrono::Utc>,
    timezone: &str,
) -> String {
    let tz = timezone
        .parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::UTC);
    let local = value.with_timezone(&tz);
    if language.code() == "de" {
        local.format("%d.%m.%Y %H:%M %Z").to_string()
    } else {
        local.format("%m/%d/%Y %H:%M %Z").to_string()
    }
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

pub fn week_count(language: &Language, count: i64) -> String {
    if count == 1 {
        translate(language, "week_singular", &[])
    } else {
        translate(language, "week_plural", &[("count", count.to_string())])
    }
}

pub fn format_week_label(language: &Language, week_start: chrono::NaiveDate) -> String {
    let week_end = week_start + chrono::Duration::days(6);
    let week = week_start.iso_week().week();
    if language.code() == "de" {
        format!(
            "KW {week} ({} bis {})",
            format_date(language, week_start),
            format_date(language, week_end)
        )
    } else {
        format!(
            "CW {week} ({} to {})",
            format_date(language, week_start),
            format_date(language, week_end)
        )
    }
}

pub fn work_category_label(language: &Language, category_name: &str) -> String {
    if language.code() != "de" {
        return category_name.to_string();
    }
    match category_name {
        "Core Duties" => "Kernaufgaben".to_string(),
        "Preparation Time" => "Vorbereitungszeit".to_string(),
        "Leadership Tasks" => "Leitungsaufgaben".to_string(),
        "Team Meeting" => "Teambesprechung".to_string(),
        "Training" => "Fortbildung".to_string(),
        "Other" => "Sonstiges".to_string(),
        "Flextime Reduction" => "Gleitzeitabbau".to_string(),
        other => other.to_string(),
    }
}

/// Returns the localised label for an absence kind identifier (e.g. `"sick"`).
/// Falls back to the raw kind string when no translation is available.
pub fn absence_kind_label(language: &Language, kind: &str) -> String {
    let key = format!("absence_kind_{kind}");
    translate(language, &key, &[])
}

/// Prefer `local_name` when available; fall back to the English `name`.
pub fn holiday_display_name(
    _language: &Language,
    name: String,
    local_name: Option<String>,
) -> String {
    local_name.filter(|v| !v.trim().is_empty()).unwrap_or(name)
}

// -- Translation lookup -------------------------------------------------------

pub fn translate(language: &Language, key: &str, params: &[(&str, String)]) -> String {
    let language_definition = lang_def(language);
    let template = language_definition
        .translations
        .iter()
        .find(|(translation_key, _)| *translation_key == key)
        .map(|(_, translation_value)| *translation_value)
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
        let week_label = format_week_label(&language, date);
        let text = translate(
            &language,
            "reopen_auto_approved_body",
            &[
                ("week_label", week_label.clone()),
                (
                    "change_request_overview",
                    "Keine offenen Änderungsanträge in dieser Woche.".to_string(),
                ),
            ],
        );

        assert!(text.contains(&week_label));
        assert!(text.contains("Keine offenen Änderungsanträge"));
    }

    #[test]
    fn password_reset_email_templates_are_translated() {
        let language = Language::from_setting("de");
        let subject = translate(&language, "password_reset_subject", &[]);
        let body = translate(
            &language,
            "password_reset_body",
            &[("reset_link", "https://zerf.example/reset".to_string())],
        );

        assert_eq!(subject, "Passwort zur\u{00fc}cksetzen");
        assert!(body.contains("https://zerf.example/reset"));
        assert!(body.contains("1 Stunde"));
    }

    #[test]
    fn account_created_email_template_uses_parameters() {
        let language = Language::from_setting("en");
        let body = translate(
            &language,
            "account_created_body",
            &[
                ("first_name", "Ada".to_string()),
                ("last_name", "Lovelace".to_string()),
                ("email", "ada@example.com".to_string()),
                ("password", "TempPass!234".to_string()),
                (
                    "login_line",
                    "\nURL:      https://zerf.example\n".to_string(),
                ),
            ],
        );

        assert!(body.contains("Hello Ada Lovelace"));
        assert!(body.contains("Email:    ada@example.com"));
        assert!(body.contains("Password: TempPass!234"));
        assert!(body.contains("URL:      https://zerf.example"));
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
        assert_eq!(week_count(&language, 2), "2 weeks");
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
