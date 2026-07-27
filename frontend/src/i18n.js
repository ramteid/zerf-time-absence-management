import { writable, derived, get } from "svelte/store";

// --- Configuration ---

const STORAGE_KEY = "zerf.ui-language";
export const DEFAULT_LANGUAGE = "en";

// Supported languages with their display labels and locale codes used for date formatting.
export const LANGUAGES = Object.freeze({
  en: { label: "English", locale: "en-US" },
  de: { label: "Deutsch", locale: "de-DE" },
});

// --- Translation tables ---

// Keys in `en` are the canonical translation keys used throughout the app.
// Keys absent from `de` fall back to the English value at runtime.
const TRANSLATIONS = {
  en: {
    hours_unit: "h",
    "{hours} / week": "{hours} / week",
    "As of yesterday": "As of yesterday",
    help_team_report:
      "Compares target and actual hours for all active users in the selected month. For the current month, data is available including today.",
    help_category_breakdown:
      "Shows how tracked hours are distributed across the different categories.",
    help_absence_report:
      "View absence entries over a selected period with type distribution. Rejected and cancelled absences are excluded.",
    help_logged:
      "Submitted and approved hours including the current day for the current month.",
    help_employee_details:
      "View detailed information about a user including balance and statistics.",
    help_my_balance:
      "Overview of your current flextime balance and submission status. The balance is calculated up to and including yesterday; today's hours are not yet counted. The overtime overview also factors in submitted hours pending approval.",
    help_flextime_chart:
      "Your cumulative flextime balance over the selected period. The balance is calculated up to and including yesterday; today's hours are not yet counted.",
    "Show explanation": "Show explanation",
    label_cost_type_none: "Uses nothing (no vacation, no flextime)",
    help_cost_type_none:
      "The day is excused: no time has to be logged and the work target for that day falls away. Nothing is taken from the vacation balance and no flextime is used, so the hours never have to be made up. If time is logged on such a day anyway (possible for categories with auto-approval, e.g. worked the morning and called in sick at noon), those hours count in full as a flextime gain. Whether the day is paid is decided in payroll, not in Zerf: training is normally paid, unpaid leave is not.",
    label_cost_type_vacation: "Uses vacation days",
    help_cost_type_vacation:
      "Every approved day is deducted from the employee's annual leave, including any carryover from the previous year and its expiry date. The work target for that day falls away, so the flextime balance is unaffected.",
    label_cost_type_flextime: "Uses flextime hours",
    help_cost_type_flextime:
      "Employees have the day off, but the work target for that day stays in place. The day therefore lowers the flextime balance by one daily target — this is how time off in lieu is taken. No vacation days are used. Zerf checks the flextime balance when the request is made and again when it is approved, so the balance cannot drop below the configured minimum.",
    help_auto_approve_past:
      'Requests with a start date on or before today are approved automatically (no approver review). Time entries can coexist with the absence on the same day (allows partial-day overlap like "worked the morning, called in sick at noon"). Backdating is limited to 30 days. Typical use: sick leave.',
    "Counts as work": "Counts as work",
    help_submission_status:
      "Whether all required weeks in the selected month have been submitted.",
    Approvals: "Approvals",
    "All approved": "All approved",
    Incomplete: "Incomplete",
    "All submitted": "All submitted",
    "All submitted and approved": "All submitted and approved",
    "All submitted (approvals pending)": "All submitted (approvals pending)",
    "Approved: {value}": "Approved: {value}",
    "Weeks missing": "Weeks missing",
    "Current week: still open": "Current week: still open",
    "Current week: draft": "Current week: draft",
    "Current week: partially submitted": "Current week: partially submitted",
    "Current week: needs revision": "Current week: needs revision",
    "Who is absent": "Who is absent",
    "Previous week": "Previous week",
    "Next week": "Next week",
    Today: "Today",
    "No absences this week.": "No absences this week.",
    "Employee Details": "Employee Details",
    "Total days": "Total days",
    Flextime: "Flextime",
    "Flextime Reduction": "Flextime Reduction",
    Vacation: "Vacation",
    Entitlement: "Entitlement",
    Taken: "Taken",
    Planned: "Planned",
    Requested: "Requested",
    Remaining: "Remaining",
    Export: "Export",
    "Export PDF": "Export PDF",
    "CSV download started.": "CSV download started.",
    "PDF download started.": "PDF download started.",
    Timesheet: "Timesheet",
    Filter: "Filter",
    Entries: "Entries",
    Days: "Days",
    audit_table_users: "User",
    audit_table_absences: "Absence",
    audit_table_time_entries: "Time Entry",
    audit_table_time_entry_weeks: "Timesheet Week",
    audit_table_categories: "Category",
    audit_table_holidays: "Holiday",
    audit_table_sessions: "Session",
    audit_table_notifications: "Notification",
    audit_table_app_settings: "Setting",
    audit_table_reopen_requests: "Edit Request",
    audit_action_created: "Created",
    audit_action_updated: "Updated",
    audit_action_deleted: "Deleted",
    audit_action_approved: "Approved",
    audit_action_auto_approved: "Auto-approved",
    audit_action_rejected: "Rejected",
    audit_action_cancelled: "Cancelled",
    audit_action_status_changed: "Status Changed",
    audit_action_team_settings_updated: "Team Setting Updated",
    audit_action_password_reset: "Password Reset",
    audit_action_deactivated: "Deactivated",
    audit_action_archived: "Archived",
    audit_action_restored: "Restored",
    audit_action_reopened: "Editing Enabled",
    audit_system_user: "System",
    audit_time_entries_week_summary:
      "Week {week}: {from} - {to} ({count} day entries)",
    Before: "Before",
    After: "After",
    For: "For",
    Date: "Date",
    Start: "Start",
    End: "End",
    Note: "Note",
    Email: "Email",
    Role: "Role",
    Type: "Type",
    From: "From",
    To: "To",
    Name: "Name",
    Color: "Color",
    Description: "Description",
    Setting: "Setting",
    Value: "Value",
    "Week start": "Week start",
    Yes: "Yes",
    No: "No",
    "of {target} target": "of {target} target",
    "Open calendar": "Open calendar",
    "Open time picker": "Open time picker",
    Year: "Year",
    "Invalid date": "Invalid date.",
    "Invalid date.": "Invalid date.",
    "Select an employee.": "Select an employee.",
    All: "All",
    "CSV export is only available for a single employee.":
      "CSV export is only available for a single employee.",
    "end_date must be >= start_date.": "From cannot be after To.",
    "Absence range exceeds one year.": "Absence range exceeds one year.",
    "Absence must include at least one workday.":
      "Absence must include at least one workday.",
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
    "Cancel absence": "Cancel absence",
    Absent: "Absent",
    Created: "Created",
    Cleared: "Cleared",
    "Please change at least one field.": "Please change at least one field.",
    "At least one actual change is required.":
      "At least one actual change is required.",
    "Carryover from {year}": "Carryover from {year}",
    "Expired on {date}": "Expired on {date}",
    "Expires on {date}": "Expires on {date}",
    "Vacation carryover": "Vacation carryover",
    "Carryover expiry date (MM-DD)": "Carryover expiry date (MM-DD)",
    "Unused vacation from the previous year expires on this date.":
      "Unused vacation from the previous year expires on this date.",
    "Shown on the login screen and in the navigation.":
      "Shown on the login screen and in the navigation.",
    "Users will be notified on this day of each month if they have unsubmitted time entries for previous months. Leave empty to disable. (1\u201328)":
      "Users will be notified on this day of each month if they have unsubmitted weeks from previous months. Leave empty to disable. (1\u201328)",
    "All draft entries of this week will be submitted for approval.":
      "All draft days of this week will be submitted for approval.",
    "Vacation days per year": "Vacation days per year",
    "Annual leave days (base)": "Annual leave days (base)",
    "Default entitlement used for every year unless overridden below (e.g. for special agreements).":
      "Default entitlement used for every year unless overridden below (e.g. for special agreements).",
    Override: "Override",
    "Workdays per week": "Workdays per week",
    "Workdays per week must be between 1 and 7.":
      "Workdays per week must be between 1 and 7.",
    "Workdays per week must be between 1 and 5.":
      "Workdays per week must be between 1 and 5.",
    days: "days",
    workday: "workday",
    workdays: "workdays",
    Set: "Set",
    "Overrides the default annual leave days for this user in the selected year.":
      "Overrides the default annual leave days for this user in the selected year.",
    "Not enough remaining vacation days.":
      "Not enough remaining vacation days.",
    "Not enough flextime balance for this absence.":
      "Not enough flextime balance for this absence.",
    "Cannot change absence category cost type (vacation ↔ flextime). Cancel and re-request with the new category.":
      "Cannot change absence category cost type (vacation ↔ flextime). Cancel and re-request with the new category.",
    "Please enter vacation days.": "Please enter vacation days.",
    "Absence Request Details": "Absence Request Details",
    "Show details": "Show details",
    "Requested at": "Requested at",
    "Forgot password?": "Forgot password?",
    "Enter your email to receive a password reset link.":
      "Enter your email to receive a password reset link.",
    "Send reset link": "Send reset link",
    "Sending...": "Sending...",
    "If your email address is registered, you will receive a reset link shortly.":
      "If your email address is registered, you will receive a reset link shortly.",
    "Back to sign in": "Back to sign in",
    "Choose a new password for your account.":
      "Choose a new password for your account.",
    "New password": "New password",
    "Confirm password": "Confirm password",
    "Passwords do not match.": "Passwords do not match.",
    "Set new password": "Set new password",
    "Password reset successfully. Please sign in.":
      "Password reset successfully. Please sign in.",
    password_reset_unavailable:
      "Password reset is not available. Please contact the administrator.",
    reset_token_expired:
      "This reset link has expired. Please request a new one.",
    reset_token_invalid: "This reset link is invalid or has already been used.",
    account_deactivated:
      "Your account has been deactivated. Please contact your administrator.",
    account_archived:
      "Your account has been archived. Please contact your administrator.",
    "Account active": "Account active",
    "User activated.": "User activated.",
    Active: "Active",
    Inactive: "Inactive",
    // Reports - new section labels and team report columns
    "Employee report": "Employee report",
    "Export timesheet": "Export timesheet",
    "Export team PDF": "Export team PDF",
    future_period_no_time_data:
      "This period is entirely in the future — hours and flextime data will appear once it begins.",
    team_table_month_only:
      "The team overview table is only available in month view.",
    "Current flextime balance": "Current flextime balance",
    "Monthly diff": "Monthly diff",
    Weekend: "Weekend",
    Weekends: "Weekends",
    "Sick days": "Sick days",
    "Vacation taken": "Vacation taken",
    "Vacation planned": "Vacation planned",
    "All weeks submitted": "All weeks submitted",
    "Note: current month - data up to yesterday":
      "Note: current month - data including today",
    // Dashboard request detail labels
    Approval: "Approval",
    Change: "Change",
    "Edit Request Details": "Edit Request Details",
    "Absence Type": "Absence Type",
    "Request Type": "Request Type",
    Changes: "Changes",
    "Diff unavailable for this request.": "Diff unavailable for this request.",
    Empty: "Empty",
    Week: "Week",
    Timezone: "Timezone",
    "Please select a timezone.": "Please select a timezone.",
    "Enable approval reminders": "Enable approval reminders",
    "When enabled, approvers are reminded by email about pending approvals every Monday.":
      "When enabled, approvers are reminded by email about pending approvals every Monday.",
    // --- Nextcloud upload settings ---
    "Nextcloud Backups": "Nextcloud Backups",
    "DB Backup Upload": "DB Backup Upload",
    "Report PDF Upload": "Report PDF Upload",
    "Enable DB backup upload": "Enable DB backup upload",
    "Enable report PDF upload": "Enable report PDF upload",
    "Share link (https://…/s/…)": "Share link (https://…/s/…)",
    "Share password (optional)": "Share password (optional)",
    "Upload day of month (1–28)": "Upload day of month (1–28)",
    "Backup interval (days)": "Backup interval (days)",
    "Upload now": "Upload now",
    "Uploading...": "Uploading...",
    "Upload settings saved.": "Upload settings saved.",
    "Report uploaded successfully.": "Report uploaded successfully.",
    "Upload failed.": "Upload failed.",
    "The backup interval is read by the backup container from the database at the start of each cycle. Changes take effect on the next backup run. The 10 most recent local backup files are kept automatically; older ones are deleted. Uploaded files in Nextcloud are not deleted automatically.":
      "The backup interval is read by the backup container from the database at the start of each cycle. Changes take effect on the next backup run. The 10 most recent local backup files are kept automatically; older ones are deleted. Uploaded files in Nextcloud are not deleted automatically.",
    "On the configured day of each month, an individual timesheet PDF is queued for every employee. Each PDF is uploaded as soon as the employee has fully submitted all their weeks — late submitters are automatically caught up on the next daily check.":
      "On the configured day of each month, an individual timesheet PDF is queued for every employee. Each PDF is uploaded as soon as the employee has fully submitted all their weeks — late submitters are automatically caught up on the next daily check.",
    // --- Payroll report settings ---
    "Payroll Report": "Payroll Report",
    "Monthly payroll report": "Monthly payroll report",
    "Send the payroll report by email": "Send the payroll report by email",
    "On the configured day of each month, the previous month's report is prepared and emailed as a PDF. It is only sent once every employee's month is final: weeks submitted, absence requests decided, and — for everyone whose hours are in the report — all time entries approved. Otherwise the report waits and is retried daily. Requires a configured email server.":
      "On the configured day of each month, the previous month's report is prepared and emailed as a PDF. It is only sent once every employee's month is final: weeks submitted, absence requests decided, and — for everyone whose hours are in the report — all time entries approved. Otherwise the report waits and is retried daily. Requires a configured email server.",
    "Recipient email address": "Recipient email address",
    "Send day of month (1–28)": "Send day of month (1–28)",
    "Report content": "Report content",
    "Absence days per employee": "Absence days per employee",
    "One row per absence period with the number of working days. Sick days are needed for health-insurance reimbursement, unpaid days reduce the salary payout.":
      "One row per absence period with the number of working days. Sick days are needed for health-insurance reimbursement, unpaid days reduce the salary payout.",
    "Working days and hours": "Working days and hours",
    "Worked days and approved hours per person, shown in hours:minutes and as a decimal value for payroll.":
      "Worked days and approved hours per person, shown in hours:minutes and as a decimal value for payroll.",
    Assistants: "Assistants",
    "All other employees": "All other employees",
    inactive: "inactive",
    "Send now": "Send now",
    "Send now prepares the previous month immediately and sends it if the month is already final. It does not replace the scheduled monthly run.":
      "Send now prepares the previous month immediately and sends it if the month is already final. It does not replace the scheduled monthly run.",
    "Payroll report settings saved.": "Payroll report settings saved.",
    "Payroll report sent.": "Payroll report sent.",
    "Nothing was sent: every month was already sent or is not final yet.":
      "Nothing was sent: every month was already sent or is not final yet.",
    "A recipient address is required to enable the payroll report.":
      "A recipient address is required to enable the payroll report.",
    "The payroll report is not enabled.": "The payroll report is not enabled.",
    "No recipient address configured for the payroll report.":
      "No recipient address configured for the payroll report.",
    "Email delivery is not configured; the payroll report cannot be sent.":
      "Email delivery is not configured; the payroll report cannot be sent.",
    "Invalid payroll report recipient.": "Invalid payroll report recipient.",
    "payroll_report_day_of_month must be between 1 and 28.":
      "The send day must be between 1 and 28.",
    "Select at least one section for the payroll report.":
      "Select at least one section for the payroll report.",
    "Category not available for you.": "Category not available for you.",
    "Absence category not available for you.":
      "Absence category not available for you.",
    "Available to employees": "Available to employees",
    "Unknown employee id.": "Unknown employee id.",
    "Unknown category id.": "Unknown category id.",
    "Unknown absence category id.": "Unknown absence category id.",
    "Team leads": "Team leads",
    "Allow team leads to create assistant users":
      "Allow team leads to create assistant users",
    'When enabled, team leads get a restricted Users tab where they may only create and manage "Assistant" users assigned to them. No other role can be created there. Disabled by default.':
      'When enabled, team leads get a restricted Users tab where they may only create and manage "Assistant" users assigned to them. No other role can be created there. Disabled by default.',
    "You can only manage assistants assigned to you.":
      "You can only manage assistants assigned to you.",
    "You will be set as their approver.": "You will be set as their approver.",
    // --- User archive / restore ---
    "Archive user?": "Archive user?",
    Archive: "Archive",
    "User archived.": "User archived.",
    "Archived Users": "Archived Users",
    "Archived on {date}": "Archived on {date}",
    Restore: "Restore",
    "Restore user?": "Restore user?",
    "User restored.": "User restored.",
    "No archived users.": "No archived users.",
    "This account will be deactivated and the user will no longer be able to log in. All data is preserved and the account can be restored later.":
      "This account will be deactivated and the user will no longer be able to log in. All data is preserved and the account can be restored later.",
    "This user approves {n} active user(s). Choose a replacement approver for each.":
      "This user approves {n} active user(s). Choose a replacement approver for each.",
    "Replacement approver for {name}": "Replacement approver for {name}",
    "Select approver": "Select approver",
    "All users must have a replacement approver assigned.":
      "All users must have a replacement approver assigned.",
    "Restore this archived account? The user will receive a temporary password and must change it on first login.":
      "Restore this archived account? The user will receive a temporary password and must change it on first login.",
    "New start date (optional)": "New start date (optional)",
    "Reset start date to avoid flextime gap":
      "Reset start date to avoid flextime gap",
    "Keep original start date": "Keep original start date",
    "If the account was archived for an extended period, resetting the start date prevents a large negative flextime balance from accumulating during the absence.":
      "If the account was archived for an extended period, resetting the start date prevents a large negative flextime balance from accumulating during the absence.",
    "Approver required for non-admin users.":
      "Approver required for non-admin users.",
    "User has historical data. Use archive instead.":
      "User has historical data. Use archive instead.",
    "System Log": "System Log",
    "Log entry": "Log entry",
    "No log entries.": "No log entries.",
    Warning: "Warning",
    Source: "Source",
    Previous: "Previous",
    Next: "Next",
    "Page {page} of {count}": "Page {page} of {count}",
  },
  de: {
    "Loading...": "Wird geladen...",
    Error: "Fehler",
    Time: "Zeit",
    Absences: "Abwesenheiten",
    Calendar: "Kalender",
    "My Calendar": "Mein Kalender",
    "Team Calendar": "Teamkalender",
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
    Weekday: "Wochentag",
    Start: "Start",
    End: "Ende",
    Category: "Kategorie",
    Minutes: "Minuten",
    Comment: "Kommentar",
    "Comment (optional)": "Kommentar (optional)",
    Status: "Status",
    Absence: "Abwesenheit",
    Total: "Gesamt",
    "Export failed.": "Export fehlgeschlagen.",
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
    Inactive: "Inaktiv",
    Color: "Farbe",
    Description: "Beschreibung",
    Order: "Reihenfolge",
    "First name": "Vorname",
    "Last name": "Nachname",
    "Your Name": "Ihr Name",
    "Please enter your first name and last name.":
      "Bitte geben Sie Ihren Vornamen und Nachnamen ein.",
    "Create the initial administrator account to get started.":
      "Erstellen Sie das erste Administratorkonto, um loszulegen.",
    "Please enter a valid email address.":
      "Bitte geben Sie eine gültige E-Mail-Adresse ein.",
    "Password must be at least 8 characters.":
      "Das Passwort muss mindestens 8 Zeichen lang sein.",
    "Password must be at least 12 characters.":
      "Das Passwort muss mindestens 12 Zeichen lang sein.",
    "Passwords do not match.": "Passwörter stimmen nicht überein.",
    "Confirm password": "Passwort bestätigen",
    "Creating account…": "Konto wird erstellt…",
    "Create admin account": "Administratorkonto erstellen",
    "Setup has already been completed.":
      "Die Einrichtung wurde bereits abgeschlossen.",
    "Invalid email address.": "Ungültige E-Mail-Adresse.",
    "First name and last name are required.":
      "Vorname und Nachname sind erforderlich.",
    "Name too long.": "Name zu lang.",
    "Password must be between 8 and 128 characters.":
      "Das Passwort muss zwischen 8 und 128 Zeichen lang sein.",
    "Weekly hours": "Wochenstunden",
    "Workdays per week": "Arbeitstage pro Woche",
    "Workdays per week must be between 1 and 7.":
      "Arbeitstage pro Woche muss zwischen 1 und 7 liegen.",
    "Workdays per week must be between 1 and 5.":
      "Arbeitstage pro Woche muss zwischen 1 und 5 liegen.",
    "Annual leave days": "Urlaubstage pro Jahr",
    "Overtime start balance (hours)": "Überstunden-Startsaldo (Stunden)",
    "Initial overtime balance in hours when the user starts. Negative = deficit.":
      "Anfangssaldo der Überstunden in Stunden zum Startdatum. Negativ = Defizit.",
    "Start date": "Startdatum",
    "Hire date": "Eintrittsdatum",
    "Used to calculate the prorated annual leave entitlement for employees who already worked before they started using Zerf. Leave empty to use the start date.":
      "Wird verwendet, um den anteiligen Urlaubsanspruch für Mitarbeitende zu berechnen, die bereits vor der Nutzung von Zerf gearbeitet haben. Leer lassen, um das Startdatum zu verwenden.",
    Clear: "Löschen",
    Settings: "Einstellungen",
    "Language settings": "Spracheinstellungen",
    "Interface language": "Oberflächensprache",
    Timezone: "Zeitzone",
    "Please select a timezone.": "Bitte wählen Sie eine Zeitzone aus.",
    "Missing translations fall back to English.":
      "Fehlende Übersetzungen fallen auf Englisch zurück.",
    "Language saved.": "Sprache gespeichert.",
    Employee: "Mitarbeitende",
    Assistant: "Aushilfe",
    "Team lead": "Teamleitung",
    Users: "Benutzer",
    Categories: "Kategorien",
    Holidays: "Feiertage",
    "Audit log": "Audit-Protokoll",
    audit_system_user: "System",
    audit_time_entries_week_summary:
      "Woche {week}: {from} - {to} ({count} Tagesbuchungen)",
    "Time tracking": "Zeiterfassung",
    "Previous week": "Vorherige Woche",
    "Next week": "Nächste Woche",
    Today: "Heute",
    "Week {week}: {from} - {to}": "Woche {week}: {from} - {to}",
    "Week {week}": "Woche {week}",

    "Add entry": "Eintrag hinzufügen",
    "Edit entry": "Eintrag bearbeiten",
    "Delete?": "Löschen?",
    "Delete this entry?": "Diesen Eintrag löschen?",
    "Submit week ({count})": "Woche einreichen ({count})",
    "Submit this week?": "Diese Woche einreichen?",
    "All draft entries of this week will be submitted for approval.":
      "Alle Entwürfe dieser Woche werden zur Genehmigung eingereicht.",
    "Week submitted.": "Woche eingereicht.",
    "Week approved.": "Woche genehmigt.",
    "Submit request": "Anfrage senden",
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
    "Cancel absence": "Stornieren",
    "Edit absence": "Abwesenheit bearbeiten",
    "Sick leave saved.": "Krankmeldung gespeichert.",
    "Request submitted.": "Anfrage eingereicht.",
    "Absence calendar": "Abwesenheitskalender",
    "Previous month": "Vorheriger Monat",
    "Next month": "Nächster Monat",
    Vacation: "Urlaub",
    Entitlement: "Anspruch",
    Taken: "Genommen",
    Planned: "Geplant",
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
    "Password changed.": "Passwort geändert.",
    Balance: "Saldo",
    Month: "Monat",
    Year: "Jahr",
    "Submitted entries": "Eingereichte Wochen",
    "Open requests": "Offene Anträge",
    "Submitted time entries": "Eingereichte Wochenzeiten",
    "No open entries.": "Keine offenen Wochen.",
    Approved: "Genehmigt",
    "Approved.": "Genehmigt.",
    "Approve all": "Alle genehmigen",
    "Open absence requests": "Offene Abwesenheitsanträge",
    "No open requests.": "Keine offenen Anträge.",
    "Reason: {reason}": "Begründung: {reason}",
    "Monthly report": "Monatsbericht",
    Export: "Export",
    "Export CSV": "CSV exportieren",
    "Export PDF": "PDF exportieren",
    "CSV download started.": "CSV-Download gestartet.",
    "PDF download started.": "PDF-Download gestartet.",
    Timesheet: "Stundennachweis",
    Entries: "Einträge",
    Note: "Notiz",
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
    User: "Benutzer",
    Table: "Tabelle",
    Record: "Eintrag",
    Draft: "Entwurf",
    Submitted: "Eingereicht",
    Rejected: "Abgelehnt",
    "Waiting for approval": "Warten auf Genehmigung",
    "Waiting for release": "Warten auf Freigabe",
    Partial: "Teilweise",
    Cancelled: "Storniert",
    "Cancellation pending": "Stornierung beantragt",
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
    "This Week": "Diese Woche",
    "My Balance": "Meine Bilanz",
    "My Team": "Mein Team",
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
    "Request cancellation?": "Stornierung beantragen?",
    "Request cancellation of this approved absence? Your team lead must approve the cancellation.":
      "Stornierung dieser genehmigten Abwesenheit beantragen? Die Stornierung muss vom Teamleiter genehmigt werden.",
    "Yes, request cancellation": "Ja, Stornierung beantragen",
    "Cancellation requested. Your team lead will review it.":
      "Stornierung beantragt. Dein Teamleiter wird sie prüfen.",
    "Request cancellation": "Stornierung beantragen",
    "Reject cancellation?": "Stornierung ablehnen?",
    "Reject this cancellation request? The absence will remain approved.":
      "Diese Stornierungsanfrage ablehnen? Die Abwesenheit bleibt genehmigt.",
    Cancellation: "Stornierung",
    "Approve weeks & manage requests": "Wochen genehmigen & Anträge verwalten",
    "Your overview": "Deine Übersicht",
    "Your hours overview": "Deine Stundenübersicht",
    "Pending Weeks": "Ausstehende Wochen",
    "Absence Requests": "Abwesenheitsanträge",
    "Week Approvals": "Wochen-Genehmigungen",
    "View in report": "Im Bericht ansehen",
    "Approve All": "Alle genehmigen",
    "Approve all?": "Alle genehmigen?",
    "Approve all {n} weeks across all users?":
      "Alle {n} Wochen aller Benutzer genehmigen?",
    "All caught up!": "Alles erledigt!",
    "No pending requests": "Keine ausstehenden Anträge",
    "Team hours overview": "Teamstunden-Übersicht",
    "Your profile & preferences": "Ihr Profil & Einstellungen",
    "Manage your team": "Team verwalten",
    "Add User": "Benutzer hinzufügen",
    "Edit User": "Benutzer bearbeiten",
    "Delete user?": "Benutzer löschen?",
    "Delete user permanently? All data of this user will be deleted. This cannot be undone.":
      "Benutzer dauerhaft löschen? Alle Daten dieses Benutzers werden gelöscht. Dies kann nicht rückgängig gemacht werden.",
    "Delete permanently": "Dauerhaft löschen",
    "User deleted.": "Benutzer gelöscht.",
    "User updated.": "Benutzer aktualisiert.",
    "Time Categories": "Zeitkategorien",
    "Add Category": "Kategorie hinzufügen",
    "Edit Category": "Kategorie bearbeiten",
    "Counts as work": "Zählt als Arbeitszeit",
    "Absence Categories": "Abwesenheitskategorien",
    "Add Absence Category": "Abwesenheitskategorie hinzufügen",
    "Edit Absence Category": "Abwesenheitskategorie bearbeiten",
    label_cost_type_none: "Verbraucht nichts (kein Urlaub, keine Gleitzeit)",
    label_cost_type_vacation: "Verbraucht Urlaubstage",
    label_cost_type_flextime: "Verbraucht Gleitzeitstunden",
    "Auto-approve past dates": "Vergangene Daten automatisch genehmigen",
    "Type is required.": "Typ ist erforderlich.",
    "Not enough flextime balance for this absence.":
      "Nicht genügend Gleitzeitguthaben für diese Abwesenheit.",
    "Cannot change absence category cost type (vacation ↔ flextime). Cancel and re-request with the new category.":
      "Der Kostentyp der Abwesenheitskategorie (Urlaub ↔ Gleitzeit) kann nicht geändert werden. Bitte Abwesenheit stornieren und neu beantragen.",
    "Absence category slug already exists.":
      "Abwesenheitskategorie-Slug existiert bereits.",
    "A category cannot both deduct vacation and reduce flextime.":
      "Eine Kategorie kann nicht gleichzeitig Urlaub abziehen und Gleitzeit reduzieren.",
    "Cannot change the cost or approval behavior of a category that already has absences. Deactivate this category and create a new one with the desired flags instead.":
      "Die Kosten- oder Genehmigungs-Logik einer Kategorie mit bereits vorhandenen Abwesenheiten kann nicht geändert werden. Bitte diese Kategorie deaktivieren und eine neue mit den gewünschten Eigenschaften anlegen.",
    "General Settings": "Allgemeine Einstellungen",
    General: "Allgemein",
    Organization: "Organisation",
    "Organization name": "Organisationsname",
    "e.g. My Company": "z.B. Mein Unternehmen",
    "Shown on the login screen and in the navigation.":
      "Wird auf dem Anmeldebildschirm und in der Navigation angezeigt.",
    "Save Changes": "Änderungen speichern",
    "Saving...": "Speichert...",
    "Signing in…": "Anmeldung läuft…",
    "Settings saved.": "Einstellungen gespeichert.",
    "SMTP settings saved.": "SMTP-Einstellungen gespeichert.",
    "Email (SMTP)": "E-Mail (SMTP)",
    "Enable SMTP": "SMTP aktivieren",
    "When enabled, notification emails are sent for approvals, rejections, and edit requests.":
      "Wenn aktiviert, werden Benachrichtigungs-E-Mails bei Genehmigungen, Ablehnungen und Bearbeitungsanfragen gesendet.",
    "Enable reminders": "Erinnerungen aktivieren",
    "When enabled, users who have not submitted all time entries are reminded by email on the configured deadline day.":
      "Wenn aktiviert, werden Benutzer, die noch nicht alle Wochen eingereicht haben, am konfigurierten Stichtag per E-Mail erinnert.",
    "Enable approval reminders": "Genehmigungs-Erinnerungen aktivieren",
    "When enabled, approvers are reminded by email about pending approvals every Monday.":
      "Wenn aktiviert, werden Genehmiger jeden Montag per E-Mail an ausstehende Genehmigungen erinnert.",
    "SMTP Host": "SMTP-Host",
    "SMTP Port": "SMTP-Port",
    Username: "Benutzername",
    "From address": "Absenderadresse",
    Encryption: "Verschlüsselung",
    None: "Keine",
    stored: "gespeichert",
    "Test Connection": "Verbindung testen",
    "Testing...": "Teste...",
    "SMTP connection successful.": "SMTP-Verbindung erfolgreich.",
    "SMTP enabled": "SMTP aktiviert",
    "SMTP disabled": "SMTP deaktiviert",
    "Connection OK": "Verbindung OK",
    "Not tested": "Nicht getestet",
    "SMTP connection test failed": "SMTP-Verbindungstest fehlgeschlagen",
    "Initial setup required.": "Ersteinrichtung erforderlich.",
    "Please configure the country, default weekly hours and default annual leave days before using the application.":
      "Bitte Land, Standard-Wochenstunden und Standard-Urlaubstage konfigurieren, bevor die Anwendung genutzt wird.",
    "Please enter your name and configure the country, default weekly hours and default annual leave days before using the application.":
      "Bitte geben Sie Ihren Namen ein und konfigurieren Sie Land, Standard-Wochenstunden und Standard-Urlaubstage, bevor die Anwendung genutzt wird.",
    "Please select a country.": "Bitte ein Land auswählen.",
    "Please select a region.": "Bitte eine Region auswählen.",
    "Please wait for regions to load.":
      "Bitte warten, bis die Regionen geladen sind.",
    "Could not load regions for the selected country.":
      "Regionen für das ausgewählte Land konnten nicht geladen werden.",
    "Clear stored password": "Gespeichertes Passwort löschen",
    "Please enter default weekly hours.":
      "Bitte Standard-Wochenstunden eingeben.",
    "Please enter default annual leave days.":
      "Bitte Standard-Urlaubstage eingeben.",
    "- Please select -": "- Bitte auswählen -",
    Country: "Land",
    Region: "Region",
    "Could not load regions.": "Regionen konnten nicht geladen werden.",
    "No regions available.": "Keine Regionen verfügbar.",
    "e.g. US-CA": "z.B. US-CA",
    "Audit Log": "Audit-Protokoll",
    "Holiday name": "Feiertagsname",
    "Holiday added.": "Feiertag hinzugefügt.",
    "No holidays for {year}.": "Keine Feiertage für {year}.",
    "Delete this holiday?": "Diesen Feiertag löschen?",
    "Repeats every year": "Wiederholt sich jedes Jahr",
    Recurring: "Jährlich",
    "Recurs every year.": "Wiederholt sich jährlich.",
    "Recurs until {year}.": "Wiederholt sich bis {year}.",
    "This holiday repeats every year. Deleting it removes it for every year, not only {year}.":
      "Dieser Feiertag wiederholt sich jedes Jahr. Beim Löschen wird er für alle Jahre entfernt, nicht nur für {year}.",
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
    "Reject this entry?": "Diese Woche ablehnen?",
    "Reject this request?": "Diese Anfrage ablehnen?",
    Request: "Anfrage",
    "Rejected.": "Abgelehnt.",
    Retry: "Erneut versuchen",
    // Default category names
    "Core Duties": "Kernaufgaben",
    "Preparation Time": "Vorbereitungszeit",
    "Leadership Tasks": "Leitungsaufgaben",
    "Team Meeting": "Teambesprechung",
    Other: "Sonstiges",
    "Switch to dark mode": "Dunklen Modus aktivieren",
    "Switch to light mode": "Hellen Modus aktivieren",
    Appearance: "Erscheinungsbild",
    "Dark mode": "Dunkler Modus",
    "Use dark colour scheme": "Dunkles Farbschema verwenden",
    Enabled: "Aktiviert",
    Disabled: "Deaktiviert",
    // Reopen-week feature
    Approvers: "Verantwortliche",
    "Approvers (Team leads / Admins)": "Verantwortliche Teamleitungen / Admins",
    "Approver (Team lead / Admin)": "Verantwortliche Teamleitung / Admin",
    "At least one approver is required for employees and team leads.":
      "Für Mitarbeitende und Teamleitungen ist mindestens eine verantwortliche Person erforderlich.",
    "Required for employees and team leads.":
      "Pflichtfeld für Mitarbeitende und Teamleitungen.",
    "An approver is required for employees and team leads.":
      "Für Mitarbeitende und Teamleitungen ist eine verantwortliche Person erforderlich.",
    "No eligible approvers found.":
      "Keine geeigneten Verantwortlichen gefunden.",
    "Request edit": "Bearbeitung anfordern",
    "Request edit for this week?": "Bearbeitung für diese Woche anfordern?",
    "Your team lead will be notified and must approve before the week becomes editable again.":
      "Ihre Teamleitung wird benachrichtigt und muss zustimmen, bevor die Woche wieder bearbeitet werden kann.",
    "This week will be reopened immediately for editing.":
      "Diese Woche wird sofort wieder zur Bearbeitung freigegeben.",
    "Edit request sent.": "Bearbeitungsanfrage gesendet.",
    "Week editing enabled.": "Woche zur Bearbeitung freigegeben.",
    "Edit request pending approval.":
      "Bearbeitungsanfrage wartet auf Genehmigung.",
    "Edit request approved.": "Bearbeitungsanfrage genehmigt.",
    "Edit request rejected.": "Bearbeitungsanfrage abgelehnt.",
    "Week edit requests": "Bearbeitungsanfragen",
    "Edit request": "Bearbeitungsanfrage",
    "wants to edit {week_label}": "möchte {week_label} wieder bearbeiten",
    "Team Settings": "Team-Einstellungen",
    "Allow employees to submit edit requests without approval":
      "Mitarbeitende dürfen Bearbeitungsanfragen ohne Genehmigung stellen",
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
    "Assistants must have weekly_hours set to 0.":
      "Aushilfen müssen Wochenstunden auf 0 gesetzt haben.",
    "Assistants cannot have an overtime start balance.":
      "Aushilfen dürfen keinen Überstunden-Startsaldo haben.",
    "Invalid leave_days.": "Ungültige Urlaubstage.",
    "An approver (Team lead or Admin) is required for non-admin users.":
      "Für alle Nicht-Admin-Benutzer ist eine Teamleitung oder ein Admin als verantwortliche Person erforderlich.",
    "Approver cannot be the user themselves.":
      "Verantwortliche Person darf nicht dieselbe Person sein.",
    "Approver must be an active Team lead or Admin.":
      "Verantwortliche Person muss eine aktive Teamleitung oder ein Admin sein.",
    "Approver not found.": "Verantwortliche Person nicht gefunden.",
    "Email already exists.": "E-Mail existiert bereits.",
    "First name and last name already exist.":
      "Diese Kombination aus Vorname und Nachname existiert bereits.",
    "User already exists.": "Benutzer existiert bereits.",
    "Could not create user.": "Benutzer konnte nicht angelegt werden.",
    "Could not update user.": "Benutzer konnte nicht aktualisiert werden.",
    "Email already exists or invalid approver.":
      "E-Mail existiert bereits oder verantwortliche Person ist ungültig.",
    "Could not update user (e.g. email conflict).":
      "Benutzer konnte nicht aktualisiert werden (z.B. E-Mail-Konflikt).",
    "Could not update approver.":
      "Verantwortliche Person konnte nicht aktualisiert werden.",
    "You cannot remove your own admin role.":
      "Sie können Ihre eigene Admin-Rolle nicht entfernen.",
    "You cannot delete yourself.": "Sie können sich nicht selbst löschen.",
    "Cannot delete: {count} active user(s) still have this person as their approver. Reassign them first.":
      "Löschen nicht möglich: {count} aktive Benutzer haben diese Person noch als verantwortliche Person. Weisen Sie sie zuerst neu zu.",
    "Cannot delete the last active admin.":
      "Der letzte aktive Administrator kann nicht gelöscht werden.",
    "User not found or inactive.": "Benutzer nicht gefunden oder inaktiv.",
    "Inactive users cannot log in.":
      "Inaktive Benutzer können sich nicht anmelden.",
    // tracks_time
    "Enable time tracking": "Zeiterfassung aktivieren",
    "Enable time tracking for this account":
      "Zeiterfassung für dieses Konto aktivieren",
    "Disable time tracking": "Zeiterfassung deaktivieren",
    "Disable time tracking?": "Zeiterfassung deaktivieren?",
    "When disabled, this admin works in management-only mode (no time entries or absences).":
      "Wenn deaktiviert, arbeitet dieser Admin nur in der Verwaltung (keine Zeiteinträge oder Abwesenheiten).",
    // Error notifications (admin opt-in)
    "Receives notifications about technical system errors":
      "Erhält Benachrichtigungen über technische Fehler vom System",
    "When enabled, this admin is alerted in the app and by email about technical errors.":
      "Wenn aktiviert, wird dieser Admin in der App und per E-Mail über technische Fehler benachrichtigt.",
    "Disabling time tracking will permanently delete all time entries, absences, and edit requests for this user. This cannot be undone.":
      "Das Deaktivieren der Zeiterfassung löscht dauerhaft alle Zeiteinträge, Abwesenheiten und Bearbeitungsanfragen dieses Benutzers. Diese Aktion kann nicht rückgängig gemacht werden.",
    'Type "{phrase}" to confirm': 'Geben Sie "{phrase}" zur Bestätigung ein',
    "I understand": "Ich verstehe",
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
    "An end year requires the recurring option to be enabled.":
      "Ein Endjahr setzt voraus, dass die Wiederholung aktiviert ist.",
    "The recurrence end year cannot be before the holiday's year.":
      "Das Endjahr der Wiederholung darf nicht vor dem Jahr des Feiertags liegen.",
    "Conflict: {message}": "Konflikt: {message}",
    "week_start must be a Monday (ISO).":
      "Wochenbeginn muss ein Montag sein (ISO).",
    "Cannot request edit - this week has no submitted, approved, or rejected entries.":
      "Bearbeitung nicht möglich: Diese Woche enthält keine eingereichten, genehmigten oder abgelehnten Einträge.",
    "A pending edit request already exists (id {id}).":
      "Eine offene Bearbeitungsanfrage existiert bereits (ID {id}).",
    "A pending request for this week already exists.":
      "Für diese Woche existiert bereits eine offene Anfrage.",
    "Request was already resolved by someone else.":
      "Anfrage wurde bereits von jemand anderem bearbeitet.",
    "Leave balance unavailable.": "Urlaubsstand nicht verfügbar.",
    "Overtime data unavailable.": "Überstundendaten nicht verfügbar.",
    "Overtime overview": "Überstundenübersicht",
    "This month: {value}": "Diesen Monat: {value}",
    Submissions: "Einreichungen",
    "Could not check submission status.":
      "Einreichungen konnte nicht geprüft werden.",
    "Auto-approve edit requests": "Bearbeitungsanfragen automatisch genehmigen",
    // Flextime chart
    "Flextime balance": "Gleitzeitkontostand",
    "Flextime opening balance": "Gleitzeitkontostand Anfang",
    "Flextime closing balance": "Gleitzeitkontostand Ende",
    "Daily diff": "Tagesdifferenz",
    Weekend: "Wochenende",
    Weekends: "Wochenenden",
    "Last 30 days": "Letzte 30 Tage",
    "Last 90 days": "Letzte 90 Tage",
    "Last 6 months": "Letzte 6 Monate",
    "Last year": "Letztes Jahr",
    "Custom range": "Benutzerdefinierter Zeitraum",
    Range: "Bereich",
    "From cannot be after To.": "Von kann nicht nach Bis liegen.",
    "Select an employee.": "Mitarbeiter auswählen.",
    All: "Alle",
    "CSV export is only available for a single employee.":
      "CSV-Export ist nur für einzelne Mitarbeitende möglich.",
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
    "Absence must include at least one workday.":
      "Die Abwesenheit muss mindestens einen Arbeitstag enthalten.",
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
    "Entry date is before user start date.":
      "Eintragsdatum liegt vor dem Startdatum des Benutzers.",
    "Overlap with an existing entry.":
      "Überschneidung mit einem bestehenden Eintrag.",
    "Entries in the future are not allowed.":
      "Einträge in der Zukunft sind nicht erlaubt.",
    "Editing would create overlapping draft entries.":
      "Bearbeitung würde überschneidende Entwürfe erzeugen.",
    "End time must be after start time.":
      "Endzeit muss nach der Startzeit liegen.",
    "End time cannot be in the future.":
      "Endzeit darf nicht in der Zukunft liegen.",
    "Comment too long (max 2000).": "Kommentar zu lang (max. 2000).",
    "Comment too long.": "Kommentar zu lang.",
    "Category not found.": "Kategorie nicht gefunden.",
    "Category is inactive.": "Kategorie ist inaktiv.",
    "Only drafts can be deleted.": "Nur Entwürfe können gelöscht werden.",
    "Only draft entries can be edited. Submit a week edit request to make the whole week editable again.":
      "Nur Entwürfe können direkt bearbeitet werden. Bitte fordern Sie eine Bearbeitung der Woche an, um die gesamte Woche wieder bearbeitbar zu machen.",
    "Only submitted entries can be approved.":
      "Nur eingereichte Wochen können genehmigt werden.",
    "Only submitted entries can be rejected.":
      "Nur eingereichte Wochen können abgelehnt werden.",
    "Entry was already reviewed by someone else.":
      "Woche wurde bereits von jemand anderem geprüft.",
    "Reason too long.": "Begründung zu lang.",
    "Reason required.": "Begründung erforderlich.",
    // Absence errors
    "Absence start date is before user start date.":
      "Abwesenheitsbeginn liegt vor dem Startdatum des Benutzers.",
    "Cannot edit.": "Bearbeitung nicht möglich.",
    "Absence was already reviewed by someone else.":
      "Abwesenheit wurde bereits von jemand anderem geprüft.",
    "Only requested absences can be approved.":
      "Nur beantragte Abwesenheiten können genehmigt werden.",
    "Only requested absences can be rejected.":
      "Nur beantragte Abwesenheiten können abgelehnt werden.",
    "Only requested absences and auto-approved sick absences can be cancelled.":
      "Nur beantragte oder genehmigte Abwesenheiten können storniert werden.",
    "Only requested absences can be cancelled.":
      "Nur beantragte oder genehmigte Abwesenheiten können storniert werden.",
    "Only requested or approved absences can be cancelled.":
      "Nur beantragte oder genehmigte Abwesenheiten können storniert werden.",
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
    "As of yesterday": "Stand gestern",
    help_team_report:
      "Vergleicht Soll- und Ist-Stunden aller aktiven Benutzer für den gewählten Monat. Für den laufenden Monat sind Daten inklusive heute verfügbar.",
    help_category_breakdown:
      "Zeigt die Verteilung der erfassten Stunden auf die verschiedenen Kategorien.",
    help_absence_report:
      "Zeigt Abwesenheitseinträge über einen gewählten Zeitraum mit Typverteilung. Abgelehnte und stornierte Abwesenheiten werden nicht angezeigt.",
    help_logged:
      "Eingereichte und genehmigte Stunden einschließlich des aktuellen Tages für den laufenden Monat.",
    help_employee_details:
      "Zeigt detaillierte Informationen über einen Mitarbeiter einschließlich Saldo und Statistiken.",
    help_my_balance:
      "Überblick über deinen aktuellen Gleitzeitstand und den Einreichungen. Der Gleitzeitstand wird bis einschließlich gestern berechnet; die heute geleisteten Stunden werden noch nicht mitgezählt. Die Überstundenübersicht berücksichtigt zusätzlich eingereichte, noch ausstehende Stunden.",
    help_flextime_chart:
      "Verlauf deines kumulierten Gleitzeitkontostands über den gewählten Zeitraum. Der Gleitzeitstand wird bis einschließlich gestern berechnet; die heute geleisteten Stunden werden noch nicht mitgezählt.",
    "Show explanation": "Erklärung anzeigen",
    help_cost_type_none:
      "Der Tag ist entschuldigt: Es muss keine Zeit erfasst werden, das Arbeitssoll für den Tag entfällt. Vom Urlaubskonto wird nichts abgezogen und es wird keine Gleitzeit verbraucht — die Stunden müssen also nicht nachgearbeitet werden. Wird an einem solchen Tag trotzdem Zeit gebucht (möglich bei Kategorien mit automatischer Genehmigung, z. B. vormittags gearbeitet, mittags krankgemeldet), zählen diese Stunden voll als Plus auf dem Gleitzeitkonto. Ob der Tag bezahlt wird, entscheidet nicht Zerf, sondern die Lohnabrechnung: Fortbildung ist normalerweise bezahlt, unbezahlter Urlaub nicht.",
    help_cost_type_vacation:
      "Jeder genehmigte Tag wird vom Jahresurlaub der Mitarbeitenden abgezogen — inklusive Resturlaub aus dem Vorjahr und dessen Verfallsfrist. Das Arbeitssoll für den Tag entfällt, der Gleitzeitstand bleibt unverändert.",
    help_cost_type_flextime:
      "Mitarbeitende haben frei, das Arbeitssoll für den Tag bleibt aber bestehen. Der Tag senkt den Gleitzeitstand dadurch um ein Tagessoll — so wird Gleitzeit abgebaut. Urlaubstage werden nicht verbraucht. Zerf prüft den Gleitzeitstand bei der Beantragung und noch einmal bei der Genehmigung, damit der eingestellte Mindeststand nicht unterschritten wird.",
    help_auto_approve_past:
      "Anträge mit Startdatum heute oder in der Vergangenheit werden automatisch genehmigt (ohne Freigabe durch eine vorgesetzte Person). Zeitbuchungen am selben Tag bleiben erlaubt (z. B. „vormittags gearbeitet, mittags krankgemeldet“). Rückdatieren ist auf 30 Tage begrenzt. Typische Verwendung: Krankmeldung.",
    help_submission_status:
      "Zeigt an, ob alle erforderlichen Wochen im gewählten Monat eingereicht wurden.",
    Approvals: "Genehmigungen",
    "All approved": "Alle genehmigt",
    Incomplete: "Unvollständig",
    "All submitted": "Alles eingereicht",
    "All submitted and approved": "Alles eingereicht und genehmigt",
    "All submitted (approvals pending)":
      "Alles eingereicht (Genehmigungen ausstehend)",
    "Approved: {value}": "Genehmigt: {value}",
    "Weeks missing": "Wochen fehlen",
    "Current week: still open": "Aktuelle Woche: noch offen",
    "Current week: draft": "Aktuelle Woche: Entwurf",
    "Current week: partially submitted":
      "Aktuelle Woche: teilweise eingereicht",
    "Current week: needs revision": "Aktuelle Woche: zur Überarbeitung",
    "Who is absent": "Wer ist abwesend",
    "No absences this week.": "Keine Abwesenheiten diese Woche.",
    "Employee Details": "Mitarbeiterdetails",
    "Total days": "Tage gesamt",
    Flextime: "Gleitzeit",
    "Flextime Reduction": "Gleitzeitabbau",
    Filter: "Filter",
    // Reports help (English defaults)
    // (English keys fall through)
    // Audit log
    audit_table_users: "Benutzer",
    audit_table_absences: "Abwesenheit",
    audit_table_time_entries: "Zeiteintrag",
    audit_table_time_entry_weeks: "Zeiterfassungswoche",
    audit_table_categories: "Kategorie",
    audit_table_holidays: "Feiertag",
    audit_table_sessions: "Sitzung",
    audit_table_notifications: "Benachrichtigung",
    audit_table_app_settings: "Einstellung",
    audit_table_reopen_requests: "Bearbeitungsanfrage",
    audit_action_created: "Erstellt",
    audit_action_updated: "Bearbeitet",
    audit_action_deleted: "Gelöscht",
    audit_action_approved: "Genehmigt",
    audit_action_auto_approved: "Automatisch genehmigt",
    audit_action_rejected: "Abgelehnt",
    audit_action_cancelled: "Storniert",
    audit_action_status_changed: "Status geändert",
    audit_action_team_settings_updated: "Team-Einstellung geändert",
    audit_action_password_reset: "Passwort zurückgesetzt",
    audit_action_deactivated: "Deaktiviert",
    audit_action_archived: "Archiviert",
    audit_action_restored: "Wiederhergestellt",
    audit_action_reopened: "Bearbeitung freigegeben",
    Before: "Vorher",
    After: "Nachher",
    For: "Für",
    Setting: "Einstellung",
    Value: "Wert",
    "Week start": "Wochenbeginn",
    Data: "Daten",
    Summary: "Zusammenfassung",
    // Admin settings
    "Time format": "Uhrzeitformat",
    "Default weekly hours": "Standard-Wochenstunden",
    "Default annual leave days": "Standard-Urlaubstage",
    "Generate password": "Passwort generieren",
    "Password (min 12 chars)": "Passwort (mind. 12 Zeichen)",
    "Registration email will be sent.":
      "Es wird eine Registrierungs-E-Mail gesendet.",
    "Password reset email will be sent.":
      "Es wird eine E-Mail mit dem neuen Passwort gesendet.",
    "No email was sent! Email / SMTP is not configured.":
      "Es wurde keine E-Mail gesendet! E-Mail / SMTP ist nicht konfiguriert.",
    "You must deliver this password to the user in person!":
      "Sie müssen dieses Passwort persönlich an den Benutzer übergeben!",
    "Default (all years without override)":
      "Standard (alle Jahre ohne Ausnahme)",
    "User created.": "Benutzer erstellt.",
    "Password reset.": "Passwort zurückgesetzt.",
    "Temporary password:": "Temporäres Passwort:",
    // Team Settings
    "Edit Requests": "Bearbeitungsanfragen",
    "When enabled for a user, their edit requests are automatically approved. No one is notified and no emails are sent.":
      "Wenn aktiviert, werden die Bearbeitungsanfragen des Benutzers automatisch genehmigt. Niemand wird benachrichtigt und es werden keine E-Mails versendet.",
    "Time Submissions": "Zeiteinreichungen",
    "When enabled for a user, their submitted weeks are automatically approved. No one is notified and no emails are sent.":
      "Wenn aktiviert, werden die eingereichten Wochen des Benutzers automatisch genehmigt. Niemand wird benachrichtigt und es werden keine E-Mails versendet.",
    "Auto-approve submissions": "Einreichungen automatisch genehmigen",
    // Notification polling
    // (no new keys needed)
    // Vacation carryover
    "Carryover from {year}": "Übertrag aus {year}",
    "Expired on {date}": "Verfallen am {date}",
    "Expires on {date}": "Verfällt am {date}",
    "Vacation carryover": "Urlaubsübertrag",
    "Carryover expiry date (MM-DD)": "Stichtag Urlaubsverfall (MM-TT)",
    "Unused vacation from the previous year expires on this date.":
      "Nicht genommener Urlaub aus dem Vorjahr verfällt an diesem Stichtag.",
    "Time submission deadline": "Einreichungsfrist",
    "Submission deadline day of month": "Stichtag (Tag des Monats)",
    "e.g. 5": "z.B. 5",
    "Users will be notified on this day of each month if they have unsubmitted time entries for previous months. Leave empty to disable. (1\u201328)":
      "Benutzer werden an diesem Tag jedes Monats benachrichtigt, wenn sie noch nicht eingereichte Wochen aus Vormonaten haben. Leer lassen zum Deaktivieren. (1\u201328)",
    // Auto break deduction settings
    "Automatic break deduction": "Automatischer Pausenabzug",
    "Enable automatic break deduction": "Automatischen Pausenabzug aktivieren",
    "When enabled, a break is automatically deducted from time entries that form a continuous work block meeting or exceeding the configured threshold.":
      "Wenn aktiviert, wird automatisch eine Pause von Zeiteintr\u00e4gen abgezogen, die einen zusammenh\u00e4ngenden Arbeitsblock bilden, der die konfigurierte Schwelle erreicht oder \u00fcberschreitet.",
    "Break threshold (hours)": "Pausenschwelle (Stunden)",
    "After how many consecutive hours a break is deducted.":
      "Nach wie vielen zusammenh\u00e4ngenden Arbeitsstunden eine Pause abgezogen wird.",
    "Break deduction (minutes)": "Pausenabzug (Minuten)",
    "How many minutes are deducted per qualifying work block.":
      "Wie viele Minuten pro qualifizierendem Arbeitsblock abgezogen werden.",
    "e.g. 6": "z.B. 6",
    "e.g. 30": "z.B. 30",
    "Please enter the break threshold.": "Bitte Pausenschwelle eingeben.",
    "Please enter the break deduction minutes.": "Bitte Pausenabzug eingeben.",
    "Second threshold (hours)": "Zweite Schwelle (Stunden)",
    "Optional. If the work block reaches this duration, the second deduction applies instead of the first.":
      "Optional. Wird diese Dauer erreicht, gilt der zweite Abzug anstelle des ersten.",
    "Second deduction (minutes)": "Zweiter Abzug (Minuten)",
    "Total minutes deducted when the second threshold is reached.":
      "Gesamte Minuten, die beim Erreichen der zweiten Schwelle abgezogen werden.",
    "e.g. 9 (optional)": "z. B. 9 (optional)",
    "e.g. 45 (optional)": "z. B. 45 (optional)",
    "Please enter both second threshold and second deduction, or leave both empty.":
      "Bitte beide Felder der zweiten Stufe ausfüllen oder beide leer lassen.",
    Break: "Pause",
    "Vacation days per year": "Urlaubstage pro Jahr",
    "Annual leave days (base)": "Urlaubstage allgemein",
    "Default entitlement used for every year unless overridden below (e.g. for special agreements).":
      "Grundsätzlicher Anspruch, der für jedes Jahr gilt, sofern unten nicht überschrieben (z. B. bei Sonderregelungen).",
    Override: "Abweichung",
    days: "Tage",
    workday: "Arbeitstag",
    workdays: "Arbeitstage",
    Set: "Setzen",
    "Overrides the default annual leave days for this user in the selected year.":
      "Überschreibt die Standard-Urlaubstage für diesen Benutzer im gewählten Jahr.",
    "Not enough remaining vacation days.":
      "Nicht genügend verbleibende Urlaubstage.",
    // "Not enough flextime balance..." and "Cannot change absence category
    // cost type..." are translated earlier in this block (near the absence
    // category dialog strings). Don't re-declare them — eslint no-dupe-keys.
    "Please enter vacation days.": "Bitte Urlaubstage eingeben.",
    "Absence Request Details": "Details des Abwesenheitsantrags",
    "Show details": "Details anzeigen",
    "Requested at": "Beantragt am",
    "Forgot password?": "Passwort vergessen?",
    "Enter your email to receive a password reset link.":
      "Geben Sie Ihre E-Mail-Adresse ein, um einen Link zum Zurücksetzen zu erhalten.",
    "Send reset link": "Reset-Link senden",
    "Sending...": "Wird gesendet...",
    "If your email address is registered, you will receive a reset link shortly.":
      "Falls Ihre E-Mail-Adresse registriert ist, erhalten Sie in Kürze einen Reset-Link.",
    "Back to sign in": "Zurück zur Anmeldung",
    "Choose a new password for your account.":
      "Wählen Sie ein neues Passwort für Ihr Konto.",
    "New password": "Neues Passwort",
    "Set new password": "Neues Passwort festlegen",
    "Password reset successfully. Please sign in.":
      "Passwort erfolgreich zurückgesetzt. Bitte melden Sie sich an.",
    password_reset_unavailable:
      "Passwort-Reset ist nicht verfügbar. Bitte wenden Sie sich an den Administrator.",
    reset_token_expired:
      "Dieser Reset-Link ist abgelaufen. Bitte fordern Sie einen neuen an.",
    reset_token_invalid:
      "Dieser Reset-Link ist ungültig oder wurde bereits verwendet.",
    account_deactivated:
      "Ihr Konto wurde deaktiviert. Bitte wenden Sie sich an Ihren Administrator.",
    account_archived:
      "Ihr Konto wurde archiviert. Bitte wenden Sie sich an Ihren Administrator.",
    "Account active": "Konto aktiv",
    "User activated.": "Benutzer aktiviert.",
    // Reports - labels and team report columns
    "Employee report": "Mitarbeiterbericht",
    "Export timesheet": "Export Stundennachweis",
    "Export team PDF": "Team-PDF exportieren",
    future_period_no_time_data:
      "Dieser Zeitraum liegt vollständig in der Zukunft — Stunden- und Gleitzeitdaten erscheinen, sobald er beginnt.",
    team_table_month_only:
      "Die Teamübersichtstabelle ist nur in der Monatsansicht verfügbar.",
    "Current flextime balance": "Aktueller Gleitzeitkontostand",
    "Monthly diff": "Monatsdifferenz",
    "Sick days": "Krankheitstage",
    "Vacation taken": "Urlaub genommen",
    "Vacation planned": "Urlaub geplant",
    "All weeks submitted": "Alle Wochen eingereicht",
    "Note: current month - data up to yesterday":
      "Hinweis: Laufender Monat - Daten inklusive heute",
    // Dashboard request detail labels
    Approval: "Genehmigung",
    Change: "Änderung",
    "Edit Request Details": "Details der Bearbeitungsanfrage",
    "Absence Type": "Abwesenheitstyp",
    "Request Type": "Anfragetyp",
    Changes: "Änderungen",
    "Diff unavailable for this request.":
      "Änderungen nicht verfügbar für diese Anfrage.",
    Empty: "Leer",
    Week: "Woche",
    // --- Nextcloud upload settings ---
    "Nextcloud Backups": "Nextcloud-Backup",
    "DB Backup Upload": "DB-Backup-Upload",
    "Report PDF Upload": "Stundenzettel-Export",
    "Enable DB backup upload": "DB-Backup-Upload aktivieren",
    "Enable report PDF upload": "Stundenzettel-Export aktivieren",
    "Share link (https://…/s/…)": "Share-Link (https://…/s/…)",
    "Share password (optional)": "Share-Passwort (optional)",
    "Upload day of month (1–28)": "Upload-Tag im Monat (1–28)",
    "Backup interval (days)": "Backup-Intervall (Tage)",
    "Upload now": "Jetzt hochladen",
    "Uploading...": "Wird hochgeladen...",
    "Upload settings saved.": "Upload-Einstellungen gespeichert.",
    "Report uploaded successfully.": "Bericht erfolgreich hochgeladen.",
    "Upload failed.": "Upload fehlgeschlagen.",
    "A Nextcloud share URL is required to enable database backup upload.":
      "Für die Aktivierung des Datenbank-Backup-Uploads ist eine Nextcloud-Share-URL erforderlich.",
    "The backup interval is read by the backup container from the database at the start of each cycle. Changes take effect on the next backup run. The 10 most recent local backup files are kept automatically; older ones are deleted. Uploaded files in Nextcloud are not deleted automatically.":
      "Das Backup-Intervall wird vom Backup-Container zu Beginn jedes Zyklus aus der Datenbank gelesen. Änderungen werden beim nächsten Backup-Lauf wirksam. Die 10 neuesten lokalen Backup-Dateien werden automatisch aufbewahrt; ältere werden gelöscht. Hochgeladene Dateien in Nextcloud werden nicht automatisch gelöscht.",
    "On the configured day of each month, an individual timesheet PDF is queued for every employee. Each PDF is uploaded as soon as the employee has fully submitted all their weeks — late submitters are automatically caught up on the next daily check.":
      "Am konfigurierten Tag des Monats wird für jeden Mitarbeiter ein individueller Stundenzettel in die Warteschlange eingereiht. Jedes PDF wird hochgeladen, sobald der Mitarbeiter alle Wochen vollständig eingereicht hat — spät Einreichende werden beim nächsten täglichen Lauf automatisch nachgeholt.",
    // --- Lohnmeldung ---
    "Payroll Report": "Lohnmeldung",
    "Monthly payroll report": "Monatliche Lohnmeldung",
    "Send the payroll report by email": "Lohnmeldung per E-Mail senden",
    "On the configured day of each month, the previous month's report is prepared and emailed as a PDF. It is only sent once every employee's month is final: weeks submitted, absence requests decided, and — for everyone whose hours are in the report — all time entries approved. Otherwise the report waits and is retried daily. Requires a configured email server.":
      "Am konfigurierten Tag des Monats wird die Meldung für den Vormonat erstellt und als PDF per E-Mail versendet. Sie wird erst gesendet, wenn der Monat für alle Personen abgeschlossen ist: Wochen eingereicht, Abwesenheitsanträge entschieden und — für alle, deren Stunden in der Meldung stehen — alle Zeiteinträge genehmigt. Andernfalls wartet die Meldung und wird täglich erneut geprüft. Ein eingerichteter E-Mail-Server ist erforderlich.",
    "Recipient email address": "E-Mail-Adresse des Empfängers",
    "Send day of month (1–28)": "Versandtag im Monat (1–28)",
    "Report content": "Inhalt der Meldung",
    "Absence days per employee": "Abwesenheitstage je Person",
    "One row per absence period with the number of working days. Sick days are needed for health-insurance reimbursement, unpaid days reduce the salary payout.":
      "Eine Zeile je Abwesenheitszeitraum mit der Anzahl der Arbeitstage. Krankheitstage werden für die Erstattung durch die Krankenkasse benötigt, unbezahlte Tage verringern die Lohnauszahlung.",
    "Working days and hours": "Arbeitstage und Arbeitsstunden",
    "Worked days and approved hours per person, shown in hours:minutes and as a decimal value for payroll.":
      "Gearbeitete Tage und genehmigte Stunden je Person, angegeben in Stunden:Minuten und zusätzlich als Dezimalwert für die Lohnabrechnung.",
    Assistants: "Aushilfen",
    "All other employees": "Alle übrigen Mitarbeitenden",
    inactive: "inaktiv",
    "Send now": "Jetzt senden",
    "Send now prepares the previous month immediately and sends it if the month is already final. It does not replace the scheduled monthly run.":
      "Jetzt senden erstellt den Vormonat sofort und versendet ihn, sofern der Monat bereits abgeschlossen ist. Der geplante monatliche Lauf bleibt davon unberührt.",
    "Payroll report settings saved.":
      "Einstellungen der Lohnmeldung gespeichert.",
    "Payroll report sent.": "Lohnmeldung versendet.",
    "Nothing was sent: every month was already sent or is not final yet.":
      "Es wurde nichts versendet: Alle Monate wurden bereits versendet oder sind noch nicht abgeschlossen.",
    "A recipient address is required to enable the payroll report.":
      "Für die Aktivierung der Lohnmeldung ist eine Empfängeradresse erforderlich.",
    "The payroll report is not enabled.":
      "Die Lohnmeldung ist nicht aktiviert.",
    "No recipient address configured for the payroll report.":
      "Für die Lohnmeldung ist keine Empfängeradresse konfiguriert.",
    "Email delivery is not configured; the payroll report cannot be sent.":
      "Der E-Mail-Versand ist nicht eingerichtet; die Lohnmeldung kann nicht versendet werden.",
    "Invalid payroll report recipient.":
      "Ungültige Empfängeradresse für die Lohnmeldung.",
    "payroll_report_day_of_month must be between 1 and 28.":
      "Der Versandtag muss zwischen 1 und 28 liegen.",
    "Select at least one section for the payroll report.":
      "Wählen Sie mindestens einen Abschnitt für die Lohnmeldung aus.",
    "Category not available for you.":
      "Diese Kategorie ist für Sie nicht verfügbar.",
    "Absence category not available for you.":
      "Diese Abwesenheitskategorie ist für Sie nicht verfügbar.",
    "Available to employees": "Verfügbar für Mitarbeiter",
    "Unknown employee id.": "Unbekannte Mitarbeiter-ID.",
    "Unknown category id.": "Unbekannte Kategorie-ID.",
    "Unknown absence category id.": "Unbekannte Abwesenheitskategorie-ID.",
    "Team leads": "Teamleitungen",
    "Allow team leads to create assistant users":
      "Teamleitungen erlauben, Aushilfen anzulegen",
    'When enabled, team leads get a restricted Users tab where they may only create and manage "Assistant" users assigned to them. No other role can be created there. Disabled by default.':
      'Wenn aktiviert, erhalten Teamleitungen einen eingeschränkten Benutzer-Tab, auf dem sie nur ihnen zugewiesene Benutzer mit Rolle "Aushilfe" anlegen und verwalten können. Andere Rollen können dort nicht angelegt werden. Standardmäßig deaktiviert.',
    "You can only manage assistants assigned to you.":
      "Sie können nur Ihnen zugewiesene Aushilfen verwalten.",
    "You will be set as their approver.":
      "Sie werden als deren Genehmiger festgelegt.",
    // --- User archive / restore ---
    "Archive user?": "Benutzer archivieren?",
    Archive: "Archivieren",
    "User archived.": "Benutzer archiviert.",
    "Archived Users": "Archivierte Benutzer",
    "Archived on {date}": "Archiviert am {date}",
    Restore: "Wiederherstellen",
    "Restore user?": "Benutzer wiederherstellen?",
    "User restored.": "Benutzer wiederhergestellt.",
    "No archived users.": "Keine archivierten Benutzer.",
    "This account will be deactivated and the user will no longer be able to log in. All data is preserved and the account can be restored later.":
      "Dieses Konto wird deaktiviert und der Benutzer kann sich nicht mehr anmelden. Alle Daten bleiben erhalten und das Konto kann später wiederhergestellt werden.",
    "This user approves {n} active user(s). Choose a replacement approver for each.":
      "Dieser Benutzer genehmigt {n} aktive(n) Benutzer. Wähle für jeden einen Ersatz-Genehmiger.",
    "Replacement approver for {name}": "Ersatz-Genehmiger für {name}",
    "Select approver": "Genehmiger auswählen",
    "All users must have a replacement approver assigned.":
      "Alle Benutzer müssen einen Ersatz-Genehmiger erhalten.",
    "Restore this archived account? The user will receive a temporary password and must change it on first login.":
      "Dieses archivierte Konto wiederherstellen? Der Benutzer erhält ein temporäres Passwort und muss es beim ersten Login ändern.",
    "New start date (optional)": "Neues Startdatum (optional)",
    "Reset start date to avoid flextime gap":
      "Startdatum zurücksetzen, um Gleitzeitlücke zu vermeiden",
    "Keep original start date": "Ursprüngliches Startdatum beibehalten",
    "If the account was archived for an extended period, resetting the start date prevents a large negative flextime balance from accumulating during the absence.":
      "Wenn das Konto längere Zeit archiviert war, verhindert das Zurücksetzen des Startdatums einen großen negativen Gleitzeitkontosaldo.",
    "Approver required for non-admin users.":
      "Genehmiger ist für Nicht-Admin-Benutzer erforderlich.",
    "User has historical data. Use archive instead.":
      "Benutzer hat historische Daten. Bitte stattdessen archivieren.",
    "System Log": "Systemprotokoll",
    "Log entry": "Protokolleintrag",
    "No log entries.": "Keine Protokolleinträge.",
    Warning: "Warnung",
    Source: "Quelle",
    Previous: "Zurück",
    Next: "Weiter",
    "Page {page} of {count}": "Seite {page} von {count}",
  },
};

