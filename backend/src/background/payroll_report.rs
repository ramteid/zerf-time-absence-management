//! Monthly payroll report email to the tax office / payroll accountant.
//!
//! Schedule — identical to the Nextcloud timesheet export (`report_upload`),
//! both built on `background::schedule`:
//!   1. Each midnight tick: once `today.day() >= payroll_report_day_of_month`,
//!      queue the previous month (idempotent, guarded by the
//!      `payroll_report_queue_period` app_setting; missed months are backfilled).
//!   2. Process queued periods: a period is sent only when every employee it
//!      covers has a final month. Otherwise it stays queued and is retried on
//!      the next daily check, so late submitters are caught up automatically.
//!      Before the configured day of month, older catch-up periods are still
//!      processed while the just-finished previous month waits.
//!
//! Finality is the shared `month_export_readiness` gate, plus one extra rule:
//! for everybody whose *hours* are in the report, all time entries of the month
//! must be approved. Payroll pays by those hours, so reporting a draft or
//! not-yet-approved month would understate what is owed. When a queued period
//! is blocked, admins who opted in to technical error notifications are told
//! who is holding it up.
//!
//! `run_now` (admin "Send now" button) queues the previous month and processes
//! everything immediately, skipping only the day-of-month threshold — never the
//! readiness gate. A manual send does **not** remove the period from the
//! queue: the regular scheduled delivery for that month still goes out
//! separately on the configured day, and the manual copy's email body carries
//! a note saying so, so the recipient does not mistake it for the final one.

use crate::background::schedule;
use crate::error::{AppError, AppResult};
use crate::i18n::Language;
use crate::repository::User;
use crate::services::payroll_report::{self, PayrollReportConfig};
use crate::services::reports::{month_export_readiness, MonthExportReadiness};
use crate::services::settings;
use crate::AppState;
use chrono::NaiveDate;

/// How many blocking employees are listed in the admin notification before it
/// is truncated — enough to act on, short enough to stay readable.
const MAX_LISTED_BLOCKERS: usize = 10;

/// Background loop: checks once per day (midnight in app timezone).
pub async fn run_loop(state: AppState) {
    schedule::run_daily_after_midnight(state, "Payroll report", |state| async move {
        run_once(&state).await
    })
    .await;
}

/// Daily scheduled run: queue the previous month once the configured day of
/// month is reached, then send every queued period that is ready.
async fn run_once(state: &AppState) -> AppResult<()> {
    let config = payroll_report::load_config(&state.pool).await?;
    if !config.enabled || config.recipients.is_empty() {
        return Ok(());
    }
    let relevant_categories = payroll_report::payroll_relevant_categories(&state.pool).await?;
    if config.has_no_content(&relevant_categories) {
        return Ok(());
    }

    let today = settings::app_today(&state.pool).await;
    let process_through_period = schedule::process_through_period(today, config.day_of_month)?;
    if process_through_period.is_none() {
        queue_previous_month(state, today).await?;
    }

    process_pending_periods(state, &config, process_through_period.as_deref(), false).await;
    Ok(())
}

/// Result of an admin-triggered run, so the UI can say whether a report
/// actually went out or the months are still waiting for approvals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct RunSummary {
    /// Periods whose report was accepted by the SMTP server.
    pub sent: usize,
    /// Periods that stay queued (not final yet, or delivery failed).
    pub pending: usize,
}

/// Triggered by the admin "Send now" button: queues the previous month
/// (idempotent) and processes all pending periods right away.
pub async fn run_now(state: &AppState) -> AppResult<RunSummary> {
    let config = payroll_report::load_config(&state.pool).await?;
    if !config.enabled {
        return Err(AppError::BadRequest(
            "The payroll report is not enabled.".into(),
        ));
    }
    if config.recipients.is_empty() {
        return Err(AppError::BadRequest(
            "No recipient address configured for the payroll report.".into(),
        ));
    }
    let relevant_categories = payroll_report::payroll_relevant_categories(&state.pool).await?;
    if config.has_no_content(&relevant_categories) {
        return Err(AppError::BadRequest(
            "Select at least one section for the payroll report.".into(),
        ));
    }
    if settings::load_smtp_config(&state.pool).await.is_none() {
        return Err(AppError::BadRequest(
            "Email delivery is not configured; the payroll report cannot be sent.".into(),
        ));
    }

    let today = settings::app_today(&state.pool).await;
    queue_previous_month(state, today).await?;
    Ok(process_pending_periods(state, &config, None, true).await)
}

