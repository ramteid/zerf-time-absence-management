import { writable, derived, get } from "svelte/store";

const STORAGE_KEY = "kitazeit.ui-language";
export const DEFAULT_LANGUAGE = "en";

export const LANGUAGES = Object.freeze({
  en: { label: "English", locale: "en-US" },
  de: { label: "Deutsch", locale: "de-DE" },
});

const TRANSLATIONS = {
  en: {
    hours_unit: "h",
    "{hours} / week": "{hours} / week",
    help_monthly_report:
      "Shows the monthly report for a team member with target/actual hours and daily details.",
    help_team_report:
      "Compares target and actual hours for all active team members in the selected month.",
    help_category_breakdown:
      "Shows how tracked hours are distributed across the different categories.",
    help_csv_export:
      "Exports the monthly report for the selected employee as a CSV file.",
    audit_table_users: "User",
    audit_table_absences: "Absence",
    audit_table_time_entries: "Time Entry",
    audit_table_categories: "Category",
    audit_table_holidays: "Holiday",
    audit_table_sessions: "Session",
    audit_table_notifications: "Notification",
    audit_table_app_settings: "Setting",
    audit_table_reopen_requests: "Reopen Request",
    audit_action_created: "Created",
    audit_action_updated: "Updated",
    audit_action_deleted: "Deleted",
    audit_action_approved: "Approved",
    audit_action_rejected: "Rejected",
    audit_action_cancelled: "Cancelled",
    audit_action_status_changed: "Status Changed",
    audit_action_team_settings_updated: "Team Setting Updated",
    "Public holiday": "Public holiday",
    Holiday: "Holiday",
    Work: "Work",
    "Work time": "Work time",
    Close: "Close",
    Absent: "Absent",
  },
  de: {
    "Loading...": "Wird geladen...",
    Error: "Fehler",
    Time: "Zeit",
    Absences: "Abwesenheiten",
    Calendar: "Kalender",
    Account: "Konto",
    Dashboard: "Dashboard",
    Reports: "Berichte",
    Admin: "Admin",
    More: "Mehr",
    "Sign out": "Abmelden",
    "Sign in": "Anmelden",
    "Sign in to your time-tracking workspace.":
      "Melden Sie sich in Ihrem Zeiterfassungsbereich an.",
    Email: "E-Mail",
    Password: "Passwort",
    "Page not found": "Seite nicht gefunden",
    Forbidden: "Kein Zugriff",
    Cancel: "Abbrechen",
    OK: "OK",
    Reason: "Begründung",
    "Reason required": "Begründung erforderlich",
    Save: "Speichern",
    Delete: "Löschen",
    Edit: "Bearbeiten",
    Add: "Hinzufügen",
    Submit: "Senden",
    Approve: "Genehmigen",
    Reject: "Ablehnen",
    Yes: "Ja",
    No: "Nein",
    Show: "Anzeigen",
    Run: "Starten",
    Date: "Datum",
    Start: "Beginn",
    End: "Ende",
    Category: "Kategorie",
    Comment: "Kommentar",
    "Comment (optional)": "Kommentar (optional)",
    Status: "Status",
    Action: "Aktion",
    Type: "Typ",
    From: "Von",
    To: "Bis",

    Name: "Name",
    Role: "Rolle",
    Hours: "Stunden",
    Leave: "Urlaub",
    Active: "Aktiv",
    Color: "Farbe",
    Description: "Beschreibung",
    Order: "Reihenfolge",
    "First name": "Vorname",
    "Last name": "Nachname",
    "Weekly hours": "Wochenstunden",
    "Annual leave days": "Urlaubstage pro Jahr",
    "Start date": "Startdatum",
    Settings: "Einstellungen",
    "Language settings": "Spracheinstellungen",
    "Interface language": "Oberflächensprache",
    "Missing translations fall back to English.":
      "Fehlende Übersetzungen fallen auf Englisch zurück.",
    "Language saved.": "Sprache gespeichert.",
    Employee: "Mitarbeitende",
    "Team lead": "Teamleitung",
    Users: "Benutzer",
    Categories: "Kategorien",
    Holidays: "Feiertage",
    "Audit log": "Audit-Protokoll",
    "Time tracking": "Zeiterfassung",
    "Previous week": "Vorherige Woche",
    "Next week": "Nächste Woche",
    "Week {week}: {from} - {to}": "Woche {week}: {from} - {to}",
    "Week {week}": "Woche {week}",

    "Copied {count} entries.": "{count} Einträge kopiert.",
    Target: "Soll",
    Actual: "Ist",
    Difference: "Differenz",
    "Add entry": "Eintrag hinzufügen",
    "Edit entry": "Eintrag bearbeiten",
    "Delete?": "Löschen?",
    "Delete this entry?": "Diesen Eintrag löschen?",
    "Request change": "Änderung anfordern",
    "Submit week ({count})": "Woche einreichen ({count})",
    "Week submitted.": "Woche eingereicht.",
    "Original: {date} {start}-{end}": "Original: {date} {start}-{end}",
    "Why is the change needed?": "Warum ist die Änderung nötig?",
    "Submit request": "Anfrage senden",
    "Change request submitted.": "Änderungsanfrage eingereicht.",
    "Annual entitlement": "Jahresanspruch",
    "Already taken": "Bereits genommen",
    "Approved upcoming": "Genehmigt bevorstehend",
    Requested: "Beantragt",
    Available: "Verfügbar",
    "Request vacation": "Urlaub beantragen",
    "Report sick": "Krank melden",
    Training: "Fortbildung",
    "Special leave": "Sonderurlaub",
    Unpaid: "Unbezahlt",
    "General absence": "Allgemeine Abwesenheit",
    "Cancel?": "Abbrechen?",
    "Cancel this request?": "Diese Anfrage abbrechen?",
    "Edit absence": "Abwesenheit bearbeiten",
    "Sick leave saved.": "Krankmeldung gespeichert.",
    "Request submitted.": "Anfrage eingereicht.",
    "Absence calendar": "Abwesenheitskalender",
    "Previous month": "Vorheriger Monat",
    "Next month": "Nächster Monat",
    Vacation: "Urlaub",
    Sick: "Krank",
    Holiday: "Feiertag",
    Work: "Arbeitszeit",
    "Work time": "Arbeitszeit",
    Copy: "Kopieren",
    "Copied!": "Kopiert!",
    Close: "Schließen",
    "My account": "Mein Konto",
    "Please change your password.": "Bitte ändern Sie Ihr Passwort.",
    "You are using a temporary password.":
      "Sie verwenden ein temporäres Passwort.",
    "Personal data": "Persönliche Daten",
    "Change password": "Passwort ändern",
    "Current password": "Aktuelles Passwort",
    "New password (min 12 chars)": "Neues Passwort (mind. 12 Zeichen)",
    "Confirm new password": "Neues Passwort bestätigen",
    "Passwords do not match.": "Passwörter stimmen nicht überein.",
    "Password changed.": "Passwort geändert.",
    "Overtime balance {year}": "Überstundenkonto {year}",
    Balance: "Saldo",
    Month: "Monat",
    Diff: "Diff.",
    Cumulative: "Kumuliert",
    "Submitted entries": "Eingereichte Einträge",
    "Open requests": "Offene Anträge",
    "Change requests": "Änderungsanträge",
    "Change Requests": "Änderungsanträge",
    "Submitted time entries": "Eingereichte Zeiteinträge",
    "No open entries.": "Keine offenen Einträge.",
    Approved: "Genehmigt",
    "Approved.": "Genehmigt.",
    "Approve all": "Alle genehmigen",
    "Open absence requests": "Offene Abwesenheitsanträge",
    "No open requests.": "Keine offenen Anträge.",
    "No open change requests.": "Keine offenen Änderungsanfragen.",
    "Change request": "Änderungsanfrage",
    "Reason: {reason}": "Begründung: {reason}",
    "New values: {date} {start}-{end}": "Neue Werte: {date} {start}-{end}",
    "Approve & apply": "Genehmigen und anwenden",
    "Monthly report": "Monatsbericht",
    "Export CSV": "CSV exportieren",
    Weekday: "Wochentag",
    Entries: "Einträge",
    Note: "Hinweis",
    "By category": "Nach Kategorie",
    "Team report": "Teambericht",
    "Category breakdown": "Kategorieauswertung",
    "No data.": "Keine Daten.",
    "Please change your temporary password.":
      "Bitte ändern Sie Ihr temporäres Passwort.",
    "New user": "Neuer Benutzer",
    "Edit user": "Benutzer bearbeiten",
    "New category": "Neue Kategorie",
    "Edit category": "Kategorie bearbeiten",
    "Add holiday": "Feiertag hinzufügen",
    "Date and name required": "Datum und Name sind erforderlich",
    "Reset password?": "Passwort zurücksetzen?",
    "A temporary password will be generated.":
      "Es wird ein temporäres Passwort erzeugt.",
    "Temporary password: {password}": "Temporäres Passwort: {password}",
    "User created. Temporary password: {password}":
      "Benutzer erstellt. Temporäres Passwort: {password}",
    "Reset PW": "PW zurücksetzen",
    "Deactivate?": "Deaktivieren?",
    Deactivate: "Deaktivieren",
    User: "Benutzer",
    Table: "Tabelle",
    Record: "Eintrag",
    Draft: "Entwurf",
    Submitted: "Eingereicht",
    Rejected: "Abgelehnt",
    Cancelled: "Storniert",
    Open: "Offen",
    Monday: "Montag",
    Tuesday: "Dienstag",
    Wednesday: "Mittwoch",
    Thursday: "Donnerstag",
    Friday: "Freitag",
    Saturday: "Samstag",
    Sunday: "Sonntag",
    // Redesign keys
    "Time Entry": "Zeiterfassung",
    contract: "Vertrag",
    Logged: "Erfasst",
    "of {target}h target": "von {target}h Soll",
    Overtime: "Überstunden",
    Remaining: "Verbleibend",
    Pending: "Ausstehend",
    Language: "Sprache",
    "this week": "diese Woche",
    "to target": "bis zum Soll",
    "Submit Week": "Woche einreichen",
    "Request Absence": "Abwesenheit beantragen",
    "Vacation, sick leave & training days":
      "Urlaub, Krankmeldung & Fortbildung",
    "Total Days": "Gesamttage",
    "Absence History": "Abwesenheitshistorie",
    "No absences yet.": "Noch keine Abwesenheiten.",
    "Absence cancelled.": "Abwesenheit storniert.",
    "Cancel this absence request?": "Diese Abwesenheitsanfrage stornieren?",
    "Approve timesheets & manage requests":
      "Stundenzettel genehmigen & Anträge verwalten",
    "Pending Timesheets": "Ausstehende Stundenzettel",
    "Absence Requests": "Abwesenheitsanträge",
    "Team Members": "Teammitglieder",
    "Timesheet Approvals": "Stundenzettel-Genehmigungen",
    "Approve All": "Alle genehmigen",
    "All caught up!": "Alles erledigt!",
    "No pending requests": "Keine ausstehenden Anträge",
    "Team hours overview": "Teamstunden-Übersicht",
    "Your profile & preferences": "Ihr Profil & Einstellungen",
    "Manage your team": "Team verwalten",
    "Add Member": "Mitglied hinzufügen",
    "Edit Member": "Mitglied bearbeiten",
    Inactive: "Inaktiv",
    "Deactivate this user?": "Diesen Benutzer deaktivieren?",
    "User deactivated.": "Benutzer deaktiviert.",
    "User updated.": "Benutzer aktualisiert.",
    "Time Categories": "Zeitkategorien",
    "Add Category": "Kategorie hinzufügen",
    "Edit Category": "Kategorie bearbeiten",
    "General Settings": "Allgemeine Einstellungen",
    General: "Allgemein",
    "Kita name": "Kita-Name",
    "Save Changes": "Änderungen speichern",
    "Saving...": "Speichert...",
    "Settings saved.": "Einstellungen gespeichert.",
    "Settings saved. Holidays have been refreshed.":
      "Einstellungen gespeichert. Feiertage wurden aktualisiert.",
    Country: "Land",
    Region: "Region",
    "e.g. US-CA": "z.B. US-CA",
    "Saving will re-fetch holidays from the Nager.Date API for the selected country and region.":
      "Beim Speichern werden die Feiertage über die Nager.Date-API für das gewählte Land und die Region neu abgerufen.",
    "Audit Log": "Audit-Protokoll",
    "Holiday name": "Feiertagsname",
    "Holiday added.": "Feiertag hinzugefügt.",
    "No holidays for {year}.": "Keine Feiertage für {year}.",
    "Delete this holiday?": "Diesen Feiertag löschen?",
    Lead: "Leitung",
    "Add Entry": "Eintrag hinzufügen",
    "Edit Entry": "Eintrag bearbeiten",
    "Edit Absence": "Abwesenheit bearbeiten",
    "Submit Request": "Anfrage senden",
    "Notes (optional)": "Anmerkungen (optional)",
    Entry: "Eintrag",
    Duration: "Dauer",
    Days: "Tage",
    Used: "Verbraucht",
    "awaiting approval": "Genehmigung ausstehend",
    pending: "ausstehend",
    open: "offen",
    "All approved.": "Alle genehmigt.",
    "Reject?": "Ablehnen?",
    "Reject this entry?": "Diesen Eintrag ablehnen?",
    "Reject this request?": "Diese Anfrage ablehnen?",
    "Reject this change request?": "Diese Änderungsanfrage ablehnen?",
    Request: "Anfrage",
    // Default category names
    "Direct Childcare": "Arbeit am Kind",
    "Preparation Time": "Vorbereitungszeit",
    "Leadership Tasks": "Leitungsaufgaben",
    "Team Meeting": "Teambesprechung",
    Other: "Sonstiges",
    "Switch to dark mode": "Dunkelmodus aktivieren",
    "Switch to light mode": "Hellmodus aktivieren",
    // Reopen-week feature
    "Approver (Team lead / Admin)": "Verantwortliche Teamleitung / Admin",
    "Required for employees.": "Pflichtfeld für Mitarbeitende.",
    "— None —": "— Keine —",
    "Request edit": "Bearbeitung anfordern",
    "Reopen this week?": "Diese Woche wieder bearbeiten?",
    "Your team lead will be notified and must approve before the week becomes editable again.":
      "Ihre Teamleitung wird benachrichtigt und muss zustimmen, bevor die Woche wieder bearbeitet werden kann.",
    "This week will be reopened immediately for editing.":
      "Diese Woche wird sofort wieder zur Bearbeitung freigegeben.",
    "Reopen request sent.": "Anfrage zur Wiederfreigabe gesendet.",
    "Week reopened.": "Woche wieder freigegeben.",
    "Reopen pending approval.": "Wiederfreigabe wartet auf Genehmigung.",
    "Reopen approved.": "Wiederfreigabe genehmigt.",
    "Reopen rejected.": "Wiederfreigabe abgelehnt.",
    "Week reopen requests": "Wochen-Wiederfreigaben",
    "wants to edit week of {date}":
      "möchte die Woche ab {date} wieder bearbeiten",
    TeamSettings: "Team-Einstellungen",
    "Team Settings": "Team-Einstellungen",
    "Allow employees to reopen weeks without approval":
      "Mitarbeitende dürfen Wochen ohne Genehmigung wieder bearbeiten",
    Notifications: "Benachrichtigungen",
    "No notifications.": "Keine Benachrichtigungen.",
    "Mark all as read": "Alle als gelesen markieren",
    "Clear all": "Alle löschen",
    "Auto-approve reopens": "Wiederfreigabe ohne Bestätigung",
    // Flextime chart
    "Flextime balance": "Gleitzeitkontostand",
    "Daily diff": "Tagesdifferenz",
    "Last 30 days": "Letzte 30 Tage",
    "Last 90 days": "Letzte 90 Tage",
    "Last 6 months": "Letzte 6 Monate",
    "Last year": "Letztes Jahr",
    "Custom range": "Benutzerdefinierter Zeitraum",
    Range: "Bereich",
    "From cannot be after To.": "Von kann nicht nach Bis liegen.",
    "Start cannot be after End.": "Start kann nicht nach Ende liegen.",
    // Hours unit
    hours_unit: "Std.",
    "{value}{unit}": "{value}{unit}",
    "{hours} / week": "{hours} / Woche",
    // Overlap / absence conflict
    "Conflict: Overlap with existing absence.":
      "Konflikt: Überschneidung mit bestehender Abwesenheit.",
    "Overlap with existing absence.":
      "Überschneidung mit bestehender Abwesenheit.",
    // Calendar: work-time categories + public holiday
    "Public holiday": "Feiertag",
    Absent: "Abwesend",
    // Reports help tooltips
    help_monthly_report:
      "Zeigt den Monatsbericht eines Mitarbeiters mit Soll-/Ist-Stunden und Details pro Tag.",
    help_team_report:
      "Vergleicht Soll- und Ist-Stunden aller aktiven Teammitglieder für den gewählten Monat.",
    help_category_breakdown:
      "Zeigt die Verteilung der erfassten Stunden auf die verschiedenen Kategorien.",
    help_csv_export:
      "Exportiert den Monatsbericht des gewählten Mitarbeiters als CSV-Datei.",
    // Reports help (English defaults)
    // (English keys fall through)
    // Audit log
    audit_table_users: "Benutzer",
    audit_table_absences: "Abwesenheit",
    audit_table_time_entries: "Zeiteintrag",
    audit_table_categories: "Kategorie",
    audit_table_holidays: "Feiertag",
    audit_table_sessions: "Sitzung",
    audit_table_notifications: "Benachrichtigung",
    audit_table_app_settings: "Einstellung",
    audit_table_reopen_requests: "Wiederfreigabe",
    audit_action_created: "Erstellt",
    audit_action_updated: "Bearbeitet",
    audit_action_deleted: "Gelöscht",
    audit_action_approved: "Genehmigt",
    audit_action_rejected: "Abgelehnt",
    audit_action_cancelled: "Storniert",
    audit_action_status_changed: "Status geändert",
    audit_action_team_settings_updated: "Team-Einstellung geändert",
    Data: "Daten",
    Summary: "Zusammenfassung",
    // Admin settings
    "Default weekly hours": "Standard-Wochenstunden",
    "Default annual leave days": "Standard-Urlaubstage",
    "Confirm password": "Passwort bestätigen",
    "Generate password": "Passwort generieren",
    "Password (min 12 chars)": "Passwort (mind. 12 Zeichen)",
    "Registration email will be sent.":
      "Es wird eine Registrierungs-E-Mail gesendet.",
    "User created.": "Benutzer erstellt.",
    "Temporary password:": "Temporäres Passwort:",
    Team: "Team",
    // Team Settings
    "Reopen Requests": "Wiederfreigabe-Anträge",
    "When enabled, employees can reopen submitted weeks without waiting for approval.":
      "Wenn aktiviert, können Mitarbeitende eingereichte Wochen ohne Genehmigungswartung wieder öffnen.",
    // Notification polling
    // (no new keys needed)
  },
};

