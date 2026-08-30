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
use chrono::{Datelike, NaiveDate};
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

/// Why a run produced no email. Kept distinct rather than collapsed into a
/// single "nothing happened" so the admin standing in front of the button is
/// told the actual reason: "nobody has finished the month" and "nothing has
/// been approved yet" call for completely different action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The month covers nobody at all — too early for this installation, or
    /// everybody in it is on the exclusion list.
    CoversNobody,
    /// People are covered but none of them has a final month yet, so a
    /// partial report would name nobody.
    NobodyFinal,
    /// People booked time in the running month, but none of it is approved
    /// yet, so the snapshot would be an empty document.
    NothingApproved,
    /// The people covered produced no rows at all — nothing worth reporting
    /// happened in the month.
    NothingToReport,
    /// Email delivery stopped being configured between the check and the send.
    EmailUnavailable,
}

/// Result of building and sending one period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendOutcome {
    Sent,
    Skipped(SkipReason),
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
    /// Periods that produced no document.
    pub pending: usize,
    /// The month this run targeted, "YYYY-MM", so the UI can name it.
    pub period: String,
    /// Why nothing was sent, when nothing was. `None` once a report went out.
    pub skipped: Option<SkipReason>,
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
    let outcome = process_period(state, &target.period, &config, &language, mode).await?;
    let skipped = match outcome {
        SendOutcome::Sent => None,
        SendOutcome::Skipped(reason) => Some(reason),
    };
    Ok(RunSummary {
        sent: usize::from(skipped.is_none()),
        pending: usize::from(skipped.is_some()),
        period: target.period,
        skipped,
    })
}

/// Queue every month from the last recorded one through the previous month.
async fn queue_previous_month(state: &AppState, today: NaiveDate) -> AppResult<()> {
    // Read before the backfill runs: an empty marker here means nothing has
    // ever been queued before, so whatever this call queues is, by
    // definition, the earliest period the feature has ever considered. That
    // period becomes the permanent floor `carry_over_boundary` clamps to, so
    // the very first report this installation ever sends cannot reach back
    // into pre-existing history it was never meant to carry.
    let first_run =
        settings::load_setting(&state.pool, settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY, "")
            .await?
            .is_empty();

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
    .await?;

    if first_run {
        // The floor is set once and never touched again — an idempotent
        // insert-only write, not tied to whether this particular call ended
        // up queuing anything (a re-run against an already-populated queue
        // must not move the floor).
        let floor =
            settings::load_setting(&state.pool, settings::PAYROLL_REPORT_FIRST_PERIOD_KEY, "")
                .await?;
        if floor.is_empty() {
            let earliest = schedule::previous_period(today);
            state
                .db
                .settings
                .save_setting(settings::PAYROLL_REPORT_FIRST_PERIOD_KEY, &earliest)
                .await?;
        }
    }
    Ok(())
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
        if let Err(e) = process_period(state, &period, config, &language, SendMode::Scheduled).await
        {
            tracing::warn!("Payroll report: skipping period {period}: {e}");
        }
    }
}

