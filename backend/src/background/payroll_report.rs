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
//! not-yet-approved month would understate what is owed.
//!
//! A blocked scheduled period is **not** an error: people simply have not
//! finished their month yet. It is logged and otherwise stays silent — the
//! payroll dashboard tile (`services::payroll_report::build_status`) is where
//! admins and team leads see who is still missing.
//!
//! `run_now` (admin "Send now" button) queues the previous month and processes
//! everything immediately, skipping the day-of-month threshold. Unlike the
//! scheduled run it does **not** wait for everyone: it sends a *provisional*
//! report covering whoever is already final, clearly marked as partial in both
//! the PDF and the email so the recipient cannot mistake short figures for
//! final ones. A manual send never removes the period from the queue, so the
//! regular scheduled delivery for that month still goes out separately.

use crate::background::schedule;
use crate::error::{AppError, AppResult};
use crate::i18n::Language;
use crate::repository::User;
use crate::report_pdf::{PayrollOmittedPerson, ProvisionalNotice};
use crate::services::payroll_report::{self, PayrollReportConfig};
use crate::services::settings;
use crate::AppState;
use chrono::NaiveDate;

/// Background loop: checks once per day (midnight in app timezone).
pub async fn run_loop(state: AppState) {
    schedule::run_daily_after_midnight(state, "Payroll report", |state| async move {
        run_once(&state).await
    })
    .await;
}