// --- Language store ---

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

// --- Core translation helpers ---

// Replaces {placeholder} tokens in a template string with values from params.
function interpolate(template, params) {
  return template.replace(/\{(\w+)\}/g, (_, key) =>
    params[key] == null ? `{${key}}` : String(params[key]),
  );
}

export function translate(lang, key, params = {}) {
  const tpl = TRANSLATIONS[lang]?.[key] ?? key;
  return interpolate(tpl, params);
}

// --- Error message localization ---

// Regex patterns for backend error messages that carry dynamic values.
// Each entry maps a pattern to a translation key and optionally transforms
// the captured groups into interpolation params.
const ERROR_PATTERNS = Object.freeze([
  {
    pattern: /^Password must be at least (?<min>\d+) characters\.$/,
    key: "Password must be at least {min} characters.",
  },
  {
    pattern:
      /^Cannot delete: (?<count>\d+) active user\(s\) still have this person as their approver\. Reassign them first\.$/,
    key: "Cannot delete: {count} active user(s) still have this person as their approver. Reassign them first.",
  },
  {
    pattern:
      /^Cannot log time on a day with an approved absence \((?<kind>[^)]+)\)\. Please cancel or adjust the absence first\.$/,
    key: "Cannot log time on a day with an approved absence ({kind}). Please cancel or adjust the absence first.",
    params(match) {
      return { kind: absenceKindLabel(match.groups.kind) };
    },
  },
  {
    pattern: /^Invalid time: (?<time>.+)$/,
    key: "Invalid time: {time}",
  },
  {
    pattern: /^A pending edit request already exists \(id (?<id>\d+)\)\.$/,
    key: "A pending edit request already exists (id {id}).",
  },
  {
    pattern:
      /^Cannot request edit [-\u2013\u2014] this week has no submitted, approved, or rejected entries\.$/,
    key: "Cannot request edit - this week has no submitted, approved, or rejected entries.",
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

  const smtpPrefix = "SMTP_CONNECTION_FAILED:";
  if (normalized.startsWith(smtpPrefix)) {
    const detail = normalized.slice(smtpPrefix.length).trim();
    return translate(lang, "SMTP connection test failed") + ": " + detail;
  }

  return normalized;
}

// --- Reactive translation store ---

// `$t(key, params?)` is the primary translation function used in Svelte components.
export const t = derived(
  language,
  ($lang) => (key, params) => translate($lang, key, params),
);

// --- Utility exports ---

export function setLanguage(lang) {
  language.set(resolveLanguage(lang));
}
export function getLanguage() {
  return get(language);
}
export function getLocale() {
  return LANGUAGES[get(language)]?.locale || LANGUAGES[DEFAULT_LANGUAGE].locale;
}

// Format a number using the current locale's decimal separator.
// Uses Intl.NumberFormat so any locale added to LANGUAGES is handled automatically.
export function fmtDecimal(value, fractionDigits = 1) {
  return new Intl.NumberFormat(getLocale(), {
    minimumFractionDigits: fractionDigits,
    maximumFractionDigits: fractionDigits,
  }).format(value);
}

// Parse a locale-formatted decimal string back to a JS number.
// Detects the decimal separator by position: the separator that appears last is
// treated as the decimal point (e.g. "1.234,56" → comma is decimal, "1,234.56"
// → period is decimal). This makes the function accept both "2,5" and "2.5"
// regardless of the current locale, so users who accidentally type the wrong
// separator are still handled correctly.
export function parseDecimal(value) {
  if (value === "" || value == null) return NaN;
  const str = String(value).trim();
  const lastComma = str.lastIndexOf(",");
  const lastPeriod = str.lastIndexOf(".");
  if (lastComma > lastPeriod) {
    // Comma is the decimal separator (e.g. "1.234,56" or "2,57")
    return parseFloat(str.replace(/\./g, "").replace(",", "."));
  }
  // Period is the decimal separator (e.g. "1,234.56" or "2.57")
  return parseFloat(str.replace(/,/g, ""));
}

export function formatDayCount(value) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return value;
  }
  return fmtDecimal(value, value % 1 === 0 ? 0 : 1);
}

