const STORAGE_KEY = "kitazeit.ui-language";

export const DEFAULT_LANGUAGE = "en";

export const LANGUAGES = Object.freeze({
  en: { label: "English", locale: "en-US" },
  de: { label: "Deutsch", locale: "de-DE" },
});

const TRANSLATIONS = {
  de: {
    "Loading...": "Wird geladen...",
    "Error": "Fehler",
    "Absences": "Abwesenheiten",
    "Calendar": "Kalender",
    "Account": "Konto",
    "Dashboard": "Dashboard",
    "Reports": "Berichte",
    "Admin": "Admin",
    "Sign out": "Abmelden",
    "Page not found": "Seite nicht gefunden",
    "Forbidden": "Kein Zugriff",
    "Sign in to your time-tracking workspace.": "Melden Sie sich in Ihrem Zeiterfassungsbereich an.",
    "Email": "E-Mail",
    "Password": "Passwort",
    "Sign in": "Anmelden",
    "Reason": "Begruendung",
    "Cancel": "Abbrechen",
    "OK": "OK",
    "Reason required": "Begruendung erforderlich",
    "Settings": "Einstellungen",
    "Language settings": "Spracheinstellungen",
    "Interface language": "Oberflaechensprache",
    "Missing translations fall back to English.": "Fehlende Uebersetzungen fallen auf Englisch zurueck.",
    "Language saved.": "Sprache gespeichert.",
    "Employee": "Mitarbeitende",
    "Team lead": "Teamleitung",
    "Users": "Benutzer",
    "Categories": "Kategorien",
    "Holidays": "Feiertage",
    "Audit log": "Audit-Protokoll",
    "Yes": "Ja",
    "No": "Nein",
    "Edit": "Bearbeiten",
    "Delete": "Loeschen",
    "Save": "Speichern",
    "Show": "Anzeigen",
    "Run": "Starten",
    "Add": "Hinzufuegen",
    "Submit": "Senden",
    "Approve": "Genehmigen",
    "Reject": "Ablehnen",
    "Date": "Datum",
    "Start": "Beginn",
    "End": "Ende",
    "Category": "Kategorie",
    "Comment": "Kommentar",
    "Comment (optional)": "Kommentar (optional)",
    "Status": "Status",
    "Action": "Aktion",
    "Type": "Typ",
    "From": "Von",
    "To": "Bis",
    "Half day": "Halber Tag",
    "Name": "Name",
    "Role": "Rolle",
    "Hours": "Stunden",
    "Leave": "Urlaub",
    "Active": "Aktiv",
    "Color": "Farbe",
    "Description": "Beschreibung",
    "Order": "Reihenfolge",
    "Time": "Zeit",
    "Table": "Tabelle",
    "User": "Benutzer",
    "Record": "Eintrag",
    "First name": "Vorname",
    "Last name": "Nachname",
    "Weekly hours": "Wochenstunden",
    "Annual leave days": "Urlaubstage pro Jahr",
    "Start date": "Startdatum",
    "New user": "Neuer Benutzer",
    "Edit user": "Benutzer bearbeiten",
    "New category": "Neue Kategorie",
    "Edit category": "Kategorie bearbeiten",
    "Add holiday": "Feiertag hinzufuegen",
    "Date and name required": "Datum und Name sind erforderlich",
    "Reset password?": "Passwort zuruecksetzen?",
    "A temporary password will be generated.": "Es wird ein temporaeres Passwort erzeugt.",
    "Temporary password: {password}": "Temporaeres Passwort: {password}",
    "User created. Temporary password: {password}": "Benutzer erstellt. Temporaeres Passwort: {password}",
    "Reset PW": "PW zuruecksetzen",
    "Deactivate?": "Deaktivieren?",
    "Deactivate": "Deaktivieren",
    "Time tracking": "Zeiterfassung",
    "Previous week": "Vorherige Woche",
    "Week {week}: {from} - {to}": "Woche {week}: {from} - {to}",
    "Next week": "Naechste Woche",
    "Copied {count} entries.": "{count} Eintraege kopiert.",
    "Copy last week": "Letzte Woche kopieren",
    "Target": "Soll",
    "Actual": "Ist",
    "Difference": "Differenz",
    "Delete?": "Loeschen?",
    "Delete this entry?": "Diesen Eintrag loeschen?",
    "Request change": "Aenderung anfordern",
    "Add entry": "Eintrag hinzufuegen",
    "Week submitted.": "Woche eingereicht.",
    "Submit week ({count})": "Woche einreichen ({count})",
    "Edit entry": "Eintrag bearbeiten",
    "Original: {date} {start}-{end}": "Original: {date} {start}-{end}",
    "Why is the change needed?": "Warum ist die Aenderung noetig?",
    "Submit request": "Anfrage senden",
    "Change request submitted.": "Aenderungsanfrage eingereicht.",
    "Annual entitlement": "Jahresanspruch",
    "Already taken": "Bereits genommen",
    "Approved upcoming": "Genehmigt bevorstehend",
    "Requested": "Beantragt",
    "Available": "Verfuegbar",
    "Request vacation": "Urlaub beantragen",
    "Report sick": "Krank melden",
    "Request training": "Fortbildung beantragen",
    "Request special leave": "Sonderurlaub beantragen",
    "Request unpaid leave": "Unbezahlte Freistellung beantragen",
    "Request general absence": "Allgemeine Abwesenheit beantragen",
    "Training": "Fortbildung",
    "Special leave": "Sonderurlaub",
    "Unpaid": "Unbezahlt",
    "General absence": "Allgemeine Abwesenheit",
    "Cancel?": "Abbrechen?",
    "Cancel this request?": "Diese Anfrage abbrechen?",
    "Edit absence": "Abwesenheit bearbeiten",
    "Sick leave saved.": "Krankmeldung gespeichert.",
    "Request submitted.": "Anfrage eingereicht.",
    "Absence calendar": "Abwesenheitskalender",
    "Previous month": "Vorheriger Monat",
    "Next month": "Naechster Monat",
    "Vacation": "Urlaub",
    "Sick": "Krank",
    "My account": "Mein Konto",
    "Please change your password.": "Bitte aendern Sie Ihr Passwort.",
    "You are using a temporary password.": "Sie verwenden ein temporaeres Passwort.",
    "Personal data": "Persoenliche Daten",
    "Name:": "Name:",
    "Email:": "E-Mail:",
    "Role:": "Rolle:",
    "Weekly hours:": "Wochenstunden:",
    "Annual leave:": "Urlaub:",
    "Start date:": "Startdatum:",
    "Change password": "Passwort aendern",
    "Current password": "Aktuelles Passwort",
    "New password (min 12 chars)": "Neues Passwort (mind. 12 Zeichen)",
    "Password changed.": "Passwort geaendert.",
    "Overtime balance {year}": "Ueberstundenkonto {year}",
    "Balance": "Saldo",
    "Month": "Monat",
    "Diff": "Diff.",
    "Cumulative": "Kumuliert",
    "Submitted entries": "Eingereichte Eintraege",
    "Open requests": "Offene Antraege",
    "Change requests": "Aenderungsantraege",
    "Submitted time entries": "Eingereichte Zeiteintraege",
    "No open entries.": "Keine offenen Eintraege.",
    "Approved.": "Genehmigt.",
    "Approve all": "Alle genehmigen",
    "Open absence requests": "Offene Abwesenheitsantraege",
    "No open requests.": "Keine offenen Antraege.",
    "No open change requests.": "Keine offenen Aenderungsanfragen.",
    "Change request": "Aenderungsanfrage",
    "Reason: {reason}": "Begruendung: {reason}",
    "New values: {date} {start}-{end}": "Neue Werte: {date} {start}-{end}",
    "Approve & apply": "Genehmigen und anwenden",
    "Week {week}": "Woche {week}",
    "Monthly report": "Monatsbericht",
    "Export CSV": "CSV exportieren",
    "Weekday": "Wochentag",
    "Entries": "Eintraege",
    "Note": "Hinweis",
    "By category": "Nach Kategorie",
    "Team report": "Teambericht",
    "Category breakdown": "Kategorieauswertung",
    "No data.": "Keine Daten.",
    "Please change your temporary password.": "Bitte aendern Sie Ihr temporaeres Passwort.",
    "Invalid language.": "Ungueltige Sprache.",
    "Invalid email or password.": "Ungueltige E-Mail oder ungültiges Passwort.",
    "Password must be at least 12 characters.": "Das Passwort muss mindestens 12 Zeichen lang sein.",
    "Password is too long (max 256 chars).": "Das Passwort ist zu lang (max. 256 Zeichen).",
    "Password must include at least 3 of: lowercase, uppercase, digit, symbol.": "Das Passwort muss mindestens 3 der folgenden Gruppen enthalten: Kleinbuchstaben, Grossbuchstaben, Ziffern, Sonderzeichen.",
    "Invalid role": "Ungueltige Rolle",
    "Invalid email.": "Ungueltige E-Mail.",
    "Could not update user (e.g. email conflict).": "Benutzer konnte nicht aktualisiert werden (z. B. E-Mail-Konflikt).",
    "Email already exists": "E-Mail existiert bereits",
    "You cannot remove your own admin role.": "Sie koennen Ihre eigene Admin-Rolle nicht entfernen.",
    "You cannot deactivate yourself.": "Sie koennen sich nicht selbst deaktivieren.",
    "Internal server error": "Interner Serverfehler",
    "Not authenticated": "Nicht angemeldet",
    "Draft": "Entwurf",
    "Submitted": "Eingereicht",
    "Approved": "Genehmigt",
    "Rejected": "Abgelehnt",
    "Cancelled": "Storniert",
    "Open": "Offen",
    "approved": "genehmigt",
    "cancelled": "storniert",
    "deactivated": "deaktiviert",
    "password_reset": "Passwort zurueckgesetzt",
    "absences": "Abwesenheiten",
    "time_entries": "Zeiteintraege",
    "change_requests": "Aenderungsanfragen",
    "users": "Benutzer",
    "Monday": "Montag",
    "Tuesday": "Dienstag",
    "Wednesday": "Mittwoch",
    "Thursday": "Donnerstag",
    "Friday": "Freitag",
    "Saturday": "Samstag",
    "Sunday": "Sonntag"
  },
};

