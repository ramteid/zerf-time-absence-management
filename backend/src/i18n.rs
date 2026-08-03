//! Backend translations for server-rendered messages.
//!
//! All language-specific data lives in the `LANGUAGES` table below.
//! To add a new language, append one entry to `LANGUAGES` -- no other
//! constants, functions, or enum variants need to change.
//! In-app notification and application-generated email copy must be defined
//! here; delivery call sites provide only event keys and dynamic parameters.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::db::DatabasePool;
use chrono::Datelike;

const DEFAULT_LANGUAGE_CODE: &str = "en";
const NOTIFICATION_EVENTS: &[&str] = &[
    "reopen_request_created",
    "reopen_approved",
    "reopen_approved_by_admin",
    "reopen_rejected",
    "reopen_rejected_by_admin",
    "absence_requested",
    "absence_updated",
    "absence_auto_approved_notice",
    "absence_approved",
    "absence_rejected",
    "absence_revoked",
    "absence_cancelled",
    "absence_cancellation_requested",
    "absence_cancellation_approved",
    "absence_cancellation_rejected",
    "timesheet_submitted",
    "timesheet_approved",
    "timesheet_rejected",
    "submission_reminder",
    "approval_reminder",
];

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
            // In-app notifications and outbound emails
            // Reopen requests
            ("notification_user_fallback", "User {user_id}"),
            ("reopen_auto_approved_title", "Week editing enabled"),
            ("reopen_auto_approved_body", "The week was reopened for editing automatically.\n\nWeek: {week_label}"),
            ("reopen_auto_approved_notice_title", "Week edit auto-approved for {requester_name}"),
            ("reopen_auto_approved_notice_body", "{requester_name}'s week edit request was auto-approved.\n\nWeek: {week_label}"),
            ("reopen_request_created_title", "New week edit request from {requester_name}"),
            ("reopen_request_created_body", "{requester_name} submitted an edit request for week {week_label}."),
            ("reopen_request_created_email_body", "Hello,\n\nA week edit request is ready for your review.\n\nEmployee: {requester_name}\nWeek: {week_label}\n\nPlease open the application to approve or reject the request."),
            ("reopen_approved_title", "Week edit request approved"),
            ("reopen_approved_body", "Your week edit request was approved.\n\nWeek: {week_label}"),
            ("reopen_approved_email_body", "Hello,\n\nYour week edit request was approved.\n\nWeek: {week_label}\n\nYou can now edit the entries for this week."),
            ("reopen_approved_by_admin_title", "Week edit request from {requester_name} approved by admin"),
            ("reopen_approved_by_admin_body", "The week edit request from {requester_name} was approved by an admin.\n\nWeek: {week_label}"),
            ("reopen_approved_by_admin_email_body", "Hello,\n\nAn administrator approved a week edit request.\n\nEmployee: {requester_name}\nWeek: {week_label}\n\nNo further action is required."),
            ("reopen_rejected_title", "Week edit request rejected"),
            ("reopen_rejected_body", "Your week edit request was rejected.\n\nWeek: {week_label}\nReason: {reason}"),
            ("reopen_rejected_email_body", "Hello,\n\nYour week edit request was rejected.\n\nWeek: {week_label}\nReason: {reason}\n\nYour entries remain unchanged."),
            ("reopen_rejected_by_admin_title", "Week edit request from {requester_name} rejected by admin"),
            ("reopen_rejected_by_admin_body", "The week edit request from {requester_name} was rejected by an admin.\n\nWeek: {week_label}\nReason: {reason}"),
            ("reopen_rejected_by_admin_email_body", "Hello,\n\nAn administrator rejected a week edit request.\n\nEmployee: {requester_name}\nWeek: {week_label}\nReason: {reason}\n\nNo further action is required."),
            ("reopen_superseded_reason", "Superseded by a new week submission."),
            // Absences
            ("absence_kind_vacation", "Vacation"),
            ("absence_kind_sick", "Sick"),
            ("absence_kind_training", "Training"),
            ("absence_kind_special_leave", "Special leave"),
            ("absence_kind_unpaid", "Unpaid"),
            ("absence_kind_general_absence", "General absence"),
            ("absence_kind_flextime_reduction", "Flextime Reduction"),
            ("absence_requested_title", "New absence request from {requester_name}"),
            ("absence_requested_body", "{requester_name} requested a {kind} absence.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_requested_email_body", "Hello,\n\nAn absence request is ready for your review.\n\nEmployee: {requester_name}\nType: {kind}\nPeriod: {start_date} to {end_date}\n\nPlease open the application to approve or reject the request."),
            ("absence_updated_title", "Absence request from {requester_name} updated"),
            ("absence_updated_body", "{requester_name} updated their {kind} absence request.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_updated_email_body", "Hello,\n\nAn absence request awaiting your review was updated.\n\nEmployee: {requester_name}\nType: {kind}\nPeriod: {start_date} to {end_date}\n\nPlease open the application to review the changes."),
            ("absence_auto_approved_notice_title", "Absence recorded for {requester_name}"),
            ("absence_auto_approved_notice_body", "{requester_name}'s {kind} absence was approved automatically.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_auto_approved_notice_email_body", "Hello,\n\nAn absence was recorded and approved automatically.\n\nEmployee: {requester_name}\nType: {kind}\nPeriod: {start_date} to {end_date}\n\nNo action is required."),
            ("absence_approved_title", "Absence approved"),
            ("absence_approved_body", "Your {kind} absence has been approved.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_approved_email_body", "Hello,\n\nYour absence was approved.\n\nType: {kind}\nPeriod: {start_date} to {end_date}"),
            ("absence_rejected_title", "Absence rejected"),
            ("absence_rejected_body", "Your {kind} absence request was rejected.\n\nPeriod: {start_date} to {end_date}\nReason: {reason}"),
            ("absence_rejected_email_body", "Hello,\n\nYour absence request was rejected.\n\nType: {kind}\nPeriod: {start_date} to {end_date}\nReason: {reason}"),
            ("absence_revoked_title", "Absence revoked"),
            ("absence_revoked_body", "Your {kind} absence was revoked by an administrator.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_revoked_email_body", "Hello,\n\nYour absence was revoked by an administrator.\n\nType: {kind}\nPeriod: {start_date} to {end_date}"),
            ("absence_cancelled_title", "Absence request from {requester_name} withdrawn"),
            ("absence_cancelled_body", "{requester_name} withdrew their {kind} absence request.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_cancelled_email_body", "Hello,\n\nAn absence request was withdrawn.\n\nEmployee: {requester_name}\nType: {kind}\nPeriod: {start_date} to {end_date}\n\nNo action is required."),
            ("absence_cancellation_requested_title", "Absence cancellation requested by {requester_name}"),
            ("absence_cancellation_requested_body", "{requester_name} requested cancellation of their {kind} absence.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_cancellation_requested_email_body", "Hello,\n\nAn absence cancellation is ready for your review.\n\nEmployee: {requester_name}\nType: {kind}\nPeriod: {start_date} to {end_date}\n\nPlease open the application to approve or reject the cancellation."),
            ("absence_cancellation_approved_title", "Absence cancellation approved"),
            ("absence_cancellation_approved_body", "Your {kind} cancellation request was approved.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_cancellation_approved_email_body", "Hello,\n\nYour absence cancellation was approved.\n\nType: {kind}\nPeriod: {start_date} to {end_date}"),
            ("absence_cancellation_rejected_title", "Absence cancellation rejected"),
            ("absence_cancellation_rejected_body", "Your {kind} cancellation request was rejected.\n\nPeriod: {start_date} to {end_date}"),
            ("absence_cancellation_rejected_email_body", "Hello,\n\nYour absence cancellation was rejected.\n\nType: {kind}\nPeriod: {start_date} to {end_date}\n\nThe absence remains approved."),
            // Timesheets and reminders
            ("timesheet_submitted_title", "{submitter_name} submitted {week_count}"),
            ("timesheet_submitted_body", "Submitted for approval:\n{week_list}"),
            ("timesheet_submitted_email_body", "Hello,\n\nA timesheet is ready for your review.\n\nEmployee: {submitter_name}\nWeeks:\n{week_list}\n\nPlease open the application to approve or reject the timesheet."),
            ("timesheet_approved_title", "{week_count} approved"),
            ("timesheet_approved_body", "Approved:\n{week_list}"),
            ("timesheet_approved_email_body", "Hello,\n\nYour timesheet was approved.\n\nWeeks:\n{week_list}"),
            ("timesheet_rejected_title", "{week_count} rejected"),
            ("timesheet_rejected_body", "Rejected:\n{week_list}\nReason: {reason}"),
            ("timesheet_rejected_email_body", "Hello,\n\nYour timesheet was rejected.\n\nWeeks:\n{week_list}\nReason: {reason}\n\nPlease update the affected entries before submitting them again."),
            ("submission_reminder_title", "Weeks not yet submitted"),
            ("submission_reminder_body", "You still have weeks that are not submitted.\n\nWeeks: {weeks}"),
            ("submission_reminder_email_body", "Hello,\n\nThe following weeks have not been submitted:\n\n{weeks}\n\nPlease open the application and submit them."),
            ("approval_reminder_title", "Pending approvals"),
            ("approval_reminder_body", "You have pending requests awaiting your approval.\n\nOpen items: {count}"),
            ("approval_reminder_email_body", "Hello,\n\nYou have requests waiting for your review.\n\nPending requests: {count}\n\nPlease open the application to review them."),
            // Transactional account emails
            ("email_default_organization_name", "Application"),
            ("email_login_url_line", "\nSign-in URL: {app_url}\n"),
            ("email_footer_with_url", "{body}\n\n{timestamp}\n\n{app_url}"),
            ("email_footer_without_url", "{body}\n\n{timestamp}"),
            ("password_reset_subject", "Reset your password"),
            ("password_reset_body", "Hello,\n\nWe received a request to reset your password.\n\nReset link (valid for 1 hour):\n{reset_link}\n\nIf you did not request this, you can ignore this email."),
            ("admin_password_reset_subject", "Your temporary password - {org_name}"),
            ("admin_password_reset_body", "Hello {first_name} {last_name},\n\nAn administrator reset your password.\n\nAccount: {email}\nTemporary password: {password}{login_line}\nFor your security, sign in and choose a new password immediately."),
            ("account_created_subject", "Your account - {org_name}"),
            ("account_created_body", "Hello {first_name} {last_name},\n\nYour account for {org_name} has been created.\n\nAccount: {email}\nTemporary password: {password}{login_line}\nFor your security, sign in and choose a new password immediately."),
            // Technical error notifications
            ("error_notification_title", "Technical system error"),
            ("error_notification_body", "Source: Application\nDetails: {details}"),
            ("technical_error_email_body", "Hello,\n\nThe application detected a technical issue.\n\nSummary: {title}\n\n{details}\n\nPlease review the system log and take action if needed."),
            ("backup_failed_title", "Database backup failed"),
            ("backup_failed_body", "Component: Database backup\nAction: Review the backup container logs."),
            ("backup_upload_failed_title", "Nextcloud backup upload failed"),
            ("backup_upload_failed_body", "Component: Nextcloud backup upload\nAction: Review the backup container logs."),
            ("backup_unknown_error_title", "Backup process failed"),
            ("backup_unknown_error_body", "Component: Backup process\nError code: {error_code}\nAction: Review the backup container logs."),
            ("report_upload_blocked_title", "Report PDF upload blocked"),
            ("report_upload_pre_start_review_body", "User: {first_name} {last_name} (ID {user_id})\nPeriod: {period}\nCurrent start date: {start_date}\nIssue: The start date falls within or after this period, and the change still requires review.\nAction: Confirm the start date and historical entries, then retry the PDF export."),
            ("report_upload_pre_start_content_body", "User: {first_name} {last_name} (ID {user_id})\nPeriod: {period}\nCurrent start date: {start_date}\nIssue: Stored report rows exist before the current start date.\nAction: Correct the start date or historical entries, then retry the PDF export."),
            ("report_upload_unsettled_time_body", "User: {first_name} {last_name} (ID {user_id})\nPeriod: {period}\nIssue: The account is archived or time tracking is disabled, but unresolved time entries remain.\nAction: Resolve draft, submitted, and rejected entries, then retry the PDF export."),
            // Monthly payroll report
            ("payroll_report_email_subject", "Payroll report {period} - {org_name}"),
            ("payroll_report_email_body", "Hello,\n\nattached you will find the payroll report for {period} from {org_name}.\n\nIt lists the absence days per employee and the working days and hours for the selected groups.\n\nThis email was generated automatically."),
            ("payroll_report_email_manual_note", "\n\nNote: this report was sent manually via \"Send now\" in Zerf. It does not replace the regular automatic delivery for this month, which is still scheduled to go out separately."),
            ("payroll_report_blocked_title", "Payroll report not sent yet"),
            ("payroll_report_blocked_body", "Component: Payroll report\nPeriod: {period}\nIssue: The month is not final for the following people:\n{employees}\nAction: Complete and approve the open entries; the report is sent automatically on the next daily check."),
            ("payroll_report_blocked_more", "\n- and {count} more"),
            ("payroll_report_reason_not_submitted", "weeks not fully submitted"),
            ("payroll_report_reason_pending_absences", "absence request still undecided"),
            ("payroll_report_reason_unresolved_entries", "archived account with unresolved time entries"),
            ("payroll_report_reason_unapproved_entries", "time entries not approved yet"),
            ("payroll_report_reason_pre_start_content", "stored data lies before the current start date"),
            // PDF copy
            ("weekday_monday", "Monday"),
            ("weekday_tuesday", "Tuesday"),
            ("weekday_wednesday", "Wednesday"),
            ("weekday_thursday", "Thursday"),
            ("weekday_friday", "Friday"),
            ("weekday_saturday", "Saturday"),
            ("weekday_sunday", "Sunday"),
            ("pdf_timesheet_title", "Timesheet"),
            ("pdf_column_date", "Date"),
            ("pdf_column_weekday", "Weekday"),
            ("pdf_column_start", "Start"),
            ("pdf_column_end", "End"),
            ("pdf_column_category", "Category"),
            ("pdf_column_duration", "Duration"),
            ("pdf_column_status", "Status"),
            ("pdf_column_absence", "Absence"),
            ("pdf_column_holiday", "Holiday"),
            ("pdf_total", "Total (approved)"),
            ("pdf_flextime_opening_balance", "Flextime opening balance"),
            ("pdf_flextime_closing_balance", "Flextime closing balance"),
            // Short status labels for individual time-entry rows. These appear
            // in the Status column so readers can reconcile per-row Duration
            // values against the Total row, which counts only approved,
            // work-crediting, break-adjusted minutes.
            ("pdf_status_draft",       "Draft"),
            ("pdf_status_submitted",   "Submitted"),
            ("pdf_status_approved",    "Approved"),
            // Approved but non-crediting: minutes show in Duration, never in Total.
            ("pdf_status_approved_nc", "Approv. (nc)"),
            ("pdf_status_other",       ""),
            // Payroll report PDF
            ("pdf_payroll_title", "Payroll report"),
            ("pdf_payroll_absences_heading", "Absence days"),
            ("pdf_payroll_assistant_hours_heading", "Working days and hours - assistants"),
            ("pdf_payroll_employee_hours_heading", "Working days and hours - employees"),
            ("pdf_payroll_column_employee", "Employee"),
            ("pdf_payroll_column_from", "From"),
            ("pdf_payroll_column_to", "To"),
            ("pdf_payroll_column_days", "Days"),
            ("pdf_payroll_column_work_days", "Work days"),
            ("pdf_payroll_column_hours", "Hours"),
            ("pdf_payroll_column_hours_decimal", "Hours (decimal)"),
            ("pdf_payroll_total", "Total"),
            ("pdf_payroll_no_rows", "No entries in this period."),
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
            // In-App-Benachrichtigungen und E-Mails
            // Bearbeitungsanfragen
            ("notification_user_fallback", "Benutzer {user_id}"),
            ("reopen_auto_approved_title", "Woche zur Bearbeitung freigegeben"),
            ("reopen_auto_approved_body", "Die Woche wurde automatisch zur Bearbeitung freigegeben.\n\nWoche: {week_label}"),
            ("reopen_auto_approved_notice_title", "Bearbeitungsanfrage von {requester_name} automatisch genehmigt"),
            ("reopen_auto_approved_notice_body", "Die Bearbeitungsanfrage von {requester_name} wurde automatisch genehmigt.\n\nWoche: {week_label}"),
            ("reopen_request_created_title", "Neue Bearbeitungsanfrage von {requester_name}"),
            ("reopen_request_created_body", "{requester_name} hat eine Bearbeitungsanfrage f\u{00fc}r Woche {week_label} gestellt."),
            ("reopen_request_created_email_body", "Hallo,\n\neine Bearbeitungsanfrage f\u{00fc}r eine Woche wartet auf Ihre Pr\u{00fc}fung.\n\nMitarbeitende Person: {requester_name}\nWoche: {week_label}\n\nBitte \u{00f6}ffnen Sie die Anwendung und genehmigen Sie die Anfrage oder lehnen Sie sie ab."),
            ("reopen_approved_title", "Bearbeitungsanfrage genehmigt"),
            ("reopen_approved_body", "Ihre Bearbeitungsanfrage wurde genehmigt.\n\nWoche: {week_label}"),
            ("reopen_approved_email_body", "Hallo,\n\nIhre Bearbeitungsanfrage wurde genehmigt.\n\nWoche: {week_label}\n\nSie k\u{00f6}nnen die Eintr\u{00e4}ge dieser Woche jetzt bearbeiten."),
            ("reopen_approved_by_admin_title", "Bearbeitungsanfrage von {requester_name} durch Admin genehmigt"),
            ("reopen_approved_by_admin_body", "Die Bearbeitungsanfrage von {requester_name} wurde von einem Admin genehmigt.\n\nWoche: {week_label}"),
            ("reopen_approved_by_admin_email_body", "Hallo,\n\neine Bearbeitungsanfrage wurde von einem Administrator genehmigt.\n\nMitarbeitende Person: {requester_name}\nWoche: {week_label}\n\nEs ist keine weitere Aktion erforderlich."),
            ("reopen_rejected_title", "Bearbeitungsanfrage abgelehnt"),
            ("reopen_rejected_body", "Ihre Bearbeitungsanfrage wurde abgelehnt.\n\nWoche: {week_label}\nGrund: {reason}"),
            ("reopen_rejected_email_body", "Hallo,\n\nIhre Bearbeitungsanfrage wurde abgelehnt.\n\nWoche: {week_label}\nGrund: {reason}\n\nIhre Eintr\u{00e4}ge bleiben unver\u{00e4}ndert."),
            ("reopen_rejected_by_admin_title", "Bearbeitungsanfrage von {requester_name} durch Admin abgelehnt"),
            ("reopen_rejected_by_admin_body", "Die Bearbeitungsanfrage von {requester_name} wurde von einem Admin abgelehnt.\n\nWoche: {week_label}\nGrund: {reason}"),
            ("reopen_rejected_by_admin_email_body", "Hallo,\n\neine Bearbeitungsanfrage wurde von einem Administrator abgelehnt.\n\nMitarbeitende Person: {requester_name}\nWoche: {week_label}\nGrund: {reason}\n\nEs ist keine weitere Aktion erforderlich."),
            ("reopen_superseded_reason", "Durch eine neue Wocheneinreichung ersetzt."),
            // Abwesenheiten
            ("absence_kind_vacation", "Urlaub"),
            ("absence_kind_sick", "Krankmeldung"),
            ("absence_kind_training", "Fortbildung"),
            ("absence_kind_special_leave", "Sonderurlaub"),
            ("absence_kind_unpaid", "Unbezahlter Urlaub"),
            ("absence_kind_general_absence", "Allgemeine Abwesenheit"),
            ("absence_kind_flextime_reduction", "Gleitzeitabbau"),
            ("absence_requested_title", "Neue Abwesenheitsanfrage von {requester_name}"),
            ("absence_requested_body", "{requester_name}: {kind}\n\nZeitraum: {start_date} bis {end_date}"),
            ("absence_requested_email_body", "Hallo,\n\neine Abwesenheitsanfrage wartet auf Ihre Pr\u{00fc}fung.\n\nMitarbeitende Person: {requester_name}\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\n\nBitte \u{00f6}ffnen Sie die Anwendung und genehmigen Sie die Anfrage oder lehnen Sie sie ab."),
            ("absence_updated_title", "Abwesenheitsanfrage von {requester_name} aktualisiert"),
            ("absence_updated_body", "{requester_name} hat die Anfrage aktualisiert.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_updated_email_body", "Hallo,\n\neine Abwesenheitsanfrage, die auf Ihre Pr\u{00fc}fung wartet, wurde aktualisiert.\n\nMitarbeitende Person: {requester_name}\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\n\nBitte \u{00f6}ffnen Sie die Anwendung und pr\u{00fc}fen Sie die \u{00c4}nderungen."),
            ("absence_auto_approved_notice_title", "Abwesenheit von {requester_name} erfasst"),
            ("absence_auto_approved_notice_body", "{requester_name}: {kind} automatisch genehmigt.\n\nZeitraum: {start_date} bis {end_date}"),
            ("absence_auto_approved_notice_email_body", "Hallo,\n\neine Abwesenheit wurde erfasst und automatisch genehmigt.\n\nMitarbeitende Person: {requester_name}\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\n\nEs ist keine Aktion erforderlich."),
            ("absence_approved_title", "Abwesenheit genehmigt"),
            ("absence_approved_body", "Art: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_approved_email_body", "Hallo,\n\nIhre Abwesenheit wurde genehmigt.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_rejected_title", "Abwesenheit abgelehnt"),
            ("absence_rejected_body", "Art: {kind}\nZeitraum: {start_date} bis {end_date}\nGrund: {reason}"),
            ("absence_rejected_email_body", "Hallo,\n\nIhre Abwesenheitsanfrage wurde abgelehnt.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\nGrund: {reason}"),
            ("absence_revoked_title", "Abwesenheit widerrufen"),
            ("absence_revoked_body", "Art: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_revoked_email_body", "Hallo,\n\nIhre Abwesenheit wurde von einem Administrator widerrufen.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_cancelled_title", "Abwesenheitsantrag von {requester_name} zur\u{00fc}ckgezogen"),
            ("absence_cancelled_body", "{requester_name} hat die Anfrage zur\u{00fc}ckgezogen.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_cancelled_email_body", "Hallo,\n\neine Abwesenheitsanfrage wurde zur\u{00fc}ckgezogen.\n\nMitarbeitende Person: {requester_name}\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\n\nEs ist keine Aktion erforderlich."),
            ("absence_cancellation_requested_title", "Stornierungsanfrage f\u{00fc}r Abwesenheit von {requester_name}"),
            ("absence_cancellation_requested_body", "{requester_name} m\u{00f6}chte die Abwesenheit stornieren.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_cancellation_requested_email_body", "Hallo,\n\neine Stornierungsanfrage f\u{00fc}r eine Abwesenheit wartet auf Ihre Pr\u{00fc}fung.\n\nMitarbeitende Person: {requester_name}\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\n\nBitte \u{00f6}ffnen Sie die Anwendung und genehmigen Sie die Stornierung oder lehnen Sie sie ab."),
            ("absence_cancellation_approved_title", "Stornierung genehmigt"),
            ("absence_cancellation_approved_body", "Art: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_cancellation_approved_email_body", "Hallo,\n\ndie Stornierung Ihrer Abwesenheit wurde genehmigt.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_cancellation_rejected_title", "Stornierung abgelehnt"),
            ("absence_cancellation_rejected_body", "Art: {kind}\nZeitraum: {start_date} bis {end_date}"),
            ("absence_cancellation_rejected_email_body", "Hallo,\n\ndie Stornierung Ihrer Abwesenheit wurde abgelehnt.\n\nArt: {kind}\nZeitraum: {start_date} bis {end_date}\n\nDie Abwesenheit bleibt genehmigt."),
            // Stundenzettel und Erinnerungen
            ("timesheet_submitted_title", "{submitter_name} hat {week_count} eingereicht"),
            ("timesheet_submitted_body", "Zur Genehmigung eingereicht:\n{week_list}"),
            ("timesheet_submitted_email_body", "Hallo,\n\nein Stundenzettel wartet auf Ihre Pr\u{00fc}fung.\n\nMitarbeitende Person: {submitter_name}\nWochen:\n{week_list}\n\nBitte \u{00f6}ffnen Sie die Anwendung und genehmigen Sie den Stundenzettel oder lehnen Sie ihn ab."),
            ("timesheet_approved_title", "{week_count} genehmigt"),
            ("timesheet_approved_body", "Genehmigt:\n{week_list}"),
            ("timesheet_approved_email_body", "Hallo,\n\nIhr Stundenzettel wurde genehmigt.\n\nWochen:\n{week_list}"),
            ("timesheet_rejected_title", "{week_count} abgelehnt"),
            ("timesheet_rejected_body", "Abgelehnt:\n{week_list}\nGrund: {reason}"),
            ("timesheet_rejected_email_body", "Hallo,\n\nIhr Stundenzettel wurde abgelehnt.\n\nWochen:\n{week_list}\nGrund: {reason}\n\nBitte korrigieren Sie die betroffenen Eintr\u{00e4}ge, bevor Sie sie erneut einreichen."),
            ("submission_reminder_title", "Arbeitszeiten noch nicht eingereicht"),
            ("submission_reminder_body", "Sie haben noch nicht eingereichte Wochen.\n\nWochen: {weeks}"),
            ("submission_reminder_email_body", "Hallo,\n\ndie folgenden Wochen wurden noch nicht eingereicht:\n\n{weeks}\n\nBitte \u{00f6}ffnen Sie die Anwendung und reichen Sie sie ein."),
            ("approval_reminder_title", "Offene Genehmigungen"),
            ("approval_reminder_body", "Es gibt offene Anfragen, die Ihre Genehmigung erfordern.\n\nOffene Vorg\u{00e4}nge: {count}"),
            ("approval_reminder_email_body", "Hallo,\n\nAnfragen warten auf Ihre Pr\u{00fc}fung.\n\nOffene Anfragen: {count}\n\nBitte \u{00f6}ffnen Sie die Anwendung und pr\u{00fc}fen Sie sie."),
            // Transaktionale Konto-E-Mails
            ("email_default_organization_name", "Anwendung"),
            ("email_login_url_line", "\nAnmelde-URL: {app_url}\n"),
            ("email_footer_with_url", "{body}\n\n{timestamp}\n\n{app_url}"),
            ("email_footer_without_url", "{body}\n\n{timestamp}"),
            ("password_reset_subject", "Ihr Passwort zur\u{00fc}cksetzen"),
            ("password_reset_body", "Hallo,\n\nwir haben eine Anfrage zum Zur\u{00fc}cksetzen Ihres Passworts erhalten.\n\nLink zum Zur\u{00fc}cksetzen (1 Stunde g\u{00fc}ltig):\n{reset_link}\n\nFalls Sie diese Anfrage nicht gestellt haben, k\u{00f6}nnen Sie diese E-Mail ignorieren."),
            ("admin_password_reset_subject", "Ihr vorl\u{00e4}ufiges Passwort - {org_name}"),
            ("admin_password_reset_body", "Hallo {first_name} {last_name},\n\nein Administrator hat Ihr Passwort zur\u{00fc}ckgesetzt.\n\nKonto: {email}\nVorl\u{00e4}ufiges Passwort: {password}{login_line}\nMelden Sie sich zu Ihrer Sicherheit an und vergeben Sie sofort ein neues Passwort."),
            ("account_created_subject", "Ihr Konto - {org_name}"),
            ("account_created_body", "Hallo {first_name} {last_name},\n\nIhr Konto f\u{00fc}r {org_name} wurde erstellt.\n\nKonto: {email}\nVorl\u{00e4}ufiges Passwort: {password}{login_line}\nMelden Sie sich zu Ihrer Sicherheit an und vergeben Sie sofort ein neues Passwort."),
            // Technische Fehlermeldungen
            ("error_notification_title", "Technischer Systemfehler"),
            ("error_notification_body", "Quelle: Anwendung\nDetails: {details}"),
            ("technical_error_email_body", "Hallo,\n\ndie Anwendung hat ein technisches Problem erkannt.\n\nZusammenfassung: {title}\n\n{details}\n\nBitte pr\u{00fc}fen Sie das Systemprotokoll und handeln Sie bei Bedarf."),
            ("backup_failed_title", "Datenbank-Backup fehlgeschlagen"),
            ("backup_failed_body", "Komponente: Datenbank-Backup\nAktion: Pr\u{00fc}fen Sie die Protokolle des Backup-Containers."),
            ("backup_upload_failed_title", "Nextcloud-Upload des Backups fehlgeschlagen"),
            ("backup_upload_failed_body", "Komponente: Nextcloud-Upload des Backups\nAktion: Pr\u{00fc}fen Sie die Protokolle des Backup-Containers."),
            ("backup_unknown_error_title", "Backup-Prozess fehlgeschlagen"),
            ("backup_unknown_error_body", "Komponente: Backup-Prozess\nFehlercode: {error_code}\nAktion: Pr\u{00fc}fen Sie die Protokolle des Backup-Containers."),
            ("report_upload_blocked_title", "Stundenzettel-PDF-Upload blockiert"),
            ("report_upload_pre_start_review_body", "Person: {first_name} {last_name} (ID {user_id})\nZeitraum: {period}\nAktuelles Startdatum: {start_date}\nProblem: Das Startdatum liegt in oder nach diesem Zeitraum und die \u{00c4}nderung muss noch gepr\u{00fc}ft werden.\nAktion: Pr\u{00fc}fen Sie das Startdatum und die historischen Eintr\u{00e4}ge und starten Sie den PDF-Export erneut."),
            ("report_upload_pre_start_content_body", "Person: {first_name} {last_name} (ID {user_id})\nZeitraum: {period}\nAktuelles Startdatum: {start_date}\nProblem: Gespeicherte Berichtsdaten liegen vor dem aktuellen Startdatum.\nAktion: Korrigieren Sie das Startdatum oder die historischen Eintr\u{00e4}ge und starten Sie den PDF-Export erneut."),
            ("report_upload_unsettled_time_body", "Person: {first_name} {last_name} (ID {user_id})\nZeitraum: {period}\nProblem: Das Konto ist archiviert oder die Zeiterfassung ist deaktiviert, aber es gibt noch ungekl\u{00e4}rte Zeiteintr\u{00e4}ge.\nAktion: Kl\u{00e4}ren Sie Eintr\u{00e4}ge im Entwurf, eingereichte und abgelehnte Eintr\u{00e4}ge und starten Sie den PDF-Export erneut."),
            // Monatliche Lohnmeldung
            ("payroll_report_email_subject", "Lohnmeldung {period} - {org_name}"),
            ("payroll_report_email_body", "Hallo,\n\nim Anhang finden Sie die Lohnmeldung f\u{00fc}r {period} von {org_name}.\n\nDiese E-Mail wurde automatisch erstellt."),
            ("payroll_report_email_manual_note", "\n\nHinweis: Dieser Bericht wurde manuell \u{00fc}ber \"Jetzt senden\" in Zerf versendet. Er ersetzt nicht den regul\u{00e4}ren automatischen Versand f\u{00fc}r diesen Monat, der weiterhin separat erfolgt."),
            ("payroll_report_blocked_title", "Lohnmeldung noch nicht versendet"),
            ("payroll_report_blocked_body", "Komponente: Lohnmeldung\nZeitraum: {period}\nProblem: Der Monat ist f\u{00fc}r folgende Personen noch nicht abgeschlossen:\n{employees}\nAktion: Kl\u{00e4}ren und genehmigen Sie die offenen Eintr\u{00e4}ge; die Meldung wird bei der n\u{00e4}chsten t\u{00e4}glichen Pr\u{00fc}fung automatisch versendet."),
            ("payroll_report_blocked_more", "\n- und {count} weitere"),
            ("payroll_report_reason_not_submitted", "Wochen nicht vollst\u{00e4}ndig eingereicht"),
            ("payroll_report_reason_pending_absences", "Abwesenheitsantrag noch offen"),
            ("payroll_report_reason_unresolved_entries", "archiviertes Konto mit ungekl\u{00e4}rten Zeiteintr\u{00e4}gen"),
            ("payroll_report_reason_unapproved_entries", "Zeiteintr\u{00e4}ge noch nicht genehmigt"),
            ("payroll_report_reason_pre_start_content", "gespeicherte Daten liegen vor dem aktuellen Startdatum"),
            // PDF-Texte
            ("weekday_monday", "Montag"),
            ("weekday_tuesday", "Dienstag"),
            ("weekday_wednesday", "Mittwoch"),
            ("weekday_thursday", "Donnerstag"),
            ("weekday_friday", "Freitag"),
            ("weekday_saturday", "Samstag"),
            ("weekday_sunday", "Sonntag"),
            ("pdf_timesheet_title", "Stundennachweis"),
            ("pdf_column_date", "Datum"),
            ("pdf_column_weekday", "Wochentag"),
            ("pdf_column_start", "Start"),
            ("pdf_column_end", "Ende"),
            ("pdf_column_category", "Kategorie"),
            ("pdf_column_duration", "Dauer"),
            ("pdf_column_status", "Status"),
            ("pdf_column_absence", "Abwesenheit"),
            ("pdf_column_holiday", "Feiertag"),
            ("pdf_total", "Gesamt (genehmigt)"),
            ("pdf_flextime_opening_balance", "Gleitzeitkontostand Anfang"),
            ("pdf_flextime_closing_balance", "Gleitzeitkontostand Ende"),
            ("pdf_status_draft",       "Entwurf"),
            ("pdf_status_submitted",   "Eingereicht"),
            ("pdf_status_approved",    "Genehmigt"),
            ("pdf_status_approved_nc", "Genehm. (nk)"),
            ("pdf_status_other",       ""),
            // Lohnmeldungs-PDF
            ("pdf_payroll_title", "Lohnmeldung"),
            ("pdf_payroll_absences_heading", "Abwesenheitstage"),
            ("pdf_payroll_assistant_hours_heading", "Arbeitstage und Arbeitsstunden - Aushilfen"),
            ("pdf_payroll_employee_hours_heading", "Arbeitstage und Arbeitsstunden - Mitarbeitende"),
            ("pdf_payroll_column_employee", "Person"),
            ("pdf_payroll_column_from", "Von"),
            ("pdf_payroll_column_to", "Bis"),
            ("pdf_payroll_column_days", "Tage"),
            ("pdf_payroll_column_work_days", "Arbeitstage"),
            ("pdf_payroll_column_hours", "Stunden"),
            ("pdf_payroll_column_hours_decimal", "Stunden (dezimal)"),
            ("pdf_payroll_total", "Gesamt"),
            ("pdf_payroll_no_rows", "Keine Eintr\u{00e4}ge in diesem Zeitraum."),
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
    let tz = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
    let local = value.with_timezone(&tz);
    if language.code() == "de" {
        local.format("%d.%m.%Y %H:%M").to_string()
    } else {
        local.format("%m/%d/%Y %H:%M").to_string()
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

/// Fully rendered title or subject and body for a notification or
/// application-generated email.
pub struct NotificationText {
    pub title: String,
    pub body: String,
}

/// Render notification or email copy from explicit central translation keys.
pub fn notification_text(
    language: &Language,
    title_key: &str,
    body_key: &str,
    params: &[(&str, String)],
) -> NotificationText {
    NotificationText {
        title: required_translation(language, title_key, params),
        body: required_translation(language, body_key, params),
    }
}

fn notification_event_key(event: &str, suffix: &str) -> String {
    assert!(
        NOTIFICATION_EVENTS.contains(&event),
        "unknown notification event: {event}"
    );
    format!("{event}_{suffix}")
}

/// Render notification copy that follows the `{event}_title` and
/// `{event}_body` naming convention.
pub fn notification_event_text(
    language: &Language,
    event: &str,
    params: &[(&str, String)],
) -> NotificationText {
    let title_key = notification_event_key(event, "title");
    let body_key = notification_event_key(event, "body");
    notification_text(language, &title_key, &body_key, params)
}

/// Render the professional email body belonging to a notification event.
pub fn notification_email_body(
    language: &Language,
    event: &str,
    params: &[(&str, String)],
) -> String {
    required_translation(
        language,
        &notification_event_key(event, "email_body"),
        params,
    )
}

/// Render structured bodies written by older releases so they remain readable
/// while the 90-day notification retention window expires. New notifications
/// are stored as fully rendered text and never enter this compatibility path.
pub fn legacy_notification_text(
    language: &Language,
    event: &str,
    body: &str,
) -> Option<NotificationText> {
    let data: serde_json::Value = serde_json::from_str(body).ok()?;

    match event {
        "timesheet_submitted" | "timesheet_approved" | "timesheet_rejected" => {
            let week_list = data
                .get("weeks")?
                .as_array()?
                .iter()
                .map(|value| legacy_week_label(language, value.as_str()?))
                .collect::<Option<Vec<_>>>()?;
            if week_list.is_empty() {
                return None;
            }
            let mut params = vec![
                ("week_count", week_count(language, week_list.len() as i64)),
                ("week_list", week_list.join("\n")),
            ];
            if event == "timesheet_submitted" {
                params.push((
                    "submitter_name",
                    data.get("submitter_name")?.as_str()?.to_string(),
                ));
            }
            if event == "timesheet_rejected" {
                params.push(("reason", data.get("reason")?.as_str()?.to_string()));
            }
            Some(notification_event_text(language, event, &params))
        }
        "reopen_request_created"
        | "reopen_approved"
        | "reopen_approved_by_admin"
        | "reopen_rejected"
        | "reopen_rejected_by_admin" => {
            let week_label = legacy_week_label(language, data.get("week")?.as_str()?)?;
            let mut params = vec![("week_label", week_label)];
            if matches!(
                event,
                "reopen_request_created" | "reopen_approved_by_admin" | "reopen_rejected_by_admin"
            ) {
                params.push((
                    "requester_name",
                    data.get("requester_name")?.as_str()?.to_string(),
                ));
            }
            if matches!(event, "reopen_rejected" | "reopen_rejected_by_admin") {
                params.push(("reason", data.get("reason")?.as_str()?.to_string()));
            }
            Some(notification_event_text(language, event, &params))
        }
        "reopen_auto_approved_notice" => {
            let params = [
                (
                    "requester_name",
                    data.get("requester_name")?.as_str()?.to_string(),
                ),
                (
                    "week_label",
                    legacy_week_label(language, data.get("week")?.as_str()?)?,
                ),
            ];
            Some(notification_text(
                language,
                "reopen_auto_approved_notice_title",
                "reopen_auto_approved_notice_body",
                &params,
            ))
        }
        "reopen_auto_approved" => {
            let params = [(
                "week_label",
                legacy_week_label(language, data.get("week")?.as_str()?)?,
            )];
            Some(notification_text(
                language,
                "reopen_auto_approved_title",
                "reopen_auto_approved_body",
                &params,
            ))
        }
        _ => None,
    }
}

fn legacy_week_label(language: &Language, week_start: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(week_start, "%Y-%m-%d")
        .ok()
        .map(|date| format_week_label(language, date))
}

pub fn email_login_line(language: &Language, public_url: Option<&str>) -> String {
    let Some(app_url) = public_url.map(str::trim).filter(|url| !url.is_empty()) else {
        return String::new();
    };
    required_translation(
        language,
        "email_login_url_line",
        &[("app_url", app_url.trim_end_matches('/').to_string())],
    )
}

pub fn email_organization_name(language: &Language, organization_name: &str) -> String {
    let organization_name = organization_name.trim();
    if organization_name.is_empty() {
        required_translation(language, "email_default_organization_name", &[])
    } else {
        organization_name.to_string()
    }
}

/// Append the shared timestamp and optional application URL to a notification
/// email using the centrally defined email layout.
pub fn email_with_footer(
    language: &Language,
    body: &str,
    timestamp: &str,
    public_url: Option<&str>,
) -> String {
    let base_params = [
        ("body", body.to_string()),
        ("timestamp", timestamp.to_string()),
    ];
    let Some(app_url) = public_url.map(str::trim).filter(|url| !url.is_empty()) else {
        return required_translation(language, "email_footer_without_url", &base_params);
    };

    required_translation(
        language,
        "email_footer_with_url",
        &[
            ("body", body.to_string()),
            ("timestamp", timestamp.to_string()),
            ("app_url", app_url.trim_end_matches('/').to_string()),
        ],
    )
}

pub fn technical_error_text(language: &Language, details: String) -> NotificationText {
    notification_text(
        language,
        "error_notification_title",
        "error_notification_body",
        &[("details", details)],
    )
}

pub fn technical_error_email_body(language: &Language, title: &str, details: &str) -> String {
    required_translation(
        language,
        "technical_error_email_body",
        &[
            ("title", title.to_string()),
            ("details", details.to_string()),
        ],
    )
}

pub fn backup_error_text(language: &Language, error_code: &str) -> NotificationText {
    match error_code {
        "backup_failed" => {
            notification_text(language, "backup_failed_title", "backup_failed_body", &[])
        }
        "backup_upload_failed" => notification_text(
            language,
            "backup_upload_failed_title",
            "backup_upload_failed_body",
            &[],
        ),
        _ => notification_text(
            language,
            "backup_unknown_error_title",
            "backup_unknown_error_body",
            &[("error_code", error_code.to_string())],
        ),
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

/// Returns the localised label for an absence category. For seeded slugs
/// (vacation, sick, training, …) the `absence_kind_<slug>` translation key is
/// honoured so existing translations still apply. For admin-created custom
/// categories no per-slug translation exists, so the function falls back to
/// the category's stored `name`, which the admin chose when creating it.
///
/// This is the canonical way to format an absence category's display name in
/// backend output (notifications, emails, PDF). Callers should pass the slug
/// and the stored category name together — both are present on `Absence` rows
/// via the join in `ABS_SELECT`.
pub fn absence_kind_label(language: &Language, slug: &str, name: &str) -> String {
    let key = format!("absence_kind_{slug}");
    let translated = translate(language, &key, &[]);
    if translated != key {
        // Seeded slug: a translation existed.
        return translated;
    }
    // Custom admin category: fall back to the stored name. We deliberately
    // avoid translating the name itself — there's no guarantee an admin's
    // name happens to match a key in the static translation table, and
    // routing every absence label through `translate` would produce
    // misleading "translations" for incidental key collisions.
    name.to_string()
}

/// Returns the localised weekday name for an English weekday identifier as
/// produced by `services::reports::weekday_en` (e.g. `"Monday"`). Used by the
/// timesheet PDF, which renders day rows directly from `MonthReport` rather
/// than from pre-translated JSON sent to the frontend.
pub fn weekday_label(language: &Language, weekday_en: &str) -> String {
    let key = format!("weekday_{}", weekday_en.to_ascii_lowercase());
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
    render_template(template, params)
}

fn required_translation(language: &Language, key: &str, params: &[(&str, String)]) -> String {
    let language_definition = lang_def(language);
    let template = language_definition
        .translations
        .iter()
        .find(|(translation_key, _)| *translation_key == key)
        .map(|(_, translation_value)| *translation_value)
        .unwrap_or_else(|| panic!("missing required translation key: {key}"));

    for placeholder in template_placeholder_names(template) {
        assert!(
            params.iter().any(|(name, _)| *name == placeholder),
            "missing template parameter {placeholder:?} for translation key {key:?}"
        );
    }

    render_template(template, params)
}

fn template_placeholder_names(template: &str) -> Vec<&str> {
    template
        .split('{')
        .skip(1)
        .filter_map(|part| part.split_once('}').map(|(name, _)| name))
        .collect()
}

fn render_template(template: &str, params: &[(&str, String)]) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut remaining = template;

    while let Some(placeholder_start) = remaining.find('{') {
        rendered.push_str(&remaining[..placeholder_start]);
        let placeholder = &remaining[placeholder_start + 1..];
        let Some(placeholder_end) = placeholder.find('}') else {
            rendered.push_str(&remaining[placeholder_start..]);
            return rendered;
        };
        let key = &placeholder[..placeholder_end];
        if let Some((_, value)) = params.iter().find(|(param_key, _)| *param_key == key) {
            rendered.push_str(value);
        } else {
            rendered.push('{');
            rendered.push_str(key);
            rendered.push('}');
        }
        remaining = &placeholder[placeholder_end + 1..];
    }
    rendered.push_str(remaining);
    rendered
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap, HashSet};

    fn placeholder_names(template: &str) -> BTreeSet<&str> {
        template_placeholder_names(template).into_iter().collect()
    }

    #[test]
    fn every_translation_key_has_a_german_translation_with_matching_placeholders() {
        let german = LANGUAGES
            .iter()
            .find(|language| language.code == "de")
            .expect("German language definition");
        let german_templates: HashMap<&str, &str> = german.translations.iter().copied().collect();
        assert_eq!(
            german_templates.len(),
            german.translations.len(),
            "German translation keys must be unique"
        );

        for language in LANGUAGES {
            let mut seen_keys = HashSet::new();
            for (key, template) in language.translations {
                assert!(
                    seen_keys.insert(*key),
                    "duplicate translation key {key:?} in {}",
                    language.code
                );
                let german_template = german_templates.get(key).unwrap_or_else(|| {
                    panic!(
                        "translation key {key:?} in {} has no German translation",
                        language.code
                    )
                });
                assert_eq!(
                    placeholder_names(template),
                    placeholder_names(german_template),
                    "placeholder mismatch for translation key {key:?}"
                );
            }
        }

        let default_language = LANGUAGES
            .iter()
            .find(|language| language.code == DEFAULT_LANGUAGE_CODE)
            .expect("default language definition");
        assert_eq!(
            german_templates.len(),
            default_language.translations.len(),
            "German must not contain keys missing from the default language"
        );
    }

    #[test]
    fn every_notification_event_has_app_and_email_templates() {
        for language in LANGUAGES {
            let templates: HashMap<&str, &str> = language.translations.iter().copied().collect();
            let params = [
                ("requester_name", "Example User".to_string()),
                ("week_label", "Example Week".to_string()),
                ("reason", "Example reason".to_string()),
                ("kind", "Example absence".to_string()),
                ("start_date", "01/01/2030".to_string()),
                ("end_date", "01/02/2030".to_string()),
                ("submitter_name", "Example User".to_string()),
                (
                    "week_count",
                    week_count(&Language::from_setting(language.code), 2),
                ),
                ("week_list", "Example Week".to_string()),
                ("weeks", "Example Week".to_string()),
                ("count", "2".to_string()),
            ];
            for event in NOTIFICATION_EVENTS {
                for suffix in ["title", "body", "email_body"] {
                    let key = format!("{event}_{suffix}");
                    assert!(
                        templates.contains_key(key.as_str()),
                        "{} is missing {key}",
                        language.code
                    );
                }

                let event_language = Language::from_setting(language.code);
                let text = notification_event_text(&event_language, event, &params);
                let email_body = notification_email_body(&event_language, event, &params);
                assert!(!text.title.contains('{'), "unresolved title for {event}");
                assert!(!text.body.contains('{'), "unresolved body for {event}");
                assert!(
                    !email_body.contains('{'),
                    "unresolved email body for {event}"
                );
            }
        }
    }

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

        let plain = translate(
            &language,
            "reopen_approved_body",
            &[("week_label", week_label.clone())],
        );
        assert!(plain.contains(&week_label));
        assert!(
            !plain.contains("{week_label}"),
            "all placeholders must be substituted"
        );
    }

    #[test]
    fn timesheet_status_titles_use_localized_week_counts() {
        for (language_code, expected_approved, expected_rejected) in [
            ("en", "2 weeks approved", "2 weeks rejected"),
            ("de", "2 Wochen genehmigt", "2 Wochen abgelehnt"),
        ] {
            let language = Language::from_setting(language_code);
            let approved = notification_event_text(
                &language,
                "timesheet_approved",
                &[
                    ("week_count", week_count(&language, 2)),
                    ("week_list", "week list".to_string()),
                ],
            );
            let rejected = notification_event_text(
                &language,
                "timesheet_rejected",
                &[
                    ("week_count", week_count(&language, 2)),
                    ("week_list", "week list".to_string()),
                    ("reason", "reason".to_string()),
                ],
            );

            assert_eq!(approved.title, expected_approved);
            assert_eq!(rejected.title, expected_rejected);
        }
    }

    #[test]
    fn parameter_values_are_not_interpreted_as_templates() {
        let rendered = render_template(
            "{requester_name}: {kind}",
            &[
                ("requester_name", "{kind}".to_string()),
                ("kind", "Vacation".to_string()),
            ],
        );

        assert_eq!(rendered, "{kind}: Vacation");
    }

    #[test]
    fn legacy_structured_notifications_use_central_templates() {
        let english = Language::from_setting("en");
        let timesheet = legacy_notification_text(
            &english,
            "timesheet_approved",
            r#"{"weeks":["2030-01-07","2030-01-14"]}"#,
        )
        .expect("legacy timesheet notification");
        assert_eq!(timesheet.title, "2 weeks approved");
        assert!(timesheet.body.starts_with("Approved:\nCW 2"));
        assert!(timesheet.body.contains("CW 3"));

        let german = Language::from_setting("de");
        let reopen = legacy_notification_text(
            &german,
            "reopen_rejected_by_admin",
            r#"{"week":"2030-01-07","requester_name":"Ada Lovelace","reason":"Zu spät"}"#,
        )
        .expect("legacy reopen notification");
        assert_eq!(
            reopen.title,
            "Bearbeitungsanfrage von Ada Lovelace durch Admin abgelehnt"
        );
        assert!(reopen.body.contains("KW 2"));
        assert!(reopen.body.contains("Grund: Zu spät"));

        let auto_approved =
            legacy_notification_text(&english, "reopen_auto_approved", r#"{"week":"2030-01-07"}"#)
                .expect("legacy automatic reopen notification");
        assert_eq!(auto_approved.title, "Week editing enabled");
        assert!(auto_approved.body.contains("CW 2"));

        assert!(legacy_notification_text(&english, "timesheet_approved", "plain text").is_none());
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

        assert_eq!(subject, "Ihr Passwort zur\u{00fc}cksetzen");
        assert!(body.contains("https://zerf.example/reset"));
        assert!(body.contains("1 Stunde"));
    }

    #[test]
    fn account_created_email_template_uses_parameters() {
        let language = Language::from_setting("en");
        let login_line = email_login_line(&language, Some("https://zerf.example"));
        let body = translate(
            &language,
            "account_created_body",
            &[
                ("org_name", "Example Org".to_string()),
                ("first_name", "Ada".to_string()),
                ("last_name", "Lovelace".to_string()),
                ("email", "ada@example.com".to_string()),
                ("password", "TempPass!234".to_string()),
                ("login_line", login_line),
            ],
        );

        assert!(body.contains("Hello Ada Lovelace"));
        assert!(body.contains("Example Org"));
        assert!(body.contains("Account: ada@example.com"));
        assert!(body.contains("Temporary password: TempPass!234"));
        assert!(body.contains("Sign-in URL: https://zerf.example"));
    }

    #[test]
    fn email_footer_uses_the_shared_layout_and_normalizes_the_url() {
        let language = Language::from_setting("en");
        assert_eq!(
            email_with_footer(
                &language,
                "Message body",
                "04/27/2026 09:00",
                Some("https://zerf.example/"),
            ),
            "Message body\n\n04/27/2026 09:00\n\nhttps://zerf.example"
        );
        assert_eq!(
            email_with_footer(&language, "Message body", "04/27/2026 09:00", None),
            "Message body\n\n04/27/2026 09:00"
        );
    }

    #[test]
    fn admin_password_reset_email_template_uses_parameters() {
        for lang in ["en", "de"] {
            let language = Language::from_setting(lang);
            let login_line = email_login_line(&language, Some("https://zerf.example"));
            let subject = translate(
                &language,
                "admin_password_reset_subject",
                &[("org_name", "TestOrg".to_string())],
            );
            let body = translate(
                &language,
                "admin_password_reset_body",
                &[
                    ("first_name", "Max".to_string()),
                    ("last_name", "Mustermann".to_string()),
                    ("email", "max@example.com".to_string()),
                    ("password", "NewTmp!567".to_string()),
                    ("login_line", login_line),
                ],
            );
            assert!(
                subject.contains("TestOrg"),
                "{lang}: subject must contain org name"
            );
            assert!(
                body.contains("Max Mustermann"),
                "{lang}: body must contain full name"
            );
            assert!(
                body.contains("max@example.com"),
                "{lang}: body must contain email"
            );
            assert!(
                body.contains("NewTmp!567"),
                "{lang}: body must contain password"
            );
            assert!(
                body.contains("https://zerf.example"),
                "{lang}: body must contain login URL"
            );
            // Must not contain any un-substituted placeholders.
            assert!(
                !body.contains('{'),
                "{lang}: no un-substituted placeholders"
            );
        }
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

    /// When local_name is absent or blank, holiday_display_name must fall back
    /// to the English name.
    #[test]
    fn holiday_display_name_falls_back_to_english_name_when_local_name_absent() {
        let language = Language::from_setting("en");
        assert_eq!(
            holiday_display_name(&language, "Labor Day".to_string(), None),
            "Labor Day"
        );
        assert_eq!(
            holiday_display_name(&language, "Labor Day".to_string(), Some("  ".to_string())),
            "Labor Day"
        );
    }

    /// `Language::name` must return the human-readable name for each supported code.
    #[test]
    fn language_name_returns_display_name() {
        assert_eq!(Language::from_setting("en").name(), "English");
        assert_eq!(Language::from_setting("de").name(), "Deutsch");
    }

    /// `Language::code` must survive a round-trip through `from_setting`.
    #[test]
    fn language_code_round_trips_through_from_setting() {
        let lang = Language::from_setting("en");
        assert_eq!(lang.code(), "en");
        let lang_de = Language::from_setting("de");
        assert_eq!(lang_de.code(), "de");
    }

    /// `work_category_label` must translate known German category names and pass
    /// unknown names through unchanged.
    #[test]
    fn work_category_label_translates_known_german_categories() {
        let de = Language::from_setting("de");
        assert_eq!(work_category_label(&de, "Core Duties"), "Kernaufgaben");
        assert_eq!(work_category_label(&de, "Training"), "Fortbildung");
        assert_eq!(work_category_label(&de, "Other"), "Sonstiges");
        assert_eq!(
            work_category_label(&de, "Flextime Reduction"),
            "Gleitzeitabbau"
        );
        // Unknown category must pass through unchanged.
        assert_eq!(work_category_label(&de, "Custom Project"), "Custom Project");
    }

    /// English leaves category names unchanged.
    #[test]
    fn work_category_label_returns_name_unchanged_for_english() {
        let en = Language::from_setting("en");
        assert_eq!(work_category_label(&en, "Core Duties"), "Core Duties");
        assert_eq!(work_category_label(&en, "Training"), "Training");
    }

    /// `absence_kind_label` must produce localised strings for seeded slugs
    /// (using `absence_kind_<slug>` keys) and fall back to the stored category
    /// name for admin-created custom categories.
    #[test]
    fn absence_kind_label_localises_known_kinds() {
        let en = Language::from_setting("en");
        // Seeded slugs honour their translation key regardless of the supplied name.
        assert_eq!(absence_kind_label(&en, "vacation", "anything"), "Vacation");
        assert_eq!(absence_kind_label(&en, "sick", "anything"), "Sick");
        assert_eq!(
            absence_kind_label(&en, "flextime_reduction", "anything"),
            "Flextime Reduction"
        );

        let de = Language::from_setting("de");
        assert_eq!(absence_kind_label(&de, "vacation", "anything"), "Urlaub");
        assert_eq!(absence_kind_label(&de, "sick", "anything"), "Krankmeldung");

        // Custom admin slug: falls back to the stored category name in both languages.
        assert_eq!(
            absence_kind_label(&en, "comp_time", "Comp Time"),
            "Comp Time"
        );
        assert_eq!(
            absence_kind_label(&de, "comp_time", "Comp Time"),
            "Comp Time"
        );
    }

    /// `format_datetime_in_timezone` must apply the given timezone offset and
    /// produce a properly formatted string.
    #[test]
    fn format_datetime_in_timezone_applies_tz_and_formats_correctly() {
        use chrono::{TimeZone, Utc};
        let utc_time = Utc.with_ymd_and_hms(2026, 5, 1, 10, 30, 0).unwrap();

        // German format (Berlin = UTC+2 in summer): "01.05.2026 12:30"
        let de = Language::from_setting("de");
        let formatted_de = format_datetime_in_timezone(&de, utc_time, "Europe/Berlin");
        assert_eq!(formatted_de, "01.05.2026 12:30");

        // English format: "05/01/2026 12:30"
        let en = Language::from_setting("en");
        let formatted_en = format_datetime_in_timezone(&en, utc_time, "Europe/Berlin");
        assert_eq!(formatted_en, "05/01/2026 12:30");
    }

    /// `format_datetime_in_timezone` must fall back to UTC when the timezone
    /// string is unrecognised.
    #[test]
    fn format_datetime_in_timezone_falls_back_to_utc_for_unknown_tz() {
        use chrono::{TimeZone, Utc};
        let utc_time = Utc.with_ymd_and_hms(2026, 5, 1, 10, 30, 0).unwrap();
        let en = Language::from_setting("en");
        // An invalid timezone should fall back to UTC.
        let formatted = format_datetime_in_timezone(&en, utc_time, "Mars/Olympus");
        // UTC time is 10:30, so the formatted string should contain 10:30.
        assert!(
            formatted.contains("10:30"),
            "expected UTC time in output, got: {formatted}"
        );
    }

    /// `week_count` must use the singular form for exactly 1 and the plural
    /// template for any other count.
    #[test]
    fn week_count_uses_singular_for_one_and_plural_for_others() {
        let en = Language::from_setting("en");
        assert_eq!(week_count(&en, 1), "1 week");
        assert_eq!(week_count(&en, 3), "3 weeks");
        assert_eq!(week_count(&en, 0), "0 weeks");

        let de = Language::from_setting("de");
        assert_eq!(week_count(&de, 1), "1 Woche");
        assert_eq!(week_count(&de, 5), "5 Wochen");
    }

    /// `format_week_label` must include the ISO week number and date range.
    #[test]
    fn format_week_label_includes_week_number_and_date_range() {
        // 2026-04-27 is Monday of ISO week 18.
        let monday = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();

        let en = Language::from_setting("en");
        let label_en = format_week_label(&en, monday);
        assert!(
            label_en.starts_with("CW 18"),
            "expected CW prefix, got: {label_en}"
        );
        assert!(
            label_en.contains("to"),
            "expected 'to' separator, got: {label_en}"
        );

        let de = Language::from_setting("de");
        let label_de = format_week_label(&de, monday);
        assert!(
            label_de.starts_with("KW 18"),
            "expected KW prefix, got: {label_de}"
        );
        assert!(
            label_de.contains("bis"),
            "expected 'bis' separator, got: {label_de}"
        );
    }
}