export function roleLabel(role) {
  const labels = {
    employee: "Employee",
    assistant: "Assistant",
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
    cancellation_pending: "Cancellation pending",
    open: "Open",
  };
  return translate(get(language), labels[status] || status);
}
export function hoursUnit() {
  const result = translate(get(language), "hours_unit");
  return result === "hours_unit" ? "h" : result;
}

export function formatHours(value) {
  // When a raw number is passed, apply locale-aware decimal formatting.
  // Strings (e.g. pre-formatted HH:MM values like "+5:30") are passed through as-is.
  const formatted = typeof value === "number" ? formatDayCount(value) : value;
  return translate(get(language), "{value}{unit}", {
    value: formatted,
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

// Module-level cache populated by setAbsenceCategoryCache (called from App.svelte).
// Avoids importing the Svelte store directly, which causes module-isolation
// issues in tests that vi.mock("svelte").
let _absenceCategoryCache = [];

export function setAbsenceCategoryCache(categories) {
  _absenceCategoryCache = categories || [];
}

// Render an absence category's display label for a given slug.
//
// The store-backed cache only carries ACTIVE categories (the
// `/absence-categories` endpoint that populates it intentionally hides
// inactive ones from the request dialog), so a slug from an absence whose
// category has since been deactivated would otherwise resolve to the raw
// slug. Callers that have access to the absence's stored category name
// (e.g. via audit-log payloads, calendar entry responses, etc.) should
// pass it as `fallbackName` — we then translate it via the regular table
// so seeded categories like "Vacation" still localize to "Urlaub" in German.
export function absenceKindLabel(kind, fallbackName) {
  const cat = _absenceCategoryCache.find((c) => c.slug === kind);
  if (cat) return translate(get(language), cat.name);
  if (fallbackName) return translate(get(language), fallbackName);
  return translate(get(language), kind);
}