function hasLanguage(language) {
  return Object.prototype.hasOwnProperty.call(LANGUAGES, language);
}

function interpolate(template, params) {
  return template.replace(/\{(\w+)\}/g, (_, key) => {
    if (params[key] == null) return `{${key}}`;
    return String(params[key]);
  });
}

let currentLanguage = DEFAULT_LANGUAGE;

export function resolveLanguage(language) {
  return hasLanguage(language) ? language : DEFAULT_LANGUAGE;
}

export function getStoredLanguage() {
  try {
    return resolveLanguage(localStorage.getItem(STORAGE_KEY) || DEFAULT_LANGUAGE);
  } catch {
    return DEFAULT_LANGUAGE;
  }
}

export function setLanguage(language) {
  currentLanguage = resolveLanguage(language);
  try {
    localStorage.setItem(STORAGE_KEY, currentLanguage);
  } catch {}
  if (typeof document !== "undefined") {
    document.documentElement.lang = currentLanguage;
    document.title = "KitaZeit";
  }
  return currentLanguage;
}

export function getLanguage() {
  return currentLanguage;
}

export function getLocale() {
  return LANGUAGES[currentLanguage]?.locale || LANGUAGES[DEFAULT_LANGUAGE].locale;
}

export function t(key, params = {}) {
  const translated = TRANSLATIONS[currentLanguage]?.[key] ?? key;
  return interpolate(translated, params);
}

export function roleLabel(role) {
  const labels = {
    employee: "Employee",
    team_lead: "Team lead",
    admin: "Admin",
  };
  return t(labels[role] || role);
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
  return t(labels[status] || status);
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
  return t(labels[kind] || kind);
}

setLanguage(getStoredLanguage());