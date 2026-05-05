import { writable, derived, get } from "svelte/store";

const STORAGE_KEY = "zerf.ui-language";
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
      "Exports the selected employee and date range as a CSV file.",
    audit_table_users: "User",
    audit_table_absences: "Absence",
    audit_table_time_entries: "Time Entry",
    audit_table_categories: "Category",
    audit_table_holidays: "Holiday",
    audit_table_sessions: "Session",
    audit_table_notifications: "Notification",
    audit_table_app_settings: "Setting",
    audit_table_reopen_requests: "Reopen Request",
    audit_table_change_requests: "Change Request",
    audit_action_created: "Created",
    audit_action_updated: "Updated",
    audit_action_deleted: "Deleted",
    audit_action_approved: "Approved",
    audit_action_rejected: "Rejected",
    audit_action_cancelled: "Cancelled",
    audit_action_status_changed: "Status Changed",
    audit_action_team_settings_updated: "Team Setting Updated",
    audit_action_password_reset: "Password Reset",
    audit_action_deactivated: "Deactivated",
    audit_action_reopened: "Reopened",
    "of {target} target": "of {target} target",
    "Open calendar": "Open calendar",
    "Open time picker": "Open time picker",
    Year: "Year",
    "Invalid date": "Invalid date.",
    "Invalid date.": "Invalid date.",
    "end_date must be >= start_date.": "From cannot be after To.",
    "Absence range exceeds one year.": "Absence range exceeds one year.",
    "Conflict: Overlap with existing absence":
      "Conflict: Overlap with existing absence.",
    "Overlap with existing absence": "Overlap with existing absence.",
    "Yes, cancel absence": "Yes, cancel absence",
    "Vacation days ({year})": "Vacation days ({year})",
    "Vacation used ({year})": "Vacation used ({year})",
    "Approved upcoming ({year})": "Approved upcoming ({year})",
    "Approved days not yet taken": "Approved days not yet taken",
    "Vacation pending ({year})": "Vacation pending ({year})",
    "Vacation remaining ({year})": "Vacation remaining ({year})",
    "Vacation requests awaiting approval":
      "Vacation requests awaiting approval",
    you: "you",
    "Public holiday": "Public holiday",
    Holiday: "Holiday",
    Work: "Work",
    "Work time": "Work time",
    Close: "Close",
    Absent: "Absent",
    Created: "Created",
    Cleared: "Cleared",
    "Please change at least one field.": "Please change at least one field.",
    "At least one actual change is required.":
      "At least one actual change is required.",
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
    Created: "Erstellt",
    Cleared: "Gelöscht",
    "Please change at least one field.":
      "Bitte ändern Sie mindestens ein Feld.",
    "At least one actual change is required.":
      "Mindestens eine tatsächliche Änderung ist erforderlich.",

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
    "Overtime start balance (minutes)": "Überstunden-Startsaldo (Minuten)",
    "Initial overtime balance in minutes when the user starts. Negative = deficit.":
      "Anfangssaldo der Überstunden in Minuten zum Startdatum. Negativ = Defizit.",
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

    Target: "Soll",
    Actual: "Ist",
    Difference: "Differenz",
    "Add entry": "Eintrag hinzufügen",
    "Edit entry": "Eintrag bearbeiten",
    "Delete?": "Löschen?",
    "Delete this entry?": "Diesen Eintrag löschen?",
    "Request change": "Änderung anfordern",
    "Submit week ({count})": "Woche einreichen ({count})",
    "Submit this week?": "Diese Woche einreichen?",
    "All draft entries of this week will be submitted for approval.":
      "Alle Entwurfs-Einträge dieser Woche werden zur Genehmigung eingereicht.",
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
    Year: "Jahr",
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
    Partial: "Teilweise",
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
    "of {target} target": "von {target} Soll",
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
    "Approve all?": "Alle genehmigen?",
    "Approve all {n} submitted entries across all users?":
      "Alle {n} eingereichten Einträge aller Benutzer genehmigen?",
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
    "Organization name": "Organisationsname",
    "Save Changes": "Änderungen speichern",
    "Saving...": "Speichert...",
    "Signing in…": "Anmeldung läuft…",
    "Settings saved.": "Einstellungen gespeichert.",
    "Initial setup required.": "Ersteinrichtung erforderlich.",
    "Please configure the country, region, default weekly hours and default annual leave days before using the application.":
      "Bitte Land, Region, Standard-Wochenstunden und Standard-Urlaubstage konfigurieren, bevor die Anwendung genutzt wird.",
    "Please select a country.": "Bitte ein Land auswählen.",
    "Please enter default weekly hours.":
      "Bitte Standard-Wochenstunden eingeben.",
    "Please enter default annual leave days.":
      "Bitte Standard-Urlaubstage eingeben.",
    "- Please select -": "- Bitte auswählen -",
    "- All -": "- Alle -",
    Country: "Land",
    Region: "Region",
    "e.g. US-CA": "z.B. US-CA",
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
    "Rejected.": "Abgelehnt.",
    Retry: "Erneut versuchen",
    // Default category names
    "Core Duties": "Kernaufgaben",
    "Preparation Time": "Vorbereitungszeit",
    "Leadership Tasks": "Leitungsaufgaben",
    "Team Meeting": "Teambesprechung",
    Other: "Sonstiges",
    "Switch to dark mode": "Dunkelmodus aktivieren",
    "Switch to light mode": "Hellmodus aktivieren",
    Appearance: "Erscheinungsbild",
    "Dark mode": "Dunkelmodus",
    "Use dark colour scheme": "Dunkles Farbschema verwenden",
    Enabled: "Aktiviert",
    Disabled: "Deaktiviert",
    // Reopen-week feature
    "Approver (Team lead / Admin)": "Verantwortliche Teamleitung / Admin",
    "Required for employees.": "Pflichtfeld für Mitarbeitende.",
    "An approver is required for employees.":
      "Für Mitarbeitende ist eine verantwortliche Person erforderlich.",
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
    "No categories available.": "Keine Kategorien verfügbar.",
    "Mark all as read": "Alle als gelesen markieren",
    "Clear all": "Alle löschen",
    "Failed to load categories. Some features may be unavailable.":
      "Kategorien konnten nicht geladen werden. Einige Funktionen sind möglicherweise nicht verfügbar.",
    "Could not reach the server. Please check your connection.":
      "Server nicht erreichbar. Bitte prüfen Sie Ihre Verbindung.",
    "Network error. Please check your connection.":
      "Netzwerkfehler. Bitte prüfen Sie Ihre Verbindung.",
    "Session expired. Please sign in again.":
      "Sitzung abgelaufen. Bitte erneut anmelden.",
    "Your session has expired. Please sign in again.":
      "Ihre Sitzung ist abgelaufen. Bitte melden Sie sich erneut an.",
    "Invalid email or password.":
      "Ungültige E-Mail-Adresse oder ungültiges Passwort.",
    "Not authenticated": "Nicht angemeldet.",
    "Not found": "Nicht gefunden.",
    "Internal server error": "Interner Serverfehler.",
    "Invalid body": "Ungültiger Anfrageinhalt.",
    "Invalid JSON": "Ungültiges JSON.",
    "Current password required.": "Aktuelles Passwort erforderlich.",
    "Current password is incorrect.": "Aktuelles Passwort ist falsch.",
    "New password must differ from the current one.":
      "Neues Passwort muss sich vom aktuellen Passwort unterscheiden.",
    "Password must be at least {min} characters.":
      "Passwort muss mindestens {min} Zeichen lang sein.",
    "Password is too long (max 256 chars).":
      "Passwort ist zu lang (max. 256 Zeichen).",
    "Password must include at least 3 of: lowercase, uppercase, digit, symbol.":
      "Passwort muss mindestens 3 davon enthalten: Kleinbuchstabe, Großbuchstabe, Ziffer, Symbol.",
    "Invalid language.": "Ungültige Sprache.",
    "Invalid time format.": "Ungültiges Uhrzeitformat.",
    "Country must be a 2-letter ISO code (or empty to clear).":
      "Land muss ein zweistelliger ISO-Code sein (oder leer zum Zurücksetzen).",
    "Region code must be at most 20 characters.":
      "Regionscode darf höchstens 20 Zeichen lang sein.",
    "Invalid default_weekly_hours.": "Ungültige Standard-Wochenstunden.",
    "Invalid default_annual_leave_days.": "Ungültige Standard-Urlaubstage.",
    "Invalid role": "Ungültige Rolle.",
    "Invalid email.": "Ungültige E-Mail-Adresse.",
    "Invalid name.": "Ungültiger Name.",
    "Invalid weekly_hours.": "Ungültige Wochenstunden.",
    "Invalid annual_leave_days.": "Ungültige Urlaubstage.",
    "An approver (Team lead or Admin) is required for employees.":
      "Für Mitarbeitende ist eine Teamleitung oder ein Admin als verantwortliche Person erforderlich.",
    "Approver cannot be the user themselves.":
      "Verantwortliche Person darf nicht dieselbe Person sein.",
    "Approver must be an active Team lead or Admin.":
      "Verantwortliche Person muss eine aktive Teamleitung oder ein Admin sein.",
    "Approver not found.": "Verantwortliche Person nicht gefunden.",
    "Email already exists or invalid approver.":
      "E-Mail existiert bereits oder verantwortliche Person ist ungültig.",
    "Could not update user (e.g. email conflict).":
      "Benutzer konnte nicht aktualisiert werden (z.B. E-Mail-Konflikt).",
    "Could not update approver.":
      "Verantwortliche Person konnte nicht aktualisiert werden.",
    "You cannot remove your own admin role.":
      "Sie können Ihre eigene Admin-Rolle nicht entfernen.",
    "You cannot deactivate yourself.":
      "Sie können sich nicht selbst deaktivieren.",
    "Cannot deactivate: {count} active user(s) still have this person as their approver. Reassign them first.":
      "Deaktivierung nicht möglich: {count} aktive Benutzer haben diese Person noch als verantwortliche Person. Weisen Sie sie zuerst neu zu.",
    "User not found or inactive.": "Benutzer nicht gefunden oder inaktiv.",
    "Cannot log time on a day with an approved absence ({kind}). Please cancel or adjust the absence first.":
      "An einem Tag mit genehmigter Abwesenheit ({kind}) kann keine Zeit erfasst werden. Bitte stornieren oder ändern Sie zuerst die Abwesenheit.",
    "Invalid time: {time}": "Ungültige Uhrzeit: {time}",
    "Invalid kind": "Ungültiger Typ.",
    "month=YYYY-MM required": "Monat im Format JJJJ-MM erforderlich.",
    "month=YYYY-MM": "Monat im Format JJJJ-MM erforderlich.",
    "Invalid year": "Ungültiges Jahr.",
    "Invalid month": "Ungültiger Monat.",
    year: "Ungültiges Jahr.",
    month: "Ungültiger Monat.",
    date: "Ungültiges Datum.",
    "from must not be after to.": "Von darf nicht nach Bis liegen.",
    "Date range must not exceed 366 days.":
      "Der Zeitraum darf 366 Tage nicht überschreiten.",
    "from is required.": "Von ist erforderlich.",
    "to is required.": "Bis ist erforderlich.",
    "CSV export failed.": "CSV-Export fehlgeschlagen.",
    "Name already exists": "Name existiert bereits.",
    "Holiday already exists": "Feiertag existiert bereits.",
    "Conflict: {message}": "Konflikt: {message}",
    "week_start must be a Monday (ISO).":
      "Wochenbeginn muss ein Montag sein (ISO).",
    "Nothing to reopen - this week has no submitted or approved entries.":
      "Keine Wiederfreigabe möglich: Diese Woche enthält keine eingereichten oder genehmigten Einträge.",
    "A pending reopen request already exists (id {id}).":
      "Eine offene Wiederfreigabe-Anfrage existiert bereits (ID {id}).",
    "A pending request for this week already exists.":
      "Für diese Woche existiert bereits eine offene Anfrage.",
    "Request was already resolved by someone else.":
      "Anfrage wurde bereits von jemand anderem bearbeitet.",
    "An open change request already exists for this entry (id {id}).":
      "Für diesen Eintrag existiert bereits eine offene Änderungsanfrage (ID {id}).",
    "Leave balance unavailable.": "Urlaubsstand nicht verfügbar.",
    "Overtime data unavailable.": "Überstundendaten nicht verfügbar.",
    "Overtime overview": "Überstundenübersicht",
    "This month: {value}": "Diesen Monat: {value}",
    "Submission status": "Einreichungsstatus",
    "All previous months submitted": "Alle vergangenen Monate eingereicht",
    "{count} month(s) incomplete": "{count} Monat(e) unvollständig",
    "No previous months yet": "Noch keine vergangenen Monate",
    "Could not check submission status.":
      "Einreichungsstatus konnte nicht geprüft werden.",
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
    "Category required.": "Kategorie erforderlich.",
    // Hours unit
    hours_unit: "Std.",
    "{value}{unit}": "{value} {unit}",
    "{hours} / week": "{hours} / Woche",
    "Open calendar": "Kalender öffnen",
    "Open time picker": "Uhrzeitauswahl öffnen",
    "Invalid date": "Ungültiges Datum.",
    "Invalid date.": "Ungültiges Datum.",
    "end_date must be >= start_date.": "Von kann nicht nach Bis liegen.",
    "Absence range exceeds one year.":
      "Der Abwesenheitszeitraum darf ein Jahr nicht überschreiten.",
    "Non-sick absences cannot overlap days with logged time. Please remove or reject the time entries first.":
      "Nicht-Krank-Abwesenheiten dürfen sich nicht mit Tagen mit gebuchter Zeit überschneiden. Bitte entfernen oder verwerfen Sie die Zeiteinträge zuerst.",
    you: "Sie",
    // Overlap / absence conflict
    "Conflict: Overlap with existing absence":
      "Konflikt: Überschneidung mit bestehender Abwesenheit.",
    "Conflict: Overlap with existing absence.":
      "Konflikt: Überschneidung mit bestehender Abwesenheit.",
    "Overlap with existing absence":
      "Überschneidung mit bestehender Abwesenheit.",
    "Overlap with existing absence.":
      "Überschneidung mit bestehender Abwesenheit.",
    // Time entry errors
    "Overlap with an existing entry.":
      "Überschneidung mit einem bestehenden Eintrag.",
    "Entries in the future are not allowed.":
      "Einträge in der Zukunft sind nicht erlaubt.",
    "Day total exceeds 14 hours.": "Tagestotal überschreitet 14 Stunden.",
    "End time must be after start time.":
      "Endzeit muss nach der Startzeit liegen.",
    "Comment too long (max 2000).": "Kommentar zu lang (max. 2000).",
    "Comment too long.": "Kommentar zu lang.",
    "Category not found.": "Kategorie nicht gefunden.",
    "Category is inactive.": "Kategorie ist inaktiv.",
    "Only drafts can be deleted.": "Nur Entwürfe können gelöscht werden.",
    "Only drafts can be edited directly. Please file a change request.":
      "Nur Entwürfe können direkt bearbeitet werden. Bitte stellen Sie eine Änderungsanfrage.",
    "Only submitted entries can be approved.":
      "Nur eingereichte Einträge können genehmigt werden.",
    "Only submitted entries can be rejected.":
      "Nur eingereichte Einträge können abgelehnt werden.",
    "Entry was already reviewed by someone else.":
      "Eintrag wurde bereits von jemand anderem geprüft.",
    "Reason too long.": "Begründung zu lang.",
    "Reason required.": "Begründung erforderlich.",
    // Change request errors
    "Date cannot be in the future.": "Datum darf nicht in der Zukunft liegen.",
    "Edit drafts directly.": "Entwürfe können direkt bearbeitet werden.",
    "Invalid time format (HH:MM).": "Ungültiges Zeitformat (HH:MM).",
    "Change request could no longer be applied because the entry changed.":
      "Änderungsanfrage konnte nicht mehr angewendet werden, da sich der Eintrag geändert hat.",
    "Change request was already resolved by someone else.":
      "Änderungsanfrage wurde bereits von jemand anderem bearbeitet.",
    "Rejected entries cannot have change requests. Use the reopen workflow to edit.":
      "Abgelehnte Einträge können keine Änderungsanfragen haben. Nutzen Sie die Wiederfreigabe.",
    // Absence errors
    "Cannot edit.": "Bearbeitung nicht möglich.",
    "Absence was already reviewed by someone else.":
      "Abwesenheit wurde bereits von jemand anderem geprüft.",
    "Only requested absences can be approved.":
      "Nur beantragte Abwesenheiten können genehmigt werden.",
    "Only requested absences can be rejected.":
      "Nur beantragte Abwesenheiten können abgelehnt werden.",
    "Only requested absences and auto-approved sick absences can be cancelled.":
      "Nur beantragte Abwesenheiten und automatisch genehmigte Krankmeldungen können storniert werden.",
    "Only requested absences can be cancelled.":
      "Nur beantragte Abwesenheiten können storniert werden.",
    "Only approved absences can be revoked.":
      "Nur genehmigte Abwesenheiten können widerrufen werden.",
    "Approved absences cannot change type.":
      "Genehmigte Abwesenheiten können den Typ nicht ändern.",
    "Sick absences cannot change type.":
      "Krankmeldungen können den Typ nicht ändern.",
    "Sick leave cannot be backdated more than 30 days.":
      "Krankmeldungen können nicht mehr als 30 Tage rückdatiert werden.",
    // Reopen request errors
    "Request is not pending.": "Anfrage ist nicht ausstehend.",
    "Yes, cancel absence": "Ja, Abwesenheit stornieren",
    "Vacation days ({year})": "Urlaubstage ({year})",
    "Vacation used ({year})": "Genommene Urlaubstage ({year})",
    "Approved upcoming ({year})": "Genehmigte bevorstehende ({year})",
    "Approved days not yet taken": "Genehmigte Tage noch nicht genommen",
    "Vacation pending ({year})": "Offene Urlaubstage ({year})",
    "Vacation remaining ({year})": "Verbleibende Urlaubstage ({year})",
    "Vacation requests awaiting approval":
      "Urlaubsanträge warten auf Genehmigung",
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
      "Exportiert den gewählten Mitarbeiter und Zeitraum als CSV-Datei.",
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
    audit_table_change_requests: "Änderungsanfrage",
    audit_action_created: "Erstellt",
    audit_action_updated: "Bearbeitet",
    audit_action_deleted: "Gelöscht",
    audit_action_approved: "Genehmigt",
    audit_action_rejected: "Abgelehnt",
    audit_action_cancelled: "Storniert",
    audit_action_status_changed: "Status geändert",
    audit_action_team_settings_updated: "Team-Einstellung geändert",
    audit_action_password_reset: "Passwort zurückgesetzt",
    audit_action_deactivated: "Deaktiviert",
    audit_action_reopened: "Wieder geöffnet",
    Data: "Daten",
    Summary: "Zusammenfassung",
    // Admin settings
    "Time format": "Uhrzeitformat",
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
    "When enabled for a user, their reopen requests are automatically approved. Their approver and all admins still receive a notification.":
      "Wenn aktiviert, werden die Wiederfreigabe-Anträge des Benutzers automatisch genehmigt. Der Verantwortliche und alle Admins erhalten trotzdem eine Benachrichtigung.",
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