function hasLanguage(language) {
  return Object.prototype.hasOwnProperty.call(LANGUAGES, language);
}
export function resolveLanguage(language) {
  return hasLanguage(language) ? language : DEFAULT_LANGUAGE;
}

function readStored() {
  try {
    return resolveLanguage(
      localStorage.getItem(STORAGE_KEY) || DEFAULT_LANGUAGE,
    );
  } catch {
    return DEFAULT_LANGUAGE;
  }
}

export const language = writable(readStored());

language.subscribe((lang) => {
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {}
  if (typeof document !== "undefined") {
    document.documentElement.lang = lang;
  }
});

function interpolate(template, params) {
  return template.replace(/\{(\w+)\}/g, (_, key) =>
    params[key] == null ? `{${key}}` : String(params[key]),
  );
}

export function translate(lang, key, params = {}) {
  const tpl = TRANSLATIONS[lang]?.[key] ?? key;
  return interpolate(tpl, params);
}

export const t = derived(
  language,
  ($lang) => (key, params) => translate($lang, key, params),
);

export function setLanguage(lang) {
  language.set(resolveLanguage(lang));
}
export function getLanguage() {
  return get(language);
}
export function getLocale() {
  return LANGUAGES[get(language)]?.locale || LANGUAGES[DEFAULT_LANGUAGE].locale;
}