/// Tell the administrators once that a scheduled report is being held back.
///
/// Once per period, never per night: the loop reaches a blocked period again
/// every day until the month is finished, and a warning that repeats daily is
/// one nobody reads. The dashboard card carries the live picture; this is the
/// nudge that makes somebody go and look at it.
async fn notify_admins_of_hold(
    state: &AppState,
    period: &str,
    language: &Language,
    ready: usize,
    pending: &[payroll_report::MemberReadiness],
) {
    // Compared with `>=`, not `==`: two blocked periods are processed oldest
    // first, so an equality check would let the marker flip between them and
    // reconsider both every night. "YYYY-MM" sorts chronologically as a string,
    // so a period at or below the last one reported has already been covered.
    let already_reported = settings::load_setting(
        &state.pool,
        settings::PAYROLL_REPORT_BLOCKED_NOTIFIED_KEY,
        "",
    )
    .await
    .unwrap_or_default();
    if already_reported.as_str() >= period {
        return;
    }

    let month_label = match schedule::period_bounds(period) {
        Ok((from, _)) => crate::i18n::format_month(language, from.year(), from.month()),
        Err(_) => period.to_string(),
    };
    let names = pending
        .iter()
        .map(|member| format!("{} {}", member.user.first_name, member.user.last_name))
        .collect::<Vec<_>>()
        .join(", ");
    let params = [
        ("month", month_label),
        ("count", pending.len().to_string()),
        ("total", (ready + pending.len()).to_string()),
        ("names", names),
    ];
    let text = crate::i18n::notification_event_text(language, "payroll_report_blocked", &params);
    let email_body =
        crate::i18n::notification_email_body(language, "payroll_report_blocked", &params);

    let admin_ids = state.db.users.active_admin_ids().await.unwrap_or_default();
    for admin_id in admin_ids {
        crate::services::notifications::deliver(
            state,
            &crate::services::notifications::Outgoing::new(
                admin_id,
                language,
                "payroll_report_blocked",
                &text.title,
                &text.body,
            )
            .email_body(&email_body)
            .dedupe_key(&format!("payroll_report_blocked:{period}")),
        )
        .await;
    }

    // Recorded even when there are no admins to tell: the marker says "this
    // period has been handled", and re-running the lookup nightly for an
    // installation without admins would achieve nothing.
    if let Err(e) = state
        .db
        .settings
        .save_setting(settings::PAYROLL_REPORT_BLOCKED_NOTIFIED_KEY, period)
        .await
    {
        tracing::warn!("Payroll report: failed to record the hold notice for {period}: {e}");
    }
}

