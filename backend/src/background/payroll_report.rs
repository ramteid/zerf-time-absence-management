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
//! not-yet-approved month would understate what is owed. This relaxation is
//! specific to *what goes in the document* — the dashboard tile asks a
//! stricter, unconditional question (everyone's own submitted+approved
//! status) and must not reuse it; see `services::payroll_report::evaluate_members`.
//!
//! A blocked scheduled period is **not** an error: people simply have not
//! finished their month yet. It is logged and otherwise stays silent — the
//! payroll dashboard tile (`services::payroll_report::build_status`) is where
//! admins and team leads see who is still missing.
//!
//! `run_now` (admin "Send now" button) sends exactly one month immediately,
//! skipping the day-of-month threshold, and never removes a period from the
//! queue — the regular scheduled delivery still goes out separately. Which
//! month it picks, and how much it waits for, is [`SendMode`].

use crate::background::schedule;
use crate::error::{AppError, AppResult};
use crate::i18n::Language;
use crate::report_pdf::{PayrollOmittedPerson, ProvisionalNotice};
use crate::repository::User;
use crate::services::payroll_report::{self, PayrollReportConfig};
use crate::services::settings;
use crate::AppState;
use chrono::NaiveDate;
use std::time::Duration;

/// How much a single send waits for, and what it does to the queue afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendMode {
    /// Nightly delivery. Nothing goes out until every covered person's month
    /// is final; once the SMTP server accepts it, the period leaves the queue
    /// and the month counts as delivered. This is the copy the payroll
    /// accountant files, so it must be complete.
    Scheduled,
    /// Admin "Send now" for a month that is owed but not finished yet. Sends
    /// whoever is already final and names the rest as missing, in both the PDF
    /// and the email. The period stays queued, so the complete delivery above
    /// still follows.
    ManualPartial,
    /// Admin "Send now" for the month currently running. Nobody can be final
    /// yet, so nothing is waited for: everyone who has booked something is
    /// included with their approved figures to date, and the result is marked
    /// as an interim snapshot. Touches no queue — the month is not owed yet.
    ManualSnapshot,
}

impl SendMode {
    fn is_manual(self) -> bool {
        !matches!(self, SendMode::Scheduled)
    }
}

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
    // Always backfill missed months, even before configured day.
    queue_previous_month(state, today).await?;

    process_pending_periods(state, &config, process_through_period.as_deref()).await;
    Ok(())
}

/// Result of an admin-triggered run, so the UI can say whether a report
/// actually went out or the month had nothing worth sending.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct RunSummary {
    /// Periods whose report was accepted by the SMTP server.
    pub sent: usize,
    /// Periods that produced no document (nobody final, or nobody covered).
    pub pending: usize,
    /// The month this run targeted, "YYYY-MM", so the UI can name it.
    pub period: String,
}

/// Triggered by the admin "Send now" button: sends one month right away.
///
/// The target is [`payroll_report::manual_send_target`] — the previous month
/// while its report is still owed, otherwise the month currently running. The
/// previous month is also (idempotently) queued here so a manual click never
/// lets the scheduled pipeline fall behind.
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

    let target = payroll_report::manual_send_target(&state.pool).await?;
    let mode = if target.in_progress {
        SendMode::ManualSnapshot
    } else {
        SendMode::ManualPartial
    };
    let language = crate::i18n::load_ui_language(&state.pool).await?;

    // Unlike the nightly loop, a failure here is propagated instead of being
    // swallowed into `pending`: the admin is standing in front of the button
    // and has to be told the send did not work.
    let sent = process_period(state, &target.period, &config, &language, mode).await?;
    Ok(RunSummary {
        sent: usize::from(sent),
        pending: usize::from(!sent),
        period: target.period,
    })
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
/// Errors are logged and the period retried tomorrow — this runs inside the
/// nightly loop, where there is nobody to report a failure to.
async fn process_pending_periods(
    state: &AppState,
    config: &PayrollReportConfig,
    process_through_period: Option<&str>,
) {
    let periods = match state.db.payroll_queue.list_pending().await {
        Ok(periods) => periods,
        Err(e) => {
            tracing::error!("Payroll report: failed to list queue: {e}");
            return;
        }
    };
    if periods.is_empty() {
        return;
    }

    let language = match crate::i18n::load_ui_language(&state.pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::error!("Payroll report: failed to load UI language: {e}");
            return;
        }
    };

    for period in periods {
        if schedule::period_is_deferred(&period, process_through_period) {
            tracing::debug!(
                "Payroll report: deferring period {period} until the configured day of month"
            );
            continue;
        }
        if let Err(e) =
            process_period(state, &period, config, &language, SendMode::Scheduled).await
        {
            tracing::warn!("Payroll report: skipping period {period}: {e}");
        }
    }
}