export function roleLabel(role) {
  const labels = {
    employee: "Employee",
    team_lead: "Team lead",
    admin: "Admin",
  };
  return translate(get(language), labels[role] || role);
}
export function statusLabel(status) {
  const labels = {
    draft: "Draft",
    submitted: "Submitted",
    approved: "Approved",
    rejected: "Rejected",
    requested: "Requested",
    cancelled: "Cancelled",
    open: "Open",
  };
  return translate(get(language), labels[status] || status);
}
export function hoursUnit() {
  const result = translate(get(language), "hours_unit");
  return result === "hours_unit" ? "h" : result;
}

export function auditTableLabel(tableName) {
  const key = `audit_table_${tableName}`;
  const result = translate(get(language), key);
  // If no translation found, key is returned as-is; fallback to capitalized name
  return result === key
    ? tableName.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())
    : result;
}

export function auditActionLabel(action) {
  const key = `audit_action_${action}`;
  const result = translate(get(language), key);
  return result === key
    ? action.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())
    : result;
}

export function absenceKindLabel(kind) {
  const labels = {
    vacation: "Vacation",
    sick: "Sick",
    training: "Training",
    special_leave: "Special leave",
    unpaid: "Unpaid",
    general_absence: "General absence",
  };
  return translate(get(language), labels[kind] || kind);
}