/// Build and send one period's report.
///
/// Returns [`SendOutcome`]: either it went out, or why it did not.
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
) -> AppResult<SendOutcome> {
    let (from, month_end) = schedule::period_bounds(period)?;
    // Read once and reuse: the clamp below, the date printed on the report and
    // the date in its filename all have to be the same day, and asking three
    // times leaves a window where a run crossing midnight disagrees with
    // itself.
    let today = settings::app_today(&state.pool).await;
    // A snapshot reports the month "up to today", so it stops at today rather
    // than the month end. Worked hours already do this on their own (a future
    // day contributes nothing), but absence days do not: an approved holiday
    // running to the 31st would otherwise be counted in full while the hours
    // beside it stop at today, making the two halves of the same document
    // disagree. Clamping the window here keeps every section on the same date.
    let to = if mode == SendMode::ManualSnapshot {
        month_end.min(today)
    } else {
        month_end
    };
    // Start with the same period-aware member set as the timesheet export,
    // then remove admins, explicitly excluded people, and assistants without
    // any recorded hours in this month. A snapshot of the running month drops
    // everyone who has not booked yet, not just assistants — see
    // [`payroll_report::payroll_members`].
    // Days booked after their own month's report went out are carried by this
    // one, so whoever holds such a day belongs to the member set even when they
    // did nothing in this period. `carry_over_boundary` already asks what a
    // report produced now would still have to carry, never what an earlier
    // one already did.
    let carried = payroll_report::carry_over_boundary(&state.pool, from).await?;
    let members = payroll_report::payroll_members(
        state,
        from,
        to,
        &config.excluded_user_ids,
        mode == SendMode::ManualSnapshot,
        carried.as_ref(),
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
                return Ok(SendOutcome::Skipped(SkipReason::NothingApproved));
            }
            (members, None)
        }
        SendMode::Scheduled | SendMode::ManualPartial => {
            // Full approval is only required when this person's hours literally
            // end up in the PDF — an unapproved entry that never gets printed
            // doesn't make the document wrong. (The dashboard tile asks a
            // stricter, unconditional question; see `evaluate_members`.)
            // No week criterion: an unhanded-in week proves nothing about a
            // payroll month — see `reports::month_export_readiness`.
            let readiness = payroll_report::evaluate_members(
                state,
                &members,
                from,
                to,
                |role| {
                    if config.includes_hours_for(role) {
                        // A booking that exists but is not approved is proof of
                        // work whose hours the document would be missing.
                        crate::services::reports::UnapprovedEntries::AnyUnsettled
                    } else {
                        crate::services::reports::UnapprovedEntries::NotRequired
                    }
                },
                false,
                // Only what this document would actually print.
                crate::services::reports::PendingAbsences::PayrollRelevant,
            )
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
                    // Settled counts as accounted for: without the mark, this
                    // month's entries would look like late bookings to every
                    // later report.
                    // Nothing was printed, so no older day was carried: only
                    // this month's own entries may be marked.
                    if mark_reported_entries(
                        state,
                        period,
                        from,
                        month_end,
                        carried.as_ref(),
                        &[],
                        &[],
                    )
                    .await
                    {
                        state.db.payroll_queue.delete_entry(period).await?;
                    }
                }
                tracing::info!("Payroll report: period {period} covers nobody; nothing to send");
                return Ok(SendOutcome::Skipped(SkipReason::CoversNobody));
            }
            if !pending.is_empty() && !mode.is_manual() {
                // Not an error — people just have not finished their month, and
                // the period simply stays queued and is retried tomorrow. But
                // the send day has passed by the time we get here (a deferred
                // period never reaches this far), so the administrators are
                // told once that the report is on hold and who is holding it.
                tracing::info!(
                    "Payroll report: period {period} still waiting for {} of {} people",
                    pending.len(),
                    ready.len() + pending.len()
                );
                notify_admins_of_hold(state, period, language, ready.len(), &pending).await;
                return Ok(SendOutcome::Skipped(SkipReason::NobodyFinal));
            }
            if ready.is_empty() {
                // A manual send while nobody has finished yet: an empty
                // document helps the payroll accountant no more than none.
                tracing::info!("Payroll report: period {period} has no finalized people yet");
                return Ok(SendOutcome::Skipped(SkipReason::NobodyFinal));
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
        payroll_report::ReportWindow {
            from,
            to,
            interim: mode == SendMode::ManualSnapshot,
            created_on: today,
            carried: carried.clone(),
        },
        &included,
        config,
        language,
        provisional,
    )
    .await?;

    // Being covered by the report is not the same as appearing in it: only
    // approved entries and approved absences produce rows. A document with
    // nothing but headings tells the tax office nothing, so no mode sends one
    // — mid-month that is the normal state of a snapshot, and for a finished
    // month it means nothing reportable happened at all.
    let covered = payroll_report::people_in_report(&data);
    if covered == 0 {
        let reason = if mode == SendMode::ManualSnapshot {
            SkipReason::NothingApproved
        } else {
            SkipReason::NothingToReport
        };
        // A scheduled month with nothing in it is settled rather than retried
        // forever, exactly like a month covering nobody: the data will not
        // appear later, because every covered person is already final.
        //
        // Note the consequence: "delivered" is derived from the queue, so the
        // dashboard tile will call this month sent even though no mail went
        // out. That is the same trade the covers-nobody branch already makes,
        // and it is the honest half of the choice — the alternative is a month
        // that stays outstanding on the tile and is retried every night for
        // ever, for a report that will never have anything in it. A plain
        // delete (not the post-send retry helper) is right here: nothing was
        // sent, so a failed delete costs one wasted retry tomorrow rather than
        // risking a second copy reaching the tax office.
        if !mode.is_manual() {
            // Same as the covers-nobody branch: a period that will never be
            // sent is still done with, and its entries must not resurface as
            // catch-up days next month.
            if mark_reported_entries(state, period, from, month_end, carried.as_ref(), &[], &[])
                .await
            {
                state.db.payroll_queue.delete_entry(period).await?;
            }
        }
        tracing::info!("Payroll report: period {period} has nothing to report; nothing sent");
        return Ok(SendOutcome::Skipped(reason));
    }

    if mode == SendMode::ManualSnapshot {
        // Name the people the document actually lists, not everyone who
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
        tracing::info!(
            "Payroll report: SMTP not configured at send time, deferring period {period}"
        );
        return Ok(SendOutcome::Skipped(SkipReason::EmailUnavailable));
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
            // The creation date is part of the name because the same month
            // can legitimately be sent more than once — an interim snapshot
            // and later the final report — and the recipient must not have two
            // identically named attachments whose contents differ.
            filename: format!("{period}_payroll_report_{}.pdf", today.format("%Y-%m-%d")),
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
        // Record what this report accounted for before the period leaves the
        // queue, so the two agree even if the delete below has to be retried.
        // Marking is what stops the catch-up days above from being printed
        // again next month; failing to mark costs a duplicated line there,
        // which is why it is only logged and never fails an already-sent
        // report.
        // Only the people whose hours this document actually printed may have
        // an older day marked as carried. Marking anybody else's would claim a
        // report accounted for hours it never showed — and would make a genuine
        // late booking uncatchable if the setting that prints them is ever
        // switched on.
        let carried_user_ids: Vec<i64> = included
            .iter()
            .filter(|member| config.includes_hours_for(&member.role))
            .map(|member| member.id)
            .collect();
        // Absences follow the document's own rule instead: it prints them for
        // everybody except assistants, whatever the hours settings say.
        let absence_user_ids: Vec<i64> = included
            .iter()
            .filter(|member| !crate::roles::is_assistant_role(&member.role))
            .map(|member| member.id)
            .collect();
        if mark_reported_entries(
            state,
            period,
            from,
            month_end,
            carried.as_ref(),
            &carried_user_ids,
            &absence_user_ids,
        )
        .await
        {
            delete_payroll_period_with_retry(state, period).await?;
        }
        tracing::info!(
            "Payroll report: sent period {period} to {}",
            config.recipients.join(", ")
        );
    }
    Ok(SendOutcome::Sent)
}

