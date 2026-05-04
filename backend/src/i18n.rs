//! Backend translations for server-generated messages.

use crate::db::DatabasePool;

const UI_LANGUAGE_KEY: &str = "ui_language";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    En,
    De,
}

impl Default for Language {
    fn default() -> Self {
        Self::En
    }
}

impl Language {
    fn from_setting(value: &str) -> Self {
        match value.trim() {
            "de" => Self::De,
            _ => Self::En,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextKey {
    ReopenAutoApprovedTitle,
    ReopenAutoApprovedBody,
    ReopenAutoApprovedNoticeTitle,
    ReopenAutoApprovedNoticeBody,
    ReopenRequestCreatedTitle,
    ReopenRequestCreatedBody,
    ReopenApprovedTitle,
    ReopenApprovedBody,
    ReopenApprovedByAdminTitle,
    ReopenApprovedByAdminBody,
    ReopenRejectedTitle,
    ReopenRejectedBody,
    ReopenRejectedByAdminTitle,
    ReopenRejectedByAdminBody,
}

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

pub fn entry_count(language: Language, count: i64) -> String {
    match (language, count) {
        (Language::De, 1) => "1 Eintrag".to_string(),
        (Language::De, _) => format!("{count} Einträge"),
        (Language::En, 1) => "1 entry".to_string(),
        (Language::En, _) => format!("{count} entries"),
    }
}

pub fn translate(language: Language, key: TextKey, params: &[(&str, String)]) -> String {
    render(template(language, key), params)
}

fn template(language: Language, key: TextKey) -> &'static str {
    match language {
        Language::En => match key {
            TextKey::ReopenAutoApprovedTitle => "Week reopened for editing",
            TextKey::ReopenAutoApprovedBody => {
                "The week starting {week_start} was reopened for editing ({entry_count})."
            }
            TextKey::ReopenAutoApprovedNoticeTitle => "Week reopen auto-approved",
            TextKey::ReopenAutoApprovedNoticeBody => {
                "{requester_name}'s week reopen for the week starting {week_start} was auto-approved ({entry_count})."
            }
            TextKey::ReopenRequestCreatedTitle => "New week reopen request",
            TextKey::ReopenRequestCreatedBody => {
                "{requester_name} wants to edit the week starting {week_start}."
            }
            TextKey::ReopenApprovedTitle => "Week reopen approved",
            TextKey::ReopenApprovedBody => {
                "Your week starting {week_start} was reopened for editing."
            }
            TextKey::ReopenApprovedByAdminTitle => "Week reopen approved by admin",
            TextKey::ReopenApprovedByAdminBody => {
                "The week reopen request from {requester_name} for the week starting {week_start} was approved by an admin."
            }
            TextKey::ReopenRejectedTitle => "Week reopen rejected",
            TextKey::ReopenRejectedBody => {
                "Your request to edit the week starting {week_start} was rejected: {reason}"
            }
            TextKey::ReopenRejectedByAdminTitle => "Week reopen rejected by admin",
            TextKey::ReopenRejectedByAdminBody => {
                "The week reopen request from {requester_name} for the week starting {week_start} was rejected by an admin: {reason}"
            }
        },
        Language::De => match key {
            TextKey::ReopenAutoApprovedTitle => "Woche zur Bearbeitung freigegeben",
            TextKey::ReopenAutoApprovedBody => {
                "Die Woche ab {week_start} wurde wieder zur Bearbeitung freigegeben ({entry_count})."
            }
            TextKey::ReopenAutoApprovedNoticeTitle => {
                "Wochenfreigabe automatisch genehmigt"
            }
            TextKey::ReopenAutoApprovedNoticeBody => {
                "Die Wiederfreigabe von {requester_name} für die Woche ab {week_start} wurde automatisch genehmigt ({entry_count})."
            }
            TextKey::ReopenRequestCreatedTitle => "Neue Anfrage zur Wochenfreigabe",
            TextKey::ReopenRequestCreatedBody => {
                "{requester_name} möchte die Woche ab {week_start} wieder bearbeiten."
            }
            TextKey::ReopenApprovedTitle => "Wochenfreigabe genehmigt",
            TextKey::ReopenApprovedBody => {
                "Ihre Woche ab {week_start} wurde zur Bearbeitung freigegeben."
            }
            TextKey::ReopenApprovedByAdminTitle => {
                "Wochenfreigabe durch Admin genehmigt"
            }
            TextKey::ReopenApprovedByAdminBody => {
                "Die Wiederfreigabe-Anfrage von {requester_name} für die Woche ab {week_start} wurde von einem Admin genehmigt."
            }
            TextKey::ReopenRejectedTitle => "Wochenfreigabe abgelehnt",
            TextKey::ReopenRejectedBody => {
                "Ihre Anfrage zur Bearbeitung der Woche ab {week_start} wurde abgelehnt: {reason}"
            }
            TextKey::ReopenRejectedByAdminTitle => {
                "Wochenfreigabe durch Admin abgelehnt"
            }
            TextKey::ReopenRejectedByAdminBody => {
                "Die Wiederfreigabe-Anfrage von {requester_name} für die Woche ab {week_start} wurde von einem Admin abgelehnt: {reason}"
            }
        },
    }
}

fn render(template: &str, params: &[(&str, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in params {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_with_parameters() {
        let text = translate(
            Language::De,
            TextKey::ReopenAutoApprovedBody,
            &[
                ("week_start", "2026-04-27".to_string()),
                ("entry_count", entry_count(Language::De, 1)),
            ],
        );

        assert_eq!(
            text,
            "Die Woche ab 2026-04-27 wurde wieder zur Bearbeitung freigegeben (1 Eintrag)."
        );
    }

    #[test]
    fn defaults_plural_entry_count_to_english() {
        assert_eq!(entry_count(Language::En, 2), "2 entries");
    }
}