/// Queue every month from the last recorded one through the previous month.
async fn queue_previous_month(state: &AppState, today: NaiveDate) -> AppResult<()> {
    schedule::queue_periods_through_previous_month(
        state,
        settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY,
        today,
        |period| async move {
            state.db.payroll_queue.enqueue(&period).await?;
            tracing::info!("Payroll report: queued period {period}");
            Ok(())
        },
    )
    .await
}

/// Send every queued period that is due and ready; leave the rest queued.
///
/// `is_manual` distinguishes an admin-triggered "Send now" run from the daily
/// scheduled one: a manual send never removes the period from the queue (see
/// [`process_period`]), so it never stops the regular delivery for that month.
async fn process_pending_periods(
    state: &AppState,
    config: &PayrollReportConfig,
    process_through_period: Option<&str>,
    is_manual: bool,
) -> RunSummary {
    let mut summary = RunSummary::default();
    let periods = match state.db.payroll_queue.list_pending().await {
        Ok(periods) => periods,
        Err(e) => {
            tracing::error!("Payroll report: failed to list queue: {e}");
            return summary;
        }
    };
    if periods.is_empty() {
        return summary;
    }

    let language = match crate::i18n::load_ui_language(&state.pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::error!("Payroll report: failed to load UI language: {e}");
            return summary;
        }
    };

    for period in periods {
        if schedule::period_is_deferred(&period, process_through_period) {
            tracing::debug!(
                "Payroll report: deferring period {period} until the configured day of month"
            );
            summary.pending += 1;
            continue;
        }
        match process_period(state, &period, config, &language, is_manual).await {
            Ok(true) => summary.sent += 1,
            Ok(false) => summary.pending += 1,
            Err(e) => {
                tracing::warn!("Payroll report: skipping period {period}: {e}");
                summary.pending += 1;
            }
        }
    }
    summary
}

/// Build and send one period's report. Returns whether it was sent; `false`
/// means the month is not final yet and stays queued for the next daily check.
///
/// `is_manual` marks an admin-triggered "Send now" copy: its email carries an
/// extra note, and — unlike the scheduled run — it never deletes the queue
/// entry, so the regular automatic delivery for this month still happens on
/// the configured day even after a successful manual send.
async fn process_period(
    state: &AppState,
    period: &str,
    config: &PayrollReportConfig,
    language: &Language,
    is_manual: bool,
) -> AppResult<bool> {
    let (from, to) = schedule::period_bounds(period)?;
    // Same member set as the timesheet export: everyone the month actually
    // covers, including archived accounts that still have data in it.
    let members = state
        .db
        .reports
        .timesheet_members_for_period(from, to)
        .await?;

    let blockers = collect_blockers(state, &members, config, from, to).await?;
    if !blockers.is_empty() {
        report_blocked(state, period, &blockers, language).await;
        return Ok(false);
    }

    let data =
        payroll_report::build_report_data(state, from, to, &members, config, language).await?;
    let bytes = crate::report_pdf::render_payroll_report_pdf(&data, language);
    if bytes.is_empty() {
        return Err(AppError::Internal(format!(
            "Generated payroll report PDF is empty for period {period}"
        )));
    }

    let smtp = settings::load_smtp_config(&state.pool)
        .await
        .ok_or_else(|| AppError::Internal("SMTP is not configured".into()))?;

    let text = email_text(
        language,
        &data.period_label,
        &organization_label(state, language).await,
        is_manual,
    );

    crate::email::send_with_attachment(
        &smtp,
        &config.recipients,
        &text.title,
        &text.body,
        crate::email::EmailAttachment {
            filename: format!("{period}_payroll_report.pdf"),
            content_type: "application/pdf".to_string(),
            bytes,
        },
    )
    .await
    .map_err(|e| AppError::Internal(format!("Payroll report email failed: {e}")))?;

    if is_manual {
        // A manual "Send now" copy never replaces the scheduled delivery: the
        // period stays queued, so the automatic run still sends the regular
        // copy for this month on the configured day.
        tracing::info!(
            "Payroll report: sent period {period} manually to {} (period stays queued for the scheduled run)",
            config.recipients.join(", ")
        );
    } else {
        // Only drop the period once the SMTP server accepted the message.
        state.db.payroll_queue.delete_entry(period).await?;
        tracing::info!(
            "Payroll report: sent period {period} to {}",
            config.recipients.join(", ")
        );
    }
    Ok(true)
}