/// Mark every time entry this period's report accounted for.
///
/// Retried like the queue delete and for the same reason: the email is already
/// gone, and an unmarked entry would show up as a late booking in next month's
/// report even though the tax office has it. Returns whether it succeeded, and
/// the caller leaves the period queued if it did not: a duplicate copy of the
/// same month is something the recipient can recognise, whereas the same hours
/// appearing again a month later under an older date reads like new work and
/// could be paid twice.
///
/// This cannot loop for long. Reaching this point means dozens of queries
/// building the report already succeeded, so a failure here is a momentary one;
/// the next run re-sends the identical document and settles the period.
async fn mark_reported_entries(
    state: &AppState,
    period: &str,
    period_start: NaiveDate,
    period_end: NaiveDate,
    carried: Option<&payroll_report::CarriedDays>,
    carried_user_ids: &[i64],
    absence_user_ids: &[i64],
) -> bool {
    // Absences first: they use their own marker with its own rule (the *first*
    // report that showed any part of an absence), and a failure here must stop
    // the period being settled just as an entry-marking failure does.
    for attempt in 1..=DELETE_RETRY_ATTEMPTS {
        match state
            .db
            .reports
            .mark_payroll_reported_absences(period, period_start, period_end, absence_user_ids)
            .await
        {
            Ok(marked) => {
                tracing::debug!(
                    "Payroll report: marked {marked} absences as reported for {period}"
                );
                break;
            }
            Err(e) if attempt < DELETE_RETRY_ATTEMPTS => {
                tracing::warn!(
                    "Payroll report: marking absences for {period} failed (attempt {attempt}): {e}"
                );
                tokio::time::sleep(DELETE_RETRY_DELAY).await;
            }
            Err(e) => {
                tracing::error!(
                    "Payroll report: could not mark absences for {period}: {e}. \
                     The period stays queued so nothing is silently declared twice."
                );
                return false;
            }
        }
    }
    for attempt in 1..=DELETE_RETRY_ATTEMPTS {
        match state
            .db
            .time_entries
            .mark_payroll_reported(
                period,
                period_start,
                period_end,
                crate::repository::PayrollCarryScope {
                    since: carried.map(|c| c.since),
                    before: carried.map(|c| c.before),
                    owed_periods: carried.map(|c| c.owed_periods.as_slice()).unwrap_or(&[]),
                    user_ids: carried_user_ids,
                },
            )
            .await
        {
            Ok(marked) => {
                tracing::debug!(
                    "Payroll report: marked {marked} time entries as reported for {period}"
                );
                return true;
            }
            Err(e) if attempt < DELETE_RETRY_ATTEMPTS => {
                tracing::warn!(
                    "Payroll report: marking period {period} failed (attempt {attempt}): {e}"
                );
                tokio::time::sleep(DELETE_RETRY_DELAY).await;
            }
            Err(e) => tracing::error!(
                "Payroll report: could not mark period {period} as reported: {e}. \
                 The period stays queued so nothing is silently paid twice."
            ),
        }
    }
    false
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