/// Build and send one period's report. Returns whether anything was sent.
///
/// What an unfinished month means depends entirely on [`SendMode`]; see its
/// variants. Everything below that decision — assembling the data, rendering
/// the PDF and handing it to SMTP — is shared.
async fn process_period(
    state: &AppState,
    period: &str,
    config: &PayrollReportConfig,
    language: &Language,
    mode: SendMode,
) -> AppResult<bool> {
    let (from, month_end) = schedule::period_bounds(period)?;
    // A snapshot reports the month "up to today", so it stops at today rather
    // than the month end. Worked hours already do this on their own (a future
    // day contributes nothing), but absence days do not: an approved holiday
    // running to the 31st would otherwise be counted in full while the hours
    // beside it stop at today, making the two halves of the same document
    // disagree. Clamping the window here keeps every section on the same date.
    let to = if mode == SendMode::ManualSnapshot {
        month_end.min(settings::app_today(&state.pool).await)
    } else {
        month_end
    };
    // Start with the same period-aware member set as the timesheet export,
    // then remove admins, explicitly excluded people, and assistants without
    // any recorded hours in this month. A snapshot of the running month drops
    // everyone who has not booked yet, not just assistants — see
    // [`payroll_report::payroll_members`].
    let members = payroll_report::payroll_members(
        state,
        from,
        to,
        &config.excluded_user_ids,
        mode == SendMode::ManualSnapshot,
    )
    .await?;

    let (included, provisional) = match mode {
        // A running month has no finality to test: every figure in it is
        // approved-to-date by construction (worked hours come from approved
        // entries, absence rows from approved absences), so everyone who
        // booked something is reported and nobody is a blocker. The notice is
        // filled in below, once the assembled document says how many people it
        // really covers.
        SendMode::ManualSnapshot => {
            if members.is_empty() {
                tracing::info!("Payroll report: period {period} has no booked time yet");
                return Ok(false);
            }
            (members, None)
        }
        SendMode::Scheduled | SendMode::ManualPartial => {
            // Full approval is only required when this person's hours literally
            // end up in the PDF — an unapproved entry that never gets printed
            // doesn't make the document wrong. (The dashboard tile asks a
            // stricter, unconditional question; see `evaluate_members`.)
            let readiness = payroll_report::evaluate_members(state, &members, from, to, |role| {
                config.includes_hours_for(role)
            })
            .await?;
            let (ready, pending): (Vec<_>, Vec<_>) = readiness
                .into_iter()
                .partition(|member| member.reason_key.is_none());

            if ready.is_empty() && pending.is_empty() {
                // The month covers nobody at all — the installation is younger
                // than the period, or everyone in it was excluded. There is
                // nothing to report, so settle the period instead of retrying
                // it every night forever and leaving the dashboard card stuck
                // on "0 of 0".
                //
                // Only the scheduled run may settle it. A manual "Send now"
                // pressed before anyone has booked the month would otherwise
                // drop the period from the queue for good, and the scheduled
                // run would never deliver it once the data did arrive.
                if !mode.is_manual() {
                    state.db.payroll_queue.delete_entry(period).await?;
                }
                tracing::info!("Payroll report: period {period} covers nobody; nothing to send");
                return Ok(false);
            }
            if !pending.is_empty() && !mode.is_manual() {
                // Not an error — people just have not finished their month.
                // Stay silent and retry tomorrow; the dashboard tile shows who
                // is missing.
                tracing::info!(
                    "Payroll report: period {period} still waiting for {} of {} people",
                    pending.len(),
                    ready.len() + pending.len()
                );
                return Ok(false);
            }
            if ready.is_empty() {
                // A manual send while nobody has finished yet: an empty
                // document helps the payroll accountant no more than none.
                tracing::info!("Payroll report: period {period} has no finalized people yet");
                return Ok(false);
            }

            // Only a manual send can get here with people missing, and only
            // then does the report need the "this is partial" marker.
            let notice = (!pending.is_empty()).then(|| ProvisionalNotice {
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
                in_progress: false,
            });
            let included: Vec<User> = ready.into_iter().map(|member| member.user).collect();
            (included, notice)
        }
    };
    let mut data = payroll_report::build_report_data(
        state,
        from,
        to,
        &included,
        config,
        language,
        provisional,
    )
    .await?;

    if mode == SendMode::ManualSnapshot {
        // Having booked time is not the same as having anything to report:
        // only approved entries and approved absences reach the tables, and
        // mid-month the current week is usually still unapproved. Without this
        // guard the tax office would receive a document with nothing but
        // headings in it, announced as covering N people. The partial send
        // refuses an empty report for the same reason.
        let covered = payroll_report::people_in_report(&data);
        if covered == 0 {
            tracing::info!("Payroll report: period {period} has nothing approved yet");
            return Ok(false);
        }
        // Count the people the document actually names, not everyone who
        // happens to have booked something this month.
        data.provisional = Some(ProvisionalNotice {
            included: covered,
            total: covered,
            omitted: Vec::new(),
            in_progress: true,
        });
    }

    let bytes = crate::report_pdf::render_payroll_report_pdf(&data, language);
    if bytes.is_empty() {
        return Err(AppError::Internal(format!(
            "Generated payroll report PDF is empty for period {period}"
        )));
    }

    let Some(smtp) = settings::load_smtp_config(&state.pool).await else {
        // SMTP disabled after queue listing – leave queued for next cycle
        // (mirrors email_queue worker behavior).
        tracing::info!("Payroll report: SMTP not configured at send time, deferring period {period}");
        return Ok(false);
    };

    let text = email_text(
        language,
        &data.period_label,
        &organization_label(state, language).await,
        mode.is_manual(),
        data.provisional.as_ref(),
    );

    crate::email::send_with_attachment(
        &state.email_circuit_breaker,
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
    .map_err(|e| {
        tracing::error!("Payroll report: sending period {period} failed: {e}");
        if mode.is_manual() {
            // An admin is waiting on the button and has to be told what broke.
            // `Internal` deliberately hides its message from the client, so a
            // manual send reports the reason as a prefixed BadRequest instead
            // (the frontend splits the prefix off and translates the lead-in).
            AppError::BadRequest(format!("PAYROLL_SEND_FAILED:{e}"))
        } else {
            AppError::Internal(format!("Payroll report email failed: {e}"))
        }
    })?;

    if mode.is_manual() {
        // A manual "Send now" copy never replaces the scheduled delivery: the
        // period stays queued, so the automatic run still sends the regular
        // copy for this month on the configured day. For the same reason it
        // must not mark the period as sent — the dashboard tile has to keep
        // showing the outstanding delivery. (A snapshot of the running month
        // has no queue entry to begin with, so this is equally a no-op there.)
        tracing::info!(
            "Payroll report: sent period {period} manually to {} (period stays queued for the scheduled run)",
            config.recipients.join(", ")
        );
    } else {
        // Only drop the period once the SMTP server accepted the message.
        // Removing it is also what tells the dashboard card the month is
        // done. The email is already gone at this point — a bare delete
        // that gives up after one transient DB hiccup would leave the
        // period looking un-sent and cause the whole report to go out to
        // the tax office / payroll accountant a second time tomorrow, so
        // retry the (idempotent) delete a few times before accepting that
        // risk.
        delete_payroll_period_with_retry(state, period).await?;
        tracing::info!(
            "Payroll report: sent period {period} to {}",
            config.recipients.join(", ")
        );
    }
    Ok(true)
}

/// Delay between delete attempts. Short: this only needs to ride out a
/// momentary DB hiccup, not a real outage.
const DELETE_RETRY_DELAY: Duration = Duration::from_secs(1);

/// Number of delete attempts before giving up and accepting the period will
/// be reported again on the next run.
const DELETE_RETRY_ATTEMPTS: u32 = 3;

/// Retry `payroll_queue.delete_entry` a few times. The DELETE is idempotent
/// (removing an already-gone period is a harmless no-op), so retrying is
/// always safe and never risks double-deleting — it only closes the window
/// in which a transient DB failure would otherwise leave an already-sent
/// period looking outstanding.
async fn delete_payroll_period_with_retry(state: &AppState, period: &str) -> AppResult<()> {
    for _ in 1..DELETE_RETRY_ATTEMPTS {
        if state.db.payroll_queue.delete_entry(period).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(DELETE_RETRY_DELAY).await;
    }
    state.db.payroll_queue.delete_entry(period).await
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
/// `provisional` additionally spells out why the attached report is not final,
/// mirroring the notice printed in the PDF: either it covers only part of the
/// staff, or the reported month itself is still running.
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
        // Ensure newline separation even if translation lacks leading newlines.
        if !text.body.ends_with('\n') {
            text.body.push_str("\n\n");
        }
        // A snapshot of the running month must not reuse the partial-send
        // wording: with nobody omitted it would claim "covers N of N people"
        // and then print an empty list of who is missing.
        let note = if notice.in_progress {
            crate::i18n::translate(
                language,
                "payroll_report_email_snapshot_note",
                &[("included", notice.included.to_string())],
            )
        } else {
            crate::i18n::translate(
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
            )
        };
        text.body.push_str(&note);
    }
    if manual {
        if !text.body.ends_with('\n') {
            text.body.push_str("\n\n");
        }
        text.body.push_str(&crate::i18n::translate(
            language,
            "payroll_report_email_manual_note",
            &[],
        ));
    }
    text
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
            in_progress: false,
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

    /// A snapshot of the running month has nobody omitted, so it must not take
    /// the partial-send wording: that would claim "covers N of N people" and
    /// then print an empty list of who is still missing.
    #[test]
    fn snapshot_note_does_not_claim_people_are_missing() {
        for code in ["en", "de"] {
            let language = crate::i18n::Language::from_setting(code);
            let snapshot_notice = ProvisionalNotice {
                included: 5,
                total: 5,
                omitted: Vec::new(),
                in_progress: true,
            };
            let snapshot = email_text(
                &language,
                "August 2026",
                "Example GmbH",
                true,
                Some(&snapshot_notice),
            );
            let partial = email_text(
                &language,
                "August 2026",
                "Example GmbH",
                true,
                Some(&notice(5, 5, &[])),
            );
            assert_ne!(
                snapshot.body, partial.body,
                "{code}: a snapshot must not reuse the partial-send wording"
            );
            // The partial wording is the one that spells out a "x of y" split;
            // the snapshot only ever states how many people it covers.
            assert!(
                partial.body.contains("5") && snapshot.body.contains("5"),
                "{code}: both state the count"
            );
            assert!(
                !snapshot.body.contains(" 5 von 5 ") && !snapshot.body.contains(" 5 of 5 "),
                "{code}: a snapshot never frames the count as a partial split"
            );
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