/// One employee holding up a period, with the reason in the report language.
struct Blocker {
    name: String,
    reason_key: &'static str,
}

/// Everyone whose month is not final yet. An empty result means the period can
/// be sent.
async fn collect_blockers(
    state: &AppState,
    members: &[User],
    config: &PayrollReportConfig,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<Blocker>> {
    let mut blockers = Vec::new();
    for member in members {
        // Hours are only payroll-grade once every entry behind them is
        // approved — a still-open or merely submitted month would be paid out
        // too low. Full approval is required exactly when this person's hours
        // are actually part of the report.
        let require_full_approval = config.includes_hours_for(&member.role);
        let readiness =
            month_export_readiness(&state.pool, member, from, to, require_full_approval).await?;
        let reason_key = match readiness {
            MonthExportReadiness::Ready => None,
            MonthExportReadiness::PreStartContent => {
                Some("payroll_report_reason_pre_start_content")
            }
            MonthExportReadiness::WeeksNotSubmitted => Some("payroll_report_reason_not_submitted"),
            MonthExportReadiness::PendingAbsenceRequests => {
                Some("payroll_report_reason_pending_absences")
            }
            MonthExportReadiness::UnresolvedTimeEntries => {
                Some("payroll_report_reason_unresolved_entries")
            }
            MonthExportReadiness::UnapprovedTimeEntries => {
                Some("payroll_report_reason_unapproved_entries")
            }
        };
        if let Some(reason_key) = reason_key {
            blockers.push(Blocker {
                name: format!("{} {}", member.first_name, member.last_name),
                reason_key,
            });
        }
    }
    Ok(blockers)
}

/// Subject and body of the payroll report email.
///
/// Kept separate from the send path so the template parameters are covered by a
/// unit test: `i18n::notification_text` panics on a missing parameter, and this
/// runs inside a background task where that would kill the loop.
///
/// `manual` appends a note identifying the email as an admin-triggered "Send
/// now" copy, so the recipient does not mistake it for the regular automatic
/// delivery, which — see [`process_period`] — is still sent separately.
fn email_text(
    language: &Language,
    period_label: &str,
    organization: &str,
    manual: bool,
) -> crate::i18n::NotificationText {
    let mut text = crate::i18n::notification_text(
        language,
        "payroll_report_email_subject",
        "payroll_report_email_body",
        &[
            ("period", period_label.to_string()),
            ("org_name", organization.to_string()),
        ],
    );
    if manual {
        text.body
            .push_str(&crate::i18n::translate(language, "payroll_report_email_manual_note", &[]));
    }
    text
}

/// Title and body of the "the report could not go out yet" admin notification,
/// listing who is holding the month up (truncated after
/// [`MAX_LISTED_BLOCKERS`] names).
fn blocked_text(
    language: &Language,
    period: &str,
    blockers: &[Blocker],
) -> crate::i18n::NotificationText {
    let listed: Vec<String> = blockers
        .iter()
        .take(MAX_LISTED_BLOCKERS)
        .map(|blocker| {
            format!(
                "- {} ({})",
                blocker.name,
                crate::i18n::translate(language, blocker.reason_key, &[])
            )
        })
        .collect();
    let mut details = listed.join("\n");
    if blockers.len() > MAX_LISTED_BLOCKERS {
        details.push_str(&crate::i18n::translate(
            language,
            "payroll_report_blocked_more",
            &[("count", (blockers.len() - MAX_LISTED_BLOCKERS).to_string())],
        ));
    }

    crate::i18n::notification_text(
        language,
        "payroll_report_blocked_title",
        "payroll_report_blocked_body",
        &[("period", period.to_string()), ("employees", details)],
    )
}

/// Log the blocked period and alert opted-in admins so the missing approvals
/// can be chased — a payroll report that silently never goes out is worse than
/// a late one.
async fn report_blocked(state: &AppState, period: &str, blockers: &[Blocker], language: &Language) {
    let text = blocked_text(language, period, blockers);
    tracing::warn!(target: "zerf::payroll_report", "{}", text.body);
    crate::services::notifications::enqueue_error(
        state,
        language,
        &format!("payroll_report_blocked_{period}"),
        &text.title,
        &text.body,
    )
    .await;
}

/// Organization name for the email copy, falling back to the product name when
/// none is configured.
async fn organization_label(state: &AppState, language: &Language) -> String {
    let configured = settings::load_setting(&state.pool, settings::ORGANIZATION_NAME_KEY, "")
        .await
        .unwrap_or_default();
    if configured.trim().is_empty() {
        crate::i18n::translate(language, "email_default_organization_name", &[])
    } else {
        configured.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocker(name: &str) -> Blocker {
        Blocker {
            name: name.to_string(),
            reason_key: "payroll_report_reason_not_submitted",
        }
    }

    /// Both templates must render in every supported language: a missing key or
    /// parameter panics, and this code runs inside a background loop.
    #[test]
    fn email_and_blocked_texts_render_in_every_language() {
        for code in ["en", "de"] {
            let language = crate::i18n::Language::from_setting(code);

            let email = email_text(&language, "May 2026", "Example GmbH", false);
            assert!(email.title.contains("May 2026"), "subject: {}", email.title);
            assert!(email.title.contains("Example GmbH"));
            assert!(email.body.contains("May 2026"));

            let blocked = blocked_text(&language, "2026-05", &[blocker("Jane Doe")]);
            assert!(!blocked.title.is_empty());
            assert!(blocked.body.contains("2026-05"));
            assert!(blocked.body.contains("Jane Doe"));
        }
    }

    /// A manually triggered "Send now" copy must carry a note so the recipient
    /// does not mistake it for the regular automatic delivery, which — since
    /// the queue entry is never deleted for a manual send — still goes out
    /// separately. A scheduled send must not carry that note.
    #[test]
    fn email_text_adds_the_manual_note_only_for_manual_sends() {
        for code in ["en", "de"] {
            let language = crate::i18n::Language::from_setting(code);

            let scheduled = email_text(&language, "May 2026", "Example GmbH", false);
            let manual = email_text(&language, "May 2026", "Example GmbH", true);

            assert_eq!(
                scheduled.body,
                email_text(&language, "May 2026", "Example GmbH", false).body,
                "scheduled body is deterministic"
            );
            assert!(
                manual.body.len() > scheduled.body.len(),
                "manual body must be strictly longer than the scheduled one ({code})"
            );
            assert!(
                manual.body.starts_with(&scheduled.body),
                "the manual note is appended, not mixed into the main body ({code})"
            );
            // Subject stays identical — only the body gains the note.
            assert_eq!(scheduled.title, manual.title);
        }
    }

    #[test]
    fn blocked_text_truncates_long_blocker_lists() {
        let language = crate::i18n::Language::default();
        let blockers: Vec<Blocker> = (0..MAX_LISTED_BLOCKERS + 2)
            .map(|index| blocker(&format!("Person {index}")))
            .collect();

        let text = blocked_text(&language, "2026-05", &blockers);
        assert!(text.body.contains("Person 0"));
        assert!(
            text.body
                .contains(&format!("Person {}", MAX_LISTED_BLOCKERS - 1)),
            "the last listed name is included"
        );
        assert!(
            !text.body.contains(&format!("Person {MAX_LISTED_BLOCKERS}")),
            "names beyond the limit are dropped"
        );
        assert!(text.body.contains('2'), "the remaining count is mentioned");
    }
}