const ABSENCE_KIND_LABELS = Object.freeze({
  vacation: "Vacation",
  sick: "Sick",
  training: "Training",
  special_leave: "Special leave",
  unpaid: "Unpaid",
  general_absence: "General absence",
});

function translatedAbsenceKind(lang, kind) {
  return translate(lang, ABSENCE_KIND_LABELS[kind] || kind);
}

const ERROR_PATTERNS = Object.freeze([
  {
    pattern: /^Password must be at least (?<min>\d+) characters\.$/,
    key: "Password must be at least {min} characters.",
  },
  {
    pattern:
      /^Cannot deactivate: (?<count>\d+) active user\(s\) still have this person as their approver\. Reassign them first\.$/,
    key: "Cannot deactivate: {count} active user(s) still have this person as their approver. Reassign them first.",
  },
  {
    pattern:
      /^Cannot log time on a day with an approved absence \((?<kind>[^)]+)\)\. Please cancel or adjust the absence first\.$/,
    key: "Cannot log time on a day with an approved absence ({kind}). Please cancel or adjust the absence first.",
    params(match, lang) {
      return { kind: translatedAbsenceKind(lang, match.groups.kind) };
    },
  },
  {
    pattern: /^Invalid time: (?<time>.+)$/,
    key: "Invalid time: {time}",
  },
  {
    pattern:
      /^An open change request already exists for this entry \(id (?<id>\d+)\)\.$/,
    key: "An open change request already exists for this entry (id {id}).",
  },
  {
    pattern: /^A pending reopen request already exists \(id (?<id>\d+)\)\.$/,
    key: "A pending reopen request already exists (id {id}).",
  },
  {
    pattern:
      /^Nothing to reopen [-\u2013\u2014] this week has no submitted or approved entries\.$/,
    key: "Nothing to reopen - this week has no submitted or approved entries.",
  },
]);

function normalizedErrorMessage(message) {
  return String(message || "Error")
    .replace(/\s+/g, " ")
    .trim();
}

function translateDirectOrPattern(lang, message) {
  const direct = translate(lang, message);
  if (direct !== message) return direct;

  for (const item of ERROR_PATTERNS) {
    const match = message.match(item.pattern);
    if (!match) continue;
    return translate(
      lang,
      item.key,
      item.params ? item.params(match, lang) : match.groups,
    );
  }

  return null;
}

export function localizeErrorMessage(message, lang = get(language)) {
  const normalized = normalizedErrorMessage(message);
  const translated = translateDirectOrPattern(lang, normalized);
  if (translated) return translated;

  const conflictPrefix = "Conflict: ";
  if (normalized.startsWith(conflictPrefix)) {
    const detail = normalized.slice(conflictPrefix.length).trim();
    const translatedDetail = translateDirectOrPattern(lang, detail) || detail;
    return translate(lang, "Conflict: {message}", {
      message: translatedDetail,
    });
  }

  return normalized;
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
    partial: "Partial",
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

export function formatHours(value) {
  return translate(get(language), "{value}{unit}", {
    value,
    unit: hoursUnit(),
  });
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
  return translatedAbsenceKind(get(language), kind);
}