/// Daily scheduled run: queue the previous month once the configured day of
/// month is reached, then send every queued period that is ready.
///
/// Public so integration tests can drive one scheduled tick directly instead
/// of waiting on the real midnight loop.
pub async fn run_once(state: &AppState) -> AppResult<()> {
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

/// Build and send one period's report. Returns whether anything was sent.
///
/// The two callers differ in what an unfinished month means:
///
/// * **scheduled run** — nothing goes out until every covered person is final.
///   The period stays queued and is retried tomorrow. This is the delivery the
///   payroll accountant treats as authoritative, so it must be complete.
/// * **`is_manual`** (admin "Send now") — sends a provisional report with
///   whoever is already final, marked as partial in the PDF and the email. It
///   never deletes the queue entry, so the complete scheduled delivery still
///   follows on the configured day.
async fn process_period(
    state: &AppState,
    period: &str,
    config: &PayrollReportConfig,
    language: &Language,
    is_manual: bool,
) -> AppResult<bool> {
    let (from, to) = schedule::period_bounds(period)?;
    // Same member set as the timesheet export — everyone the month actually
    // covers, including archived accounts that still have data in it — minus
    // admins and anyone the admin excluded from the report.
    let members = payroll_report::payroll_members(
        state
            .db
            .reports
            .timesheet_members_for_period(from, to)
            .await?,
        &config.excluded_user_ids,
    );

    let readiness = payroll_report::evaluate_members(state, &members, config, from, to).await?;
    let (ready, pending): (Vec<_>, Vec<_>) = readiness
        .into_iter()
        .partition(|member| member.reason_key.is_none());

    if ready.is_empty() && pending.is_empty() {
        // The month covers nobody at all — the installation is younger than
        // the period, or everyone in it was excluded. There is nothing to
        // report, so settle the period instead of retrying it every night
        // forever and leaving the dashboard card stuck on "0 of 0".
        if !is_manual {
            state.db.payroll_queue.delete_entry(period).await?;
            record_last_sent_period(state, period).await;
        }
        tracing::info!("Payroll report: period {period} covers nobody; nothing to send");
        return Ok(false);
    }
    if !pending.is_empty() && !is_manual {
        // Not an error — people just have not finished their month. Stay
        // silent and retry tomorrow; the dashboard tile shows who is missing.
        tracing::info!(
            "Payroll report: period {period} still waiting for {} of {} people",
            pending.len(),
            ready.len() + pending.len()
        );
        return Ok(false);
    }
    if ready.is_empty() {
        // A manual send while nobody has finished yet: an empty document helps
        // the payroll accountant no more than no document at all.
        tracing::info!("Payroll report: period {period} has no finalized people yet");
        return Ok(false);
    }

    // Only a manual send can get here with people missing, and only then does
    // the report need the "this is partial" marker.
    let provisional = (!pending.is_empty()).then(|| ProvisionalNotice {
        included: ready.len(),
        total: ready.len() + pending.len(),
        omitted: pending
            .iter()
            .map(|member| PayrollOmittedPerson {
                name: format!("{} {}", member.user.first_name, member.user.last_name),
                reason_key: member
                    .reason_key
                    .unwrap_or("payroll_report_reason_not_submitted"),
            })
            .collect(),
    });

    let included: Vec<User> = ready.into_iter().map(|member| member.user).collect();
    let data = payroll_report::build_report_data(
        state, from, to, &included, config, language, provisional,
    )
    .await?;
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
        data.provisional.as_ref(),
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
        // copy for this month on the configured day. For the same reason it
        // must not mark the period as sent — the dashboard tile has to keep
        // showing the outstanding delivery.
        tracing::info!(
            "Payroll report: sent period {period} manually to {} (period stays queued for the scheduled run)",
            config.recipients.join(", ")
        );
    } else {
        // Only drop the period once the SMTP server accepted the message.
        state.db.payroll_queue.delete_entry(period).await?;
        record_last_sent_period(state, period).await;
        tracing::info!(
            "Payroll report: sent period {period} to {}",
            config.recipients.join(", ")
        );
    }
    Ok(true)
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
/// `provisional` additionally spells out that the attached report covers only
/// part of the staff, mirroring the notice printed in the PDF.
fn email_text(
    language: &Language,
    period_label: &str,
    organization: &str,
    manual: bool,
    provisional: Option<&ProvisionalNotice>,
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
    if let Some(notice) = provisional {
        text.body.push_str(&crate::i18n::translate(
            language,
            "payroll_report_email_provisional_note",
            &[
                ("included", notice.included.to_string()),
                ("total", notice.total.to_string()),
                (
                    "employees",
                    notice
                        .omitted
                        .iter()
                        .map(|person| {
                            format!(
                                "- {} ({})",
                                person.name,
                                crate::i18n::translate(language, person.reason_key, &[])
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ],
        ));
    }
    if manual {
        text.body.push_str(&crate::i18n::translate(
            language,
            "payroll_report_email_manual_note",
            &[],
        ));
    }
    text
}

/// Record a settled period so the dashboard card can show the month as done.
///
/// Only ever moves forward: catch-up runs process the oldest queued month
/// first, and a stale value would make the card claim a newer month had
/// already gone out. Failing to record must not fail the run — the report is
/// already delivered at this point — so problems are logged, not propagated.
async fn record_last_sent_period(state: &AppState, period: &str) {
    let stored = settings::load_setting(
        &state.pool,
        settings::PAYROLL_REPORT_LAST_SENT_PERIOD_KEY,
        "",
    )
    .await
    .unwrap_or_default();
    if !stored.is_empty() && !schedule::period_is_after(period, &stored) {
        return;
    }
    if let Err(e) = state
        .db
        .settings
        .save_setting(settings::PAYROLL_REPORT_LAST_SENT_PERIOD_KEY, period)
        .await
    {
        tracing::warn!("Payroll report: could not record last sent period {period}: {e}");
    }
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

    fn notice(included: usize, total: usize, names: &[&str]) -> ProvisionalNotice {
        ProvisionalNotice {
            included,
            total,
            omitted: names
                .iter()
                .map(|name| PayrollOmittedPerson {
                    name: (*name).to_string(),
                    reason_key: "payroll_report_reason_not_submitted",
                })
                .collect(),
        }
    }

    /// Every template must render in every supported language: a missing key or
    /// parameter panics, and this code runs inside a background loop.
    #[test]
    fn email_texts_render_in_every_language() {
        for code in ["en", "de"] {
            let language = crate::i18n::Language::from_setting(code);

            let email = email_text(&language, "May 2026", "Example GmbH", false, None);
            assert!(email.title.contains("May 2026"), "subject: {}", email.title);
            assert!(email.title.contains("Example GmbH"));
            assert!(email.body.contains("May 2026"));

            let partial = email_text(
                &language,
                "May 2026",
                "Example GmbH",
                true,
                Some(&notice(8, 12, &["Jane Doe"])),
            );
            assert!(partial.body.contains("Jane Doe"), "{code}: names the gap");
            assert!(partial.body.contains('8') && partial.body.contains("12"));
        }
    }

    /// A partial send must say so in the email body. Without it the payroll
    /// accountant cannot tell short figures from final ones.
    #[test]
    fn email_text_marks_provisional_sends() {
        for code in ["en", "de"] {
            let language = crate::i18n::Language::from_setting(code);

            let complete = email_text(&language, "May 2026", "Example GmbH", true, None);
            let partial = email_text(
                &language,
                "May 2026",
                "Example GmbH",
                true,
                Some(&notice(1, 2, &["Jane Doe"])),
            );

            assert!(
                partial.body.len() > complete.body.len(),
                "{code}: the provisional note is added on top of the normal body"
            );
            assert!(
                !complete.body.contains("Jane Doe"),
                "{code}: a complete report names nobody as missing"
            );
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

            let scheduled = email_text(&language, "May 2026", "Example GmbH", false, None);
            let manual = email_text(&language, "May 2026", "Example GmbH", true, None);

            assert_eq!(
                scheduled.body,
                email_text(&language, "May 2026", "Example GmbH", false, None).body,
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

    /// Every missing person must be named — unlike the old admin notification
    /// this list is not truncated, because the recipient needs the complete
    /// picture of what the attached figures leave out.
    #[test]
    fn provisional_note_lists_every_missing_person() {
        let language = crate::i18n::Language::default();
        let names: Vec<String> = (0..12).map(|index| format!("Person {index}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let text = email_text(
            &language,
            "May 2026",
            "Example GmbH",
            true,
            Some(&notice(3, 15, &refs)),
        );
        for name in &names {
            assert!(text.body.contains(name), "{name} must be listed");
        }
    }
}
