//! Monthly payroll report: configuration and data assembly.
//!
//! The report is one PDF per month for the whole company, emailed to the
//! payroll accountant / tax office. It replaces a hand-maintained spreadsheet
//! and therefore contains exactly what payroll needs to file:
//!   * absence days per employee for the categories that are automatically
//!     payroll-relevant (see [`AbsenceCategory::is_payroll_relevant`]) — sick
//!     days drive health-insurance reimbursement, unpaid days reduce the
//!     salary payout,
//!   * working days and worked hours per assistant (and optionally per
//!     employee), which is what assistants are paid by.
//!
//! Scheduling lives in `background::payroll_report`; this module only decides
//! *what* is in the document.

use crate::error::AppResult;
use crate::i18n::{self, Language};
use crate::report_pdf::{
    PayrollAbsenceRow, PayrollCarriedWorkDay, PayrollDeclaredWorkDay, PayrollHoursRow,
    PayrollHoursSection, PayrollLateEntryRow, PayrollReportData,
};
use crate::repository::{AbsenceCategory, AbsenceCategoryDb, PayrollReportedContentRow, User};
use crate::roles::is_assistant_role;
use crate::services::reports::MonthExportReadiness;
use crate::services::settings;
use crate::time_calc::count_workdays;
use crate::AppState;
use chrono::{Datelike, NaiveDate, NaiveTime};

/// Heading translation key of the assistants' working-hours table.
pub const ASSISTANT_HOURS_HEADING_KEY: &str = "pdf_payroll_assistant_hours_heading";
/// Heading translation key of the employees' working-hours table.
pub const EMPLOYEE_HOURS_HEADING_KEY: &str = "pdf_payroll_employee_hours_heading";

/// Admin-configured content and schedule of the payroll report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayrollReportConfig {
    pub enabled: bool,
    /// Recipient addresses, in stored order. All recipients are equal — every
    /// address goes in the message's `To` header, none is primary/CC.
    pub recipients: Vec<String>,
    /// Day of month on which the previous month is queued (1-28).
    pub day_of_month: u8,
    pub include_assistant_hours: bool,
    pub include_employee_hours: bool,
    /// People the admin deliberately left out of the report. They neither
    /// appear in the document nor hold its delivery up.
    pub excluded_user_ids: Vec<i64>,
}

impl PayrollReportConfig {
    /// True when the report would contain no section at all. Such a
    /// configuration is rejected on save and skipped by the scheduler, because
    /// mailing an empty document to the tax office helps nobody. The absence
    /// section's presence depends on whether any category currently
    /// qualifies as payroll-relevant — pass the result of
    /// [`payroll_relevant_categories`].
    pub fn has_no_content(&self, relevant_categories: &[AbsenceCategory]) -> bool {
        relevant_categories.is_empty()
            && !self.include_assistant_hours
            && !self.include_employee_hours
    }

    /// Whether this user's working days and hours appear in the report.
    /// Drives both the rendered sections and the stricter readiness gate: hours
    /// are only meaningful once every entry behind them is approved.
    pub fn includes_hours_for(&self, role: &str) -> bool {
        if is_assistant_role(role) {
            self.include_assistant_hours
        } else {
            self.include_employee_hours
        }
    }
}

/// One signed correction the next report would declare for an older workday.
/// Kept separate from the rendered row so member selection, the PDF and the
/// post-send ledger write all use the same person-day values.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LateEntryDelta {
    user_id: i64,
    date: NaiveDate,
    minutes: i64,
    /// False for a pre-migration day whose historic baseline is unknowable.
    /// Such a day stays on the entry-marker fallback permanently.
    ledger_backed: bool,
}

#[derive(Default)]
struct LateEntryRows {
    rows: Vec<PayrollLateEntryRow>,
    declared_work_days: Vec<PayrollDeclaredWorkDay>,
    carried_work_days: Vec<PayrollCarriedWorkDay>,
}

#[derive(Default)]
struct HoursRows {
    rows: Vec<PayrollHoursRow>,
    declared_work_days: Vec<PayrollDeclaredWorkDay>,
}

#[derive(Default)]
struct AbsenceRows {
    rows: Vec<PayrollAbsenceRow>,
    ids: Vec<i64>,
}

pub async fn load_config(pool: &crate::db::DatabasePool) -> AppResult<PayrollReportConfig> {
    Ok(PayrollReportConfig {
        enabled: settings::load_setting(pool, settings::PAYROLL_REPORT_ENABLED_KEY, "false")
            .await?
            == "true",
        recipients: parse_recipient_list(
            &settings::load_setting(pool, settings::PAYROLL_REPORT_RECIPIENT_KEY, "").await?,
        ),
        day_of_month: settings::load_setting(pool, settings::PAYROLL_REPORT_DAY_OF_MONTH_KEY, "5")
            .await?
            .parse::<u8>()
            .unwrap_or(5)
            .clamp(1, 28),
        include_assistant_hours: settings::load_setting(
            pool,
            settings::PAYROLL_REPORT_ASSISTANT_HOURS_KEY,
            "true",
        )
        .await?
            == "true",
        include_employee_hours: settings::load_setting(
            pool,
            settings::PAYROLL_REPORT_EMPLOYEE_HOURS_KEY,
            "false",
        )
        .await?
            == "true",
        excluded_user_ids: parse_excluded_ids(
            &settings::load_setting(pool, settings::PAYROLL_REPORT_EXCLUDED_USERS_KEY, "").await?,
        ),
    })
}

/// The people a payroll report for this period actually covers.
///
/// Three groups never appear, and therefore never block delivery either:
///   * **admins** — they are the ones running the system, not staff the payroll
///     accountant files for, so they are dropped unconditionally;
///   * anyone the admin put on the exclusion list.
///   * assistants without any time entry in this period. Assistants have no
///     fixed target, so an empty month needs no declaration and is complete by
///     definition. Any recorded entry makes the assistant relevant, regardless
///     of whether it is still a draft, submitted, approved, or rejected.
///
/// `everyone_needs_recorded_time` widens that last rule from assistants to
/// everybody. It is only set for an interim snapshot of the *running* month,
/// where somebody who simply has not got round to booking yet is not missing
/// anything — the month is not over. For a finished month the opposite is
/// true: an employee with an empty month still owes a declaration, so they
/// stay in and the default (`false`) keeps them.
///
/// `carried` additionally admits people who booked a day of an already-reported
/// month too late for that month's report (see [`CarriedDays`]). `None` asks
/// the plain question "who does this period concern" — which is what the
/// Submissions tile wants, since a late booking from a closed month says
/// nothing about whether this month is closed.
///
/// Report content, the readiness gate and the dashboard tile all go through
/// this one filter, so what the tile counts is exactly what the PDF contains.
pub async fn payroll_members(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    excluded_user_ids: &[i64],
    everyone_needs_recorded_time: bool,
    carried: Option<&CarriedDays>,
) -> AppResult<Vec<User>> {
    // Neither set is role-filtered — they cover every user with activity in
    // the period, which is what lets the same lookup serve the assistant-only
    // rule and the snapshot's everyone rule.
    let users_with_entries = app_state
        .db
        .reports
        .user_ids_with_time_entries_in_range(from, to)
        .await?;
    // Also include people who have only payroll-relevant absences (e.g. sick
    // for the whole month) – otherwise their sick days vanish.
    let users_with_payroll_absences = app_state
        .db
        .reports
        .user_ids_with_payroll_absences_in_range(from, to)
        .await?;
    // A person whose only activity is an older correction has no current-month
    // row to bring them in. Work from the signed delta calculation rather than
    // from every unmarked entry: an exact rebook has a zero difference and is
    // neither a report row nor a permanent phantom member.
    let late_entry_deltas = match carried {
        Some(carried) if carried.reported_as.is_none() => {
            late_entry_deltas(app_state, carried).await?
        }
        _ => Vec::new(),
    };
    let late_entry_user_ids: std::collections::HashSet<i64> = late_entry_deltas
        .iter()
        .filter(|delta| delta.minutes != 0)
        .map(|delta| delta.user_id)
        .collect();
    let late_members = match carried {
        Some(carried) => match carried.reported_as.as_deref() {
            // A post-ledger report reads its historical people from the exact
            // declaration rows, so a later correction or archived account
            // cannot rewrite who the delivered PDF named. Older reports have
            // no declaration rows and retain the entry-marker readback.
            Some(period) => {
                let mut members = app_state
                    .db
                    .reports
                    .users_with_declared_days_for_period(period)
                    .await?;
                let legacy = app_state
                    .db
                    .reports
                    .users_with_carried_time_entries_before(
                        Some(period),
                        carried.since,
                        carried.before,
                        &carried.owed_periods,
                    )
                    .await?;
                let mut known: std::collections::HashSet<i64> =
                    members.iter().map(|member| member.id).collect();
                for member in legacy {
                    if known.insert(member.id) {
                        members.push(member);
                    }
                }
                members
            }
            None => {
                let mut members = Vec::with_capacity(late_entry_user_ids.len());
                for user_id in &late_entry_user_ids {
                    if let Some(member) = app_state.db.users.find_by_id(*user_id).await? {
                        members.push(member);
                    }
                }
                members
            }
        },
        None => Vec::new(),
    };
    // Absences need the same widening, and for the same reason: a sick note
    // filed after somebody left is exactly when a last one tends to arrive, and
    // the period-scoped query below cannot see a person who is no longer active
    // and has nothing in this month.
    let late_absence_members = match carried {
        Some(carried) if carried.reported_as.is_none() => {
            app_state
                .db
                .reports
                .users_with_carried_absences_before(carried.since, carried.before)
                .await?
        }
        _ => Vec::new(),
    };
    // Kept apart on purpose. A carried *entry* admits anybody whose hours the
    // report prints, assistants included. A carried *absence* must not admit an
    // assistant: the document never prints an assistant's absence, so it would
    // pull somebody into every future report's covered set who can never
    // produce a row — and, since only non-assistants' absences are ever marked
    // as declared, would keep doing so for ever.
    let users_with_late_entries: std::collections::HashSet<i64> =
        if carried.is_some_and(|carried| carried.reported_as.is_none()) {
            late_entry_user_ids
        } else {
            late_members.iter().map(|member| member.id).collect()
        };
    let users_with_late_absences: std::collections::HashSet<i64> = late_absence_members
        .iter()
        .map(|member| member.id)
        .collect();
    let mut members = app_state.db.reports.timesheet_members_for_period(to).await?;
    // The period-scoped query only knows people who are still active and
    // tracking time, so an assistant who has left since is missing from it.
    // Their unpaid day is exactly what this is for.
    let known: std::collections::HashSet<i64> = members.iter().map(|member| member.id).collect();
    // `known` grows as they are added: somebody holding both a carried day and
    // a carried absence appears in both lists, and adding them twice would
    // duplicate every row they produce.
    let mut known = known;
    for member in late_members.into_iter().chain(late_absence_members) {
        if known.insert(member.id) {
            members.push(member);
        }
    }
    // Restore the surname ordering the caller relies on for the printed lists.
    members.sort_by(|left, right| {
        left.last_name
            .cmp(&right.last_name)
            .then_with(|| left.first_name.cmp(&right.first_name))
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(members
        .into_iter()
        .filter(|member| {
            let is_assistant = is_assistant_role(&member.role);
            let needs_recorded_time = everyone_needs_recorded_time || is_assistant;
            // An absence only makes somebody relevant when the report would
            // print it, which for an assistant it never does — they are paid by
            // the hour, so only recorded time can bring them in.
            let has_relevant_data = users_with_entries.contains(&member.id)
                || users_with_late_entries.contains(&member.id)
                || (!is_assistant
                    && (users_with_payroll_absences.contains(&member.id)
                        || users_with_late_absences.contains(&member.id)));
            !crate::roles::is_admin_role(&member.role)
                && !excluded_user_ids.contains(&member.id)
                && (!needs_recorded_time || has_relevant_data)
        })
        .collect())
}

/// Whether this period's report has already gone out in full.
///
/// Derived from the queue instead of a stored "last sent" marker: a period is
/// queued when its turn comes and removed only once the SMTP server accepted a
/// *complete* report, so a period that reached the queue and is no longer in it
/// is genuinely done. An admin's interim "Send now" copy deliberately leaves
/// the entry in place, so an early partial send never makes a month look
/// finished. Deriving it this way is also what makes the answer correct on
/// installations that predate the queue-period marker.
pub async fn period_delivered(pool: &crate::db::DatabasePool, period: &str) -> AppResult<bool> {
    let queued_through =
        settings::load_setting(pool, settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY, "").await?;
    let reached_the_queue = !queued_through.is_empty()
        && (queued_through == period
            || crate::background::schedule::period_is_after(&queued_through, period));
    if !reached_the_queue {
        return Ok(false);
    }
    let still_queued = crate::repository::PayrollReportQueueDb::new(pool.clone())
        .list_pending()
        .await?
        .iter()
        .any(|queued| queued == period);
    Ok(!still_queued)
}

/// Which days from earlier months a report shows in its catch-up section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarriedDays {
    /// Never below this date. Always the start of the very first period the
    /// payroll report ever queued (`PAYROLL_REPORT_FIRST_PERIOD_KEY`), fixed
    /// forever once that first period exists. Without it, an installation's
    /// first-ever report would treat "nothing has been queued before" as "no
    /// lower limit at all", sweeping in every approved entry created between
    /// the `payroll_reported_period` migration and the day payroll reporting
    /// was actually turned on — which for an org that already used Zerf for
    /// time tracking could be months of unrelated history.
    pub since: NaiveDate,
    /// Only days before this date — always the reported month's own start,
    /// since everything from there on belongs to its regular sections.
    pub before: NaiveDate,
    /// Months ("YYYY-MM") whose own report has not gone out yet. Days in them
    /// are skipped: that report will print them itself, and carrying them here
    /// as well would send the same hours twice.
    ///
    /// A list rather than a single "oldest owed" cut-off, because the queue can
    /// have gaps. With March stuck behind a late approval while April and May
    /// were delivered, a cut-off at March would also freeze a genuine late
    /// booking in April — for as long as March stayed stuck.
    pub owed_periods: Vec<String>,
    /// `None` asks what a report produced now would carry. `Some(period)` asks
    /// what that period's report actually carried, read back from its declared
    /// day ledger (or the legacy entry marker for reports sent before it).
    ///
    /// The difference matters for a month that has already gone out: asking the
    /// first question about it would list days booked *since* the send as
    /// though the tax office had received them, when they are in fact still
    /// waiting for the next report. `Some(period)` also switches the *regular*
    /// hours sections from a live recompute to reading back the same recorded
    /// declarations (`sent_hours_rows`) for exactly this reason — the hours,
    /// not only the correction rows, must describe what was mailed, not the
    /// entries' current state.
    ///
    /// Absences answer only the first question. Their marker records the
    /// *first* period that showed any part of an absence — which is what lets
    /// one column serve a period spanning a month boundary — so it cannot say
    /// what a given period carried, and a delivered month shows no catch-up
    /// absences rather than a wrong set.
    pub reported_as: Option<String>,
}

/// The first day whose working-time changes a *later* report may still carry.
///
/// A correction exists only after the report for its affected month has gone
/// out. Everything from `period_start` onwards belongs to the month being
/// reported right now, and everything in a month that is still queued will be
/// covered by that month's own report — carrying either would report the same
/// hours twice.
///
/// So the boundary is the earlier of the reported month's start and the start
/// of the oldest month still owed. `None` means no report has ever been
/// delivered, and nothing can be a late booking yet.
pub async fn carry_over_boundary(
    pool: &crate::db::DatabasePool,
    period_start: NaiveDate,
) -> AppResult<Option<CarriedDays>> {
    // Nothing ever reached the queue, so no month has been reported and no
    // entry can have missed its report.
    let queued_through =
        settings::load_setting(pool, settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY, "").await?;
    if queued_through.is_empty() {
        return Ok(None);
    }

    // Months whose own report is still to come. Their days must not be raided
    // — that report will print them itself — but they are named individually
    // rather than collapsed into "everything from the oldest one onwards":
    // the queue can have gaps. One month stuck behind a late approval, with
    // later months already delivered, must not freeze carry-over for those
    // later months too, possibly for as long as the stuck month lasts.
    let mut owed_periods = crate::repository::PayrollReportQueueDb::new(pool.clone())
        .list_pending()
        .await?;
    // A month can be owed without being queued yet: the queue is backfilled at
    // the start of a run, so between two runs the setting can trail several
    // months behind. Both a send and the dashboard card have to count those as
    // owed, exactly like `manual_send_target` does — otherwise the card offers
    // days as catch-ups that the older report about to be sent will print
    // itself.
    let today = settings::app_today(pool).await;
    for period in crate::background::schedule::periods_to_backfill(
        &queued_through,
        &crate::background::schedule::previous_period(today),
    ) {
        if !owed_periods.contains(&period) {
            owed_periods.push(period);
        }
    }

    Ok(Some(CarriedDays {
        since: carry_over_floor(pool).await?,
        // Everything from the reported month onwards belongs to its regular
        // sections, not to the catch-up one.
        before: period_start,
        owed_periods,
        reported_as: None,
    }))
}

/// The start of the very first period the payroll report ever queued — the
/// permanent lower bound nothing may ever be carried past. See
/// [`CarriedDays::since`] for why this exists.
///
/// Falls back to `before` (making the caller's range empty) when the floor is
/// somehow unset while this is asked for anyway — reachable only if
/// `PAYROLL_REPORT_QUEUE_PERIOD_KEY` is set without `PAYROLL_REPORT_FIRST_PERIOD_KEY`
/// (an installation upgraded mid-flight, before either setting existed);
/// refusing to carry anything is the safe direction to fail in.
async fn carry_over_floor(pool: &crate::db::DatabasePool) -> AppResult<NaiveDate> {
    let first_period =
        settings::load_setting(pool, settings::PAYROLL_REPORT_FIRST_PERIOD_KEY, "").await?;
    if first_period.is_empty() {
        // An installation that was already sending payroll reports before this
        // floor existed. There is nothing to protect it from: migration 044
        // marked every entry that existed when the carry-over feature arrived,
        // so no pre-existing history can be mistaken for a late booking. The
        // floor must therefore be permissive here — a restrictive fallback
        // would silently switch carry-over off for exactly the installations
        // that have been running longest.
        return Ok(NO_CARRY_OVER_FLOOR);
    }
    crate::background::schedule::period_bounds(&first_period).map(|(start, _)| start)
}

/// Lower bound that excludes nothing, for installations predating
/// [`settings::PAYROLL_REPORT_FIRST_PERIOD_KEY`]. Earlier than any date this
/// application can hold: a time entry cannot precede its owner's start date,
/// and no real employment record reaches back this far.
const NO_CARRY_OVER_FLOOR: NaiveDate = match NaiveDate::from_ymd_opt(1970, 1, 1) {
    Some(date) => date,
    None => unreachable!(),
};

/// The month an admin's "Send now" targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualSendTarget {
    /// Period to send, "YYYY-MM".
    pub period: String,
    /// True when `period` is the month currently running. Nobody's month can
    /// be final yet in that case, so the send is an interim snapshot of what
    /// is approved so far rather than a partial copy of a month that is owed.
    pub in_progress: bool,
}

/// Pick the month an admin's "Send now" should target.
///
/// The previous month is what is actually owed, so it keeps priority for as
/// long as its report has not gone out in full — in practice only the first
/// days of a month, before the configured send day. Once it is delivered there
/// is nothing left to send for it, and the button moves on to the month now
/// running, where it sends a snapshot of everything approved to date.
pub async fn manual_send_target(pool: &crate::db::DatabasePool) -> AppResult<ManualSendTarget> {
    let today = settings::app_today(pool).await;

    // A month is owed either because it is already queued, or because it has
    // not been queued yet but will be the moment a run starts — the send path
    // backfills the queue before it picks anything up. Both lists have to be
    // considered here, or the button would name one month and send another:
    // with the queue empty and the marker several months behind, the naive
    // answer is "the previous month" while the run would actually reach for
    // the oldest backfilled one.
    let queued = crate::repository::PayrollReportQueueDb::new(pool.clone())
        .list_pending()
        .await?;
    let pending_backfill = crate::background::schedule::periods_to_backfill(
        &settings::load_setting(pool, settings::PAYROLL_REPORT_QUEUE_PERIOD_KEY, "").await?,
        &crate::background::schedule::previous_period(today),
    );

    // "YYYY-MM" sorts chronologically, so the lexicographic minimum across
    // both lists is the oldest month still owed. It wins over the running
    // month: a month stuck behind a late submitter must not be skipped in
    // favour of a newer one, or the button could never push it out at all.
    let oldest_owed = queued.iter().chain(pending_backfill.iter()).min();
    if let Some(period) = oldest_owed {
        return Ok(ManualSendTarget {
            period: period.clone(),
            in_progress: false,
        });
    }

    // Nothing is owed any more, so the button moves on to the month now
    // running and sends its state so far.
    Ok(ManualSendTarget {
        period: crate::background::schedule::current_period(today),
        in_progress: true,
    })
}

/// Traffic-light status of one person's month, as shown on the dashboard tile.
pub mod status_value {
    /// Everything submitted and approved — this person is done.
    pub const READY: &str = "ready";
    /// Everything submitted, but an approval or absence decision is missing.
    pub const AWAITING_APPROVAL: &str = "awaiting_approval";
    /// Weeks are still missing, or the data needs an admin's attention.
    pub const NOT_SUBMITTED: &str = "not_submitted";
}

/// One row of the payroll status tile's detail list.
///
/// `user_id` and `name` are `None` for people the requesting team lead is not
/// allowed to see. Their status still counts towards the totals — a lead needs
/// to know whether the month is complete even when the outstanding person is
/// not on their team — but the identity never leaves the server.
#[derive(serde::Serialize)]
pub struct SubmissionStatusMember {
    pub user_id: Option<i64>,
    pub name: Option<String>,
    pub status: &'static str,
    pub reason_key: Option<&'static str>,
}

/// Everything the Submissions dashboard tile renders for the tracked month.
#[derive(serde::Serialize)]
pub struct SubmissionStatus {
    /// Tracked period, "YYYY-MM" — the previous month by default, or the
    /// current in-progress month when the tile's transient "show this month"
    /// peek was requested (see `build_submission_status`).
    pub period: String,
    /// Localized month name, e.g. "Juli 2026".
    pub period_label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub total: usize,
    pub ready: usize,
    pub awaiting_approval: usize,
    pub not_submitted: usize,
    pub members: Vec<SubmissionStatusMember>,
}

/// One line of what the payroll report holds for one person: either an absence
/// period or a person's working days and hours.
///
/// `name` is `None` for somebody a team lead may not see — the line still
/// counts towards the totals, because a lead has to be able to tell whether the
/// month looks complete, but the identity never leaves the server.
#[derive(serde::Serialize)]
pub struct PayrollContentRow {
    pub name: Option<String>,
    /// `absence`, `hours`, `late_hours` for a day carried over from an
    /// already-reported month, or `late_absence` for absence days no report
    /// ever showed.
    pub kind: &'static str,
    /// Localized absence category; `None` on an hours line.
    pub category: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// Absence days, or days worked on an hours line.
    pub days: f64,
    /// Minutes worked; `None` on an absence line.
    pub minutes: Option<i64>,
    pub medical_certificate_required: Option<bool>,
}

/// What the payroll report for the tracked month contains — or, while the month
/// is still running, what it is shaping up to contain.
#[derive(serde::Serialize)]
pub struct PayrollContent {
    /// False when the payroll report is switched off; the tile stays hidden.
    pub enabled: bool,
    pub period: String,
    pub period_label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// True once the scheduled delivery for this period has gone out.
    pub sent: bool,
    pub day_of_month: u8,
    /// The month is still running, so these figures are a snapshot of today.
    pub in_progress: bool,
    pub absence_count: usize,
    /// People with an hours line — assistants, unless employee hours are on.
    pub people_with_hours: usize,
    pub minutes: i64,
    pub rows: Vec<PayrollContentRow>,
}

/// Flatten a finished document into the exact rows its delivered dashboard
/// card needs. The ordering mirrors the PDF: regular absences, regular hours,
/// late absences, then working-time corrections.
pub fn reported_content_rows(data: &PayrollReportData) -> Vec<PayrollReportedContentRow> {
    let mut rows = Vec::new();
    for absence in data.absence_rows.iter().flatten() {
        rows.push(PayrollReportedContentRow {
            user_id: absence.user_id,
            employee: absence.employee.clone(),
            kind: "absence".to_string(),
            category: Some(absence.category.clone()),
            from_date: Some(absence.from),
            to_date: Some(absence.to),
            days: absence.days,
            minutes: None,
            medical_certificate_required: absence.medical_certificate_required,
        });
    }
    for section in &data.hours_sections {
        for hours in &section.rows {
            rows.push(PayrollReportedContentRow {
                user_id: hours.user_id,
                employee: hours.employee.clone(),
                kind: "hours".to_string(),
                category: None,
                from_date: None,
                to_date: None,
                days: hours.work_days as f64,
                minutes: Some(hours.minutes),
                medical_certificate_required: None,
            });
        }
    }
    for absence in &data.late_absence_rows {
        rows.push(PayrollReportedContentRow {
            user_id: absence.user_id,
            employee: absence.employee.clone(),
            kind: "late_absence".to_string(),
            category: Some(absence.category.clone()),
            from_date: Some(absence.from),
            to_date: Some(absence.to),
            days: absence.days,
            minutes: None,
            medical_certificate_required: absence.medical_certificate_required,
        });
    }
    for correction in &data.late_entry_rows {
        rows.push(PayrollReportedContentRow {
            user_id: correction.user_id,
            employee: correction.employee.clone(),
            kind: "late_hours".to_string(),
            category: None,
            from_date: Some(correction.date),
            to_date: Some(correction.date),
            days: 1.0,
            minutes: Some(correction.minutes),
            medical_certificate_required: None,
        });
    }
    rows
}

async fn visible_payroll_user_ids(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
) -> AppResult<Option<std::collections::HashSet<i64>>> {
    if requester.is_admin() {
        return Ok(None);
    }
    Ok(Some(
        app_state
            .db
            .reports
            .active_team_members(requester.id, false)
            .await?
            .into_iter()
            .map(|member| member.id)
            .collect(),
    ))
}

fn content_kind(kind: &str) -> AppResult<&'static str> {
    match kind {
        "absence" => Ok("absence"),
        "hours" => Ok("hours"),
        "late_absence" => Ok("late_absence"),
        "late_hours" => Ok("late_hours"),
        _ => Err(crate::error::AppError::Internal(format!(
            "Invalid stored payroll content kind: {kind}"
        ))),
    }
}

fn payroll_content_rows(
    stored_rows: Vec<PayrollReportedContentRow>,
    visible_user_ids: Option<&std::collections::HashSet<i64>>,
) -> AppResult<(Vec<PayrollContentRow>, usize, usize, i64)> {
    let mut rows = Vec::with_capacity(stored_rows.len());
    let mut absence_count = 0;
    let mut people_with_hours = std::collections::HashSet::new();
    let mut minutes = 0;

    for stored in stored_rows {
        let kind = content_kind(&stored.kind)?;
        if matches!(kind, "absence" | "late_absence") {
            absence_count += 1;
        }
        if stored.minutes.is_some() {
            people_with_hours.insert(stored.user_id);
            minutes += stored.minutes.unwrap_or(0);
        }
        let name = match visible_user_ids {
            None => Some(stored.employee),
            Some(ids) if ids.contains(&stored.user_id) => Some(stored.employee),
            Some(_) => None,
        };
        rows.push(PayrollContentRow {
            name,
            kind,
            category: stored.category,
            from: stored.from_date,
            to: stored.to_date,
            days: stored.days,
            minutes: stored.minutes,
            medical_certificate_required: stored.medical_certificate_required,
        });
    }

    Ok((rows, absence_count, people_with_hours.len(), minutes))
}

/// Build the payroll content tile.
///
/// Deliberately assembled by the very code that builds the document
/// ([`build_report_data`]), on the very member set the matching send mode would
/// use, so the tile cannot claim something the PDF would not print. For the
/// finished month that is the scheduled run's view; for the running month it is
/// the interim snapshot's — clamped to today, and dropping rows that only mean
/// something once a month is over.
pub async fn build_content(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    language: &Language,
    show_current_month: bool,
) -> AppResult<PayrollContent> {
    let config = load_config(&app_state.pool).await?;
    let today = settings::app_today(&app_state.pool).await;
    let period = if show_current_month {
        crate::background::schedule::current_period(today)
    } else {
        crate::background::schedule::previous_period(today)
    };
    let (from, month_end) = crate::background::schedule::period_bounds(&period)?;
    let period_label = crate::i18n::format_month(language, from.year(), from.month());
    let in_progress = show_current_month;
    let to = if in_progress {
        month_end.min(today)
    } else {
        month_end
    };

    if !config.enabled {
        return Ok(PayrollContent {
            enabled: false,
            period,
            period_label,
            from,
            to,
            sent: false,
            day_of_month: config.day_of_month,
            in_progress,
            absence_count: 0,
            people_with_hours: 0,
            minutes: 0,
            rows: Vec::new(),
        });
    }

    let sent = period_delivered(&app_state.pool, &period).await?;
    let visible_user_ids = visible_payroll_user_ids(app_state, requester).await?;
    if sent
        && app_state
            .db
            .reports
            .payroll_period_accounted(&period)
            .await?
    {
        let stored_rows = app_state
            .db
            .reports
            .payroll_reported_content(&period)
            .await?;
        let (rows, absence_count, people_with_hours, minutes) =
            payroll_content_rows(stored_rows, visible_user_ids.as_ref())?;
        return Ok(PayrollContent {
            enabled: true,
            period,
            period_label,
            from,
            to,
            sent: true,
            day_of_month: config.day_of_month,
            in_progress: false,
            absence_count,
            people_with_hours,
            minutes,
            rows,
        });
    }
    // One description of the carried days for the member set and the document
    // alike, so the tile cannot show a day the report would not print.
    //
    // For a month that has already gone out the question is what its report
    // *did* carry, not what a report assembled today would: a day booked since
    // the send belongs to the next report, and showing it here under "what this
    // month's report contained" would say the tax office already has it.
    let carried = if sent {
        // An exact match on the mark (`reported_as: Some(period)`) already
        // scopes this precisely, so `since` only has to be a value the mark
        // could actually have used — the feature's own floor always is.
        Some(CarriedDays {
            since: carry_over_floor(&app_state.pool).await?,
            before: from,
            // Reading history back: the mark alone says what went out, and a
            // month owed *now* has no bearing on what that send did months ago.
            owed_periods: Vec::new(),
            reported_as: Some(period.clone()),
        })
    } else {
        carry_over_boundary(&app_state.pool, from).await?
    };
    let members = payroll_members(
        app_state,
        from,
        to,
        &config.excluded_user_ids,
        in_progress,
        carried.as_ref(),
    )
    .await?;

    let data = build_report_data(
        app_state,
        ReportWindow {
            from,
            to,
            interim: in_progress,
            created_on: today,
            carried,
        },
        &members,
        &config,
        language,
        None,
    )
    .await?;
    let (rows, absence_count, people_with_hours, minutes) =
        payroll_content_rows(reported_content_rows(&data), visible_user_ids.as_ref())?;

    Ok(PayrollContent {
        enabled: true,
        period,
        period_label,
        from,
        to,
        sent,
        day_of_month: config.day_of_month,
        in_progress,
        absence_count,
        people_with_hours,
        minutes,
        rows,
    })
}

/// Build the Submissions status for the dashboard tile.
///
/// This tile answers "who has closed their month" and nothing else. It used to
/// double as the payroll report's readiness display, but the report no longer
/// waits on unhanded-in weeks (see `reports::month_export_readiness`), so the
/// two questions came apart: an outstanding week is worth chasing, and is not
/// a reason to hold a document. What the report will actually contain is a
/// separate tile, [`build_content`].
///
/// It still covers the people [`payroll_members`] tracks — everybody the month
/// concerns, minus administrators, minus anyone explicitly excluded, minus
/// assistants who booked nothing at all and therefore have nothing to close.
///
/// `show_current_month` switches the tracked period from the previous month to
/// the current, still-running one. It is the tile's transient "show this month"
/// peek: a client-side, non-persistent choice, not a stored setting.
pub async fn build_submission_status(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    language: &Language,
    show_current_month: bool,
) -> AppResult<SubmissionStatus> {
    let config = load_config(&app_state.pool).await?;
    let today = settings::app_today(&app_state.pool).await;
    let period = if show_current_month {
        crate::background::schedule::current_period(today)
    } else {
        crate::background::schedule::previous_period(today)
    };
    let (from, to) = crate::background::schedule::period_bounds(&period)?;

    let period_label = crate::i18n::format_month(language, from.year(), from.month());

    // The tile counts everybody the month covers, including people who have
    // not booked anything yet — showing who still owes something is the whole
    // point of the card, so it never uses the snapshot's narrower filter.
    // No carried days: this tile asks who has closed *this* month, and a late
    // booking from a month already reported cannot answer that.
    let members =
        payroll_members(app_state, from, to, &config.excluded_user_ids, false, None).await?;
    // The tile's colours always require full approval, for every person — see
    // the doc comment on `evaluate_members` — and, unlike the payroll gate, a
    // week nobody handed in is exactly what this tile is here to show.
    let evaluated = evaluate_members(
        app_state,
        &members,
        from,
        to,
        // Closing a month means nothing of it is left open, drafts included.
        |_role| crate::services::reports::UnapprovedEntries::AnyUnsettled,
        true,
        crate::services::reports::PendingAbsences::Any,
    )
    .await?;

    // Team leads only see the names of their own people; everybody else on the
    // tile is counted but anonymized.
    let visible_ids: Option<std::collections::HashSet<i64>> = if requester.is_admin() {
        None
    } else {
        Some(
            app_state
                .db
                .reports
                .active_team_members(requester.id, false)
                .await?
                .into_iter()
                .map(|member| member.id)
                .collect(),
        )
    };

    let mut status = SubmissionStatus {
        period,
        period_label,
        from,
        to,
        total: evaluated.len(),
        ready: 0,
        awaiting_approval: 0,
        not_submitted: 0,
        members: Vec::with_capacity(evaluated.len()),
    };
    for member in evaluated {
        let value = status_for_member(app_state, &member.user, member.readiness, from, to).await?;
        match value {
            status_value::READY => status.ready += 1,
            status_value::AWAITING_APPROVAL => status.awaiting_approval += 1,
            _ => status.not_submitted += 1,
        }
        let visible = visible_ids
            .as_ref()
            .is_none_or(|ids| ids.contains(&member.user.id));
        status.members.push(SubmissionStatusMember {
            user_id: visible.then_some(member.user.id),
            name: visible.then(|| format!("{} {}", member.user.first_name, member.user.last_name)),
            status: value,
            reason_key: member.reason_key,
        });
    }
    Ok(status)
}

/// Colour for the readiness values that decide themselves.
///
/// Two variants are deliberately missing, because neither can be coloured
/// without asking a second question, which [`status_for_member`] does:
///
/// * `PendingAbsenceRequests` — the shared gate reports it *before* it ever
///   looks at week submission, so on its own it cannot tell "handed everything
///   in, waiting for a decision" (amber) from "also still owes weeks" (red).
/// * `UnapprovedTimeEntries` — it covers drafts and submitted rows alike. A
///   draft is not waiting for anybody: nobody handed it in. Painting it amber
///   sent approvers looking for work that was never submitted to them.
fn unambiguous_status(readiness: MonthExportReadiness) -> Option<&'static str> {
    match readiness {
        MonthExportReadiness::Ready => Some(status_value::READY),
        MonthExportReadiness::WeeksNotSubmitted
        | MonthExportReadiness::UnresolvedTimeEntries
        | MonthExportReadiness::PreStartContent => Some(status_value::NOT_SUBMITTED),
        MonthExportReadiness::UnapprovedTimeEntries
        | MonthExportReadiness::PendingAbsenceRequests => None,
    }
}

/// Traffic-light colour for one person on the dashboard card.
///
/// Red always wins over amber: the card's whole purpose is to show who still
/// has to hand something in, so somebody who owes weeks must not be painted
/// amber merely because they *also* have an absence request awaiting a
/// decision.
async fn status_for_member(
    app_state: &AppState,
    user: &User,
    readiness: MonthExportReadiness,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<&'static str> {
    if let Some(status) = unambiguous_status(readiness) {
        return Ok(status);
    }
    if readiness == MonthExportReadiness::UnapprovedTimeEntries {
        // Drafts and rejected leftovers are the employee's move, not an
        // approver's: report them as not submitted. Only genuinely submitted
        // rows are "waiting for approval".
        let today = crate::services::settings::app_today(&app_state.pool).await;
        let judged_to = crate::services::reports::judged_period_end(to, today);
        let unsubmitted = app_state
            .db
            .reports
            .has_unsubmitted_time_entries_in_range(user.id, from, judged_to)
            .await?;
        return Ok(if unsubmitted {
            status_value::NOT_SUBMITTED
        } else {
            status_value::AWAITING_APPROVAL
        });
    }
    // Only `PendingAbsenceRequests` reaches here. Ask the week question the
    // gate skipped: an open request is "submitted, awaiting approval" (amber),
    // but missing weeks outrank it (red).
    let submission_exempt = !crate::roles::has_submission_obligation(&user.role, user.weekly_hours);
    let weeks_in = crate::services::reports::all_weeks_submitted_for_month(
        &app_state.pool,
        user.id,
        from,
        to,
        user.start_date,
        submission_exempt,
        user.workdays_per_week,
    )
    .await?;
    Ok(if weeks_in {
        status_value::AWAITING_APPROVAL
    } else {
        status_value::NOT_SUBMITTED
    })
}

/// One covered person plus why their month is not final yet. `reason_key` is
/// `None` when they are ready to be reported.
pub struct MemberReadiness {
    pub user: User,
    pub readiness: MonthExportReadiness,
    pub reason_key: Option<&'static str>,
}

/// Evaluate the month-finality gate for everyone the report covers.
///
/// `require_full_approval` decides, per person, whether an approved-but-open
/// month blocks them or not. The two callers need different answers to that
/// question, which is why it is a parameter instead of being derived from
/// [`PayrollReportConfig`] here:
///
/// * the **send path** only needs a person's entries approved when their
///   hours literally end up in the PDF (`config.includes_hours_for`) — if
///   their working time isn't printed, an unapproved entry doesn't make the
///   document wrong;
/// * the **dashboard tile** always requires full approval for everyone. Its
///   green/amber/red split ("submitted and approved" / "submitted, not yet
///   approved" / "not submitted") is a per-person status the admin asked for
///   unconditionally — whether this person's hours happen to be toggled into
///   the report is a business decision about document *content*, not about
///   whether they personally finished their month.
///
/// Reusing the send path's relaxed rule for the tile was the bug this
/// parameter fixes: with `payroll_report_include_employee_hours` off by
/// default, a regular employee's submitted-but-unapproved month used to read
/// as `Ready` — green — on the dashboard.
pub async fn evaluate_members(
    app_state: &AppState,
    members: &[User],
    from: NaiveDate,
    to: NaiveDate,
    unapproved: impl Fn(&str) -> crate::services::reports::UnapprovedEntries,
    require_week_submission: bool,
    pending_absences: crate::services::reports::PendingAbsences,
) -> AppResult<Vec<MemberReadiness>> {
    let mut evaluated = Vec::with_capacity(members.len());
    for member in members {
        let readiness = crate::services::reports::month_export_readiness(
            &app_state.pool,
            member,
            from,
            to,
            unapproved(&member.role),
            require_week_submission,
            pending_absences,
        )
        .await?;
        evaluated.push(MemberReadiness {
            user: member.clone(),
            readiness,
            reason_key: readiness_reason_key(readiness),
        });
    }
    Ok(evaluated)
}

/// Translation key describing why a month is not final, or `None` when it is.
fn readiness_reason_key(readiness: MonthExportReadiness) -> Option<&'static str> {
    match readiness {
        MonthExportReadiness::Ready => None,
        MonthExportReadiness::PreStartContent => Some("payroll_report_reason_pre_start_content"),
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
    }
}

/// Split the stored comma-separated user ID list. Blanks, duplicates and
/// non-numeric leftovers are dropped: a user who was hard-deleted after being
/// excluded would otherwise keep a dangling ID in the setting forever, and a
/// stale ID simply matches nobody.
pub fn parse_excluded_ids(stored: &str) -> Vec<i64> {
    let mut ids: Vec<i64> = Vec::new();
    for candidate in stored.split(',') {
        let Ok(id) = candidate.trim().parse::<i64>() else {
            continue;
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// Serialize an excluded-user list back into the stored comma-separated form,
/// normalized through the parser so the stored value is always duplicate-free.
pub fn format_excluded_ids(ids: &[i64]) -> String {
    parse_excluded_ids(
        &ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(","),
    )
    .iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>()
    .join(",")
}

/// The slice of time a report covers, and whether that slice is finished.
///
/// The three travel together everywhere: `to` is the month end for a finished
/// month but only today for an interim look, and `interim` decides which rows
/// carry meaning at that point.
#[derive(Debug, Clone)]
pub struct ReportWindow {
    pub from: NaiveDate,
    /// Last day covered. Today rather than the month end while `interim`.
    pub to: NaiveDate,
    /// The month is still running, so rows that only mean something once it is
    /// over — a salaried employee's "worked 0 days" — are left out.
    pub interim: bool,
    /// The day the document is being assembled, in the app's timezone. Printed
    /// on the report and used in its filename. Passed in rather than read here
    /// so every part of one document agrees on the date — for an interim look
    /// `to` is derived from this very day.
    pub created_on: NaiveDate,
    /// Days from earlier months this report carries; see [`CarriedDays`].
    /// `None` carries nothing.
    ///
    /// Passed in rather than looked up inside the builder so that the send path
    /// records exactly the days the document printed: it uses this one value
    /// for the member set, for the document, and for marking those days as
    /// reported afterwards.
    pub carried: Option<CarriedDays>,
}

/// Absence categories the payroll report includes automatically — sick-like
/// categories and anything that costs neither vacation nor flextime (see
/// [`AbsenceCategory::is_payroll_relevant`]). Order follows `list_all()`
/// (active first, then sort_order, then name), which is also the order rows
/// appear in the rendered PDF.
pub async fn payroll_relevant_categories(
    pool: &crate::db::DatabasePool,
) -> AppResult<Vec<AbsenceCategory>> {
    let categories = AbsenceCategoryDb::new(pool.clone()).list_all().await?;
    Ok(categories
        .into_iter()
        .filter(AbsenceCategory::is_payroll_relevant)
        .collect())
}

/// Split the stored comma-separated recipient list, dropping blanks and
/// duplicates (case-insensitively) while preserving the admin's order.
pub fn parse_recipient_list(stored: &str) -> Vec<String> {
    let mut recipients: Vec<String> = Vec::new();
    for address in stored.split(',') {
        let address = address.trim();
        if address.is_empty()
            || recipients
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(address))
        {
            continue;
        }
        recipients.push(address.to_string());
    }
    recipients
}

/// Serialize a recipient list back into the stored comma-separated form.
pub fn format_recipient_list(recipients: &[String]) -> String {
    parse_recipient_list(&recipients.join(",")).join(",")
}

/// Assemble everything the payroll report PDF renders for one period.
///
/// `members` are the people the report actually covers, already narrowed by
/// [`payroll_members`]. For a manual partial send this is only the subset whose
/// month is final; `provisional` then describes what is missing.
pub async fn build_report_data(
    app_state: &AppState,
    window: ReportWindow,
    members: &[User],
    config: &PayrollReportConfig,
    language: &Language,
    provisional: Option<crate::report_pdf::ProvisionalNotice>,
) -> AppResult<PayrollReportData> {
    let ReportWindow {
        from,
        to,
        interim,
        created_on,
        carried,
    } = window;
    let organization_name =
        settings::load_setting(&app_state.pool, settings::ORGANIZATION_NAME_KEY, "").await?;

    let relevant_categories = payroll_relevant_categories(&app_state.pool).await?;
    let (absence_rows, reported_absence_ids) = if relevant_categories.is_empty() {
        (None, Vec::new())
    } else {
        let built = build_absence_rows(
            app_state,
            from,
            to,
            members,
            &relevant_categories,
            language,
            carried.as_ref().and_then(|c| c.reported_as.as_deref()),
        )
        .await?;
        (Some(built.rows), built.ids)
    };

    // A delivered month reads its hours back from what its own send actually
    // marked (see `sent_hours_rows`) rather than recomputing live — otherwise
    // an entry approved afterwards for a date inside that month would inflate
    // this section as though it had been mailed, while it is in fact still
    // waiting for a future report to carry it, and would end up counted twice
    // on the dashboard.
    let (hours_sections, mut declared_work_days) =
        if let Some(period) = carried.as_ref().and_then(|c| c.reported_as.as_deref()) {
            (
                sent_hours_rows(app_state, period, from, to, members, config).await?,
                Vec::new(),
            )
        } else {
            let mut hours_sections = Vec::new();
            let mut declared_work_days = Vec::new();
            if config.include_assistant_hours {
                let built = build_hours_rows(app_state, from, to, members, true, true).await?;
                declared_work_days.extend(built.declared_work_days);
                hours_sections.push(PayrollHoursSection {
                    heading_key: ASSISTANT_HOURS_HEADING_KEY,
                    // Assistants are paid by the hour, so an empty month is never
                    // their row — in any mode.
                    rows: built.rows,
                });
            }
            if config.include_employee_hours {
                let built = build_hours_rows(app_state, from, to, members, false, interim).await?;
                declared_work_days.extend(built.declared_work_days);
                hours_sections.push(PayrollHoursSection {
                    heading_key: EMPLOYEE_HOURS_HEADING_KEY,
                    // Employees are salaried, so "worked no days" is real
                    // information for a finished month and their zero row stays. In an
                    // interim look at a month still running it means nothing yet —
                    // usually just an unapproved current week — so it is dropped, which
                    // also keeps `people_in_report` equal to what the PDF prints.
                    rows: built.rows,
                });
            }
            (hours_sections, declared_work_days)
        };

    // Days from earlier months whose own report has already gone out. They are
    // built for every mode, so the dashboard card shows exactly what the PDF
    // would print. An interim snapshot shows them too — it is explicitly a
    // preview, and the final report will carry the same rows.
    let late_entries = build_late_entry_rows(app_state, carried.as_ref(), members, config).await?;
    declared_work_days.extend(late_entries.declared_work_days);
    // Absence days no report has ever shown. Built whenever the absence
    // section itself exists — the catch-up is pointless if the document has
    // nowhere to print absences at all.
    let (late_absence_rows, late_absence_ids) = if relevant_categories.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        build_late_absence_rows(
            app_state,
            carried.as_ref(),
            members,
            &relevant_categories,
            language,
        )
        .await?
    };

    Ok(PayrollReportData {
        // `from` is the first day of the reported month, so it carries the
        // period the heading needs without passing the raw "YYYY-MM" string
        // (and its parsing) down into the service layer.
        period_label: crate::i18n::format_month(language, from.year(), from.month()),
        organization_name,
        absence_rows,
        hours_sections,
        late_entry_rows: late_entries.rows,
        declared_work_days,
        carried_work_days: late_entries.carried_work_days,
        reported_absence_ids,
        late_absence_rows,
        late_absence_ids,
        created_on,
        provisional,
    })
}

/// Working-day corrections for already-reported months.
///
/// One signed row per person and day. A newly booked day is positive; reducing
/// or moving a declared day can be negative. Payroll must book the correction
/// into the month where the work belongs, so it cannot be folded into the
/// reported month's totals.
///
/// Only people whose hours this report prints can have such a row — the
/// document never carried anybody else's hours, so there is nothing to catch
/// up on. Days before a person's start date are dropped here as everywhere
/// else in the app.
async fn build_late_entry_rows(
    app_state: &AppState,
    carried: Option<&CarriedDays>,
    members: &[User],
    config: &PayrollReportConfig,
) -> AppResult<LateEntryRows> {
    let Some(carried) = carried else {
        return Ok(LateEntryRows::default());
    };
    let printed: std::collections::HashMap<i64, &User> = members
        .iter()
        .filter(|member| config.includes_hours_for(&member.role))
        .map(|member| (member.id, member))
        .collect();
    if printed.is_empty() {
        return Ok(LateEntryRows::default());
    }

    let (deltas, records_declarations) = match carried.reported_as.as_deref() {
        Some(period) => {
            let declared = app_state
                .db
                .reports
                .declared_days_for_period(period)
                .await?;
            let mut deltas: Vec<LateEntryDelta> = declared
                .into_iter()
                .filter(|day| {
                    day.day >= carried.since
                        && day.day < carried.before
                        && !carried
                            .owed_periods
                            .iter()
                            .any(|owed| owed == &day.day.format("%Y-%m").to_string())
                })
                .map(|day| LateEntryDelta {
                    user_id: day.user_id,
                    date: day.day,
                    minutes: day.minutes,
                    ledger_backed: true,
                })
                .collect();

            // One post-migration report can contain both ledger-backed rows
            // and a correction for a pre-migration day. Preserve the latter
            // through its entry marker instead of treating the report as an
            // all-ledger or all-legacy document.
            let entries = app_state
                .db
                .reports
                .carried_time_entries_before(
                    Some(period),
                    carried.since,
                    carried.before,
                    &carried.owed_periods,
                )
                .await?;
            let legacy_by_day = net_minutes_by_day(app_state, entries, &printed).await?;
            let legacy_days: Vec<(i64, NaiveDate)> = legacy_by_day.keys().copied().collect();
            let known_ledger_days = app_state
                .db
                .reports
                .declared_minutes_for_days(&legacy_days)
                .await?;
            deltas.extend(
                legacy_by_day
                    .into_iter()
                    .filter(|(day, _)| !known_ledger_days.contains_key(day))
                    .map(|((user_id, date), minutes)| LateEntryDelta {
                        user_id,
                        date,
                        minutes,
                        ledger_backed: false,
                    }),
            );
            (deltas, false)
        }
        None => (late_entry_deltas(app_state, carried).await?, true),
    };

    let mut rendered: Vec<LateEntryDelta> = deltas
        .into_iter()
        .filter(|delta| delta.minutes != 0 && printed.contains_key(&delta.user_id))
        .collect();
    rendered.sort_by(|left, right| {
        employee_name(printed[&left.user_id])
            .cmp(&employee_name(printed[&right.user_id]))
            .then_with(|| left.date.cmp(&right.date))
    });
    let rows = rendered
        .iter()
        .map(|delta| PayrollLateEntryRow {
            user_id: delta.user_id,
            employee: employee_name(printed[&delta.user_id]),
            date: delta.date,
            minutes: delta.minutes,
        })
        .collect();
    let declared_work_days = if records_declarations {
        rendered
            .iter()
            .filter(|delta| delta.ledger_backed)
            .map(|delta| PayrollDeclaredWorkDay {
                user_id: delta.user_id,
                date: delta.date,
                minutes: delta.minutes,
            })
            .collect()
    } else {
        Vec::new()
    };
    let carried_work_days = if records_declarations {
        rendered
            .into_iter()
            .map(|delta| PayrollCarriedWorkDay {
                user_id: delta.user_id,
                date: delta.date,
            })
            .collect()
    } else {
        Vec::new()
    };
    // Person first, then chronological, so one person's corrections stay
    // together in the order the reader applies them.
    Ok(LateEntryRows {
        rows,
        declared_work_days,
        carried_work_days,
    })
}

/// Signed workday differences a report produced now would still have to
/// declare, before role/settings filtering decides whether this document
/// prints the person's hours.
async fn late_entry_deltas(
    app_state: &AppState,
    carried: &CarriedDays,
) -> AppResult<Vec<LateEntryDelta>> {
    let entries = app_state
        .db
        .reports
        .carried_day_entries_before(carried.since, carried.before, &carried.owed_periods)
        .await?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let mut days: std::collections::HashSet<(i64, NaiveDate)> = std::collections::HashSet::new();
    let mut marked_work_days: std::collections::HashSet<(i64, NaiveDate)> =
        std::collections::HashSet::new();
    let mut current_entries = Vec::new();
    let mut marked_entries = Vec::new();
    for entry in entries {
        days.insert((entry.user_id, entry.day));
        let (Some(start_time), Some(end_time)) = (entry.start_time, entry.end_time) else {
            continue;
        };
        if !entry.counts_as_work {
            continue;
        }
        let row = (entry.user_id, entry.day, start_time, end_time);
        current_entries.push(row.clone());
        if entry.already_reported {
            marked_work_days.insert((entry.user_id, entry.day));
            marked_entries.push(row);
        }
    }

    let auto_break = crate::services::reports::load_auto_break_config(&app_state.pool).await?;
    let current_minutes = net_minutes_by_day_with_rules(current_entries, auto_break.as_deref())?;
    // The legacy fallback's own comparison point: the marked entries' current
    // minutes, recomputed exactly as `current_minutes` is — same rules, same
    // grouping — but over only the rows already accounted for. The marked
    // rows were never deleted, only marked, so this is live, queryable data,
    // not an invented baseline. Subtracting it from the whole day's current
    // total (below) folds the marked hours into the *same* break computation
    // as the new ones, instead of pricing the new shift as if the already-paid
    // hours earlier in the day did not exist — which silently under-deducts
    // the break whenever the combined day crosses a threshold the new shift
    // alone does not.
    let legacy_marked_minutes =
        net_minutes_by_day_with_rules(marked_entries, auto_break.as_deref())?;
    let mut ordered_days: Vec<(i64, NaiveDate)> = days.into_iter().collect();
    ordered_days.sort_unstable();
    let declared_minutes = app_state
        .db
        .reports
        .declared_minutes_for_days(&ordered_days)
        .await?;
    let declared_periods = app_state
        .db
        .reports
        .declared_periods_for_days(&ordered_days)
        .await?;

    Ok(ordered_days
        .into_iter()
        .map(|(user_id, date)| {
            let (minutes, ledger_backed) = match declared_minutes.get(&(user_id, date)) {
                Some(declared) => (
                    current_minutes.get(&(user_id, date)).copied().unwrap_or(0) - declared,
                    true,
                ),
                // No ledger row means the day predates migration 047. Preserve
                // the established marker behavior instead of inventing a past
                // declared total that cannot be reconstructed.
                None if declared_periods.contains(&date.format("%Y-%m").to_string())
                    && !marked_work_days.contains(&(user_id, date)) =>
                {
                    (
                        current_minutes.get(&(user_id, date)).copied().unwrap_or(0),
                        true,
                    )
                }
                None => (
                    current_minutes.get(&(user_id, date)).copied().unwrap_or(0)
                        - legacy_marked_minutes
                            .get(&(user_id, date))
                            .copied()
                            .unwrap_or(0),
                    false,
                ),
            };
            LateEntryDelta {
                user_id,
                date,
                minutes,
                ledger_backed,
            }
        })
        .collect())
}

/// Net worked minutes per person and day, from a fixed list of entries rather
/// than a live status query — the auto-break deduction shared by the catch-up
/// section and a delivered month's own hours (see [`sent_hours_rows`]).
///
/// `eligible` restricts which user IDs are worth grouping at all; an entry for
/// anyone else is dropped before it is even bucketed by day. Days before a
/// person's start date are dropped too, belt and braces — the queries that
/// feed this already exclude them, and must, or the marking and the reading
/// would disagree.
async fn net_minutes_by_day(
    app_state: &AppState,
    entries: Vec<(i64, NaiveDate, String, String)>,
    eligible: &std::collections::HashMap<i64, &User>,
) -> AppResult<std::collections::HashMap<(i64, NaiveDate), i64>> {
    let entries = entries.into_iter().filter(|(user_id, entry_date, _, _)| {
        eligible
            .get(user_id)
            .is_some_and(|member| *entry_date >= member.start_date)
    });
    let auto_break = crate::services::reports::load_auto_break_config(&app_state.pool).await?;
    net_minutes_by_day_with_rules(entries, auto_break.as_deref())
}

fn net_minutes_by_day_with_rules(
    entries: impl IntoIterator<Item = (i64, NaiveDate, String, String)>,
    auto_break: Option<&[(i64, i64)]>,
) -> AppResult<std::collections::HashMap<(i64, NaiveDate), i64>> {
    let mut times_by_day: std::collections::HashMap<(i64, NaiveDate), Vec<(NaiveTime, NaiveTime)>> =
        std::collections::HashMap::new();
    for (user_id, entry_date, start_time, end_time) in entries {
        times_by_day
            .entry((user_id, entry_date))
            .or_default()
            .push((
                crate::services::reports::parse_report_time(&start_time)?,
                crate::services::reports::parse_report_time(&end_time)?,
            ));
    }

    let mut net = std::collections::HashMap::new();
    for (key, times) in times_by_day {
        let raw_minutes: i64 = times
            .iter()
            .map(|(start, end)| (*end - *start).num_minutes())
            .sum();
        let deduction = auto_break
            .map(|rules| crate::time_calc::compute_day_auto_break(&times, rules))
            .unwrap_or(0);
        net.insert(key, (raw_minutes - deduction).max(0));
    }
    Ok(net)
}

/// The hours sections of an already-delivered month, read back from what its
/// own send declared rather than recomputed live.
///
/// `build_hours_rows` (the general path) always reflects the current, live
/// state of the entries in range — correct for a month that has not gone out
/// yet, since that is exactly what a real send right now would produce. For a
/// month that has already gone out, "live" is the wrong answer: a new entry
/// approved afterwards for a date inside that month would inflate this
/// section as though it had been mailed, while it is in fact still waiting to
/// be carried into a *future* report — showing it here as well would count it
/// twice on the dashboard, once under a month it never reached.
///
/// Migration 047 records the exact net person-day totals on the document.
/// Reports sent before it have no such rows and retain the entry-marker
/// readback, preserving their established behavior without inventing history.
async fn sent_hours_rows(
    app_state: &AppState,
    period: &str,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    config: &PayrollReportConfig,
) -> AppResult<Vec<PayrollHoursSection>> {
    let printed: std::collections::HashMap<i64, &User> = members
        .iter()
        .filter(|member| config.includes_hours_for(&member.role))
        .map(|member| (member.id, member))
        .collect();
    if printed.is_empty() {
        return Ok(Vec::new());
    }

    let declared = app_state
        .db
        .reports
        .declared_days_for_period(period)
        .await?;
    let net_by_day = if declared.is_empty() {
        let entries = app_state
            .db
            .reports
            .time_entries_reported_in_range(period, from, to)
            .await?;
        net_minutes_by_day(app_state, entries, &printed).await?
    } else {
        declared
            .into_iter()
            .filter(|day| day.day >= from && day.day <= to && printed.contains_key(&day.user_id))
            .map(|day| ((day.user_id, day.day), day.minutes))
            .collect()
    };

    let mut totals: std::collections::HashMap<i64, (i64, i64)> = std::collections::HashMap::new();
    for ((user_id, _date), minutes) in net_by_day {
        if minutes <= 0 {
            continue;
        }
        let entry = totals.entry(user_id).or_default();
        entry.0 += 1;
        entry.1 += minutes;
    }

    let mut sections = Vec::new();
    for (heading_key, assistants) in [
        (ASSISTANT_HOURS_HEADING_KEY, true),
        (EMPLOYEE_HOURS_HEADING_KEY, false),
    ] {
        let include = if assistants {
            config.include_assistant_hours
        } else {
            config.include_employee_hours
        };
        if !include {
            continue;
        }
        // A sent month is by definition finished, never a running-month
        // snapshot, so this mirrors `build_hours_rows`'s own rule exactly:
        // an assistant's empty month is never a row, an employee's zero row
        // is real information once the month is over and stays.
        let drop_zero_rows = assistants;
        let mut rows: Vec<PayrollHoursRow> = printed
            .iter()
            .filter(|(_, member)| is_assistant_role(&member.role) == assistants)
            .filter_map(|(id, member)| {
                let (work_days, minutes) = totals.get(id).copied().unwrap_or((0, 0));
                if drop_zero_rows && work_days == 0 && minutes == 0 {
                    return None;
                }
                Some(PayrollHoursRow {
                    user_id: *id,
                    employee: employee_name(member),
                    work_days,
                    minutes,
                })
            })
            .collect();
        rows.sort_by(|left, right| left.employee.cmp(&right.employee));
        sections.push(PayrollHoursSection { heading_key, rows });
    }
    Ok(sections)
}

/// The first day of the month after `date`.
fn next_month_start(date: NaiveDate) -> Option<NaiveDate> {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1)
}

/// The stretches of `[from, to]` a report may declare as catch-up days, in
/// chronological order.
///
/// Days in a month whose own report is still queued are cut out: that report
/// prints them itself through its ordinary, month-clamped path, so declaring
/// them here as well would report the same days twice.
///
/// What is left can be more than one stretch, and treating it as one — or
/// dropping the whole absence because part of it is owed — is a silent loss of
/// days. An absence running from a month that has been reported into one that
/// is still owed is the everyday case: skipping it entirely leaves its earlier
/// half declared by nobody, because the owed month's report marks the absence
/// as declared the moment it prints its own half, and the catch-up path never
/// looks at it again.
fn reportable_segments(
    from: NaiveDate,
    to: NaiveDate,
    owed_periods: &[String],
) -> Vec<(NaiveDate, NaiveDate)> {
    let mut segments: Vec<(NaiveDate, NaiveDate)> = Vec::new();
    let mut month_start = match NaiveDate::from_ymd_opt(from.year(), from.month(), 1) {
        Some(start) => start,
        None => return segments,
    };
    while month_start <= to {
        let Some(next_month) = next_month_start(month_start) else {
            break;
        };
        let slice_from = month_start.max(from);
        let slice_to = next_month.pred_opt().unwrap_or(next_month).min(to);
        let period = format!("{:04}-{:02}", month_start.year(), month_start.month());
        if slice_from <= slice_to && !owed_periods.iter().any(|owed| owed == &period) {
            match segments.last_mut() {
                // Grow the run while consecutive months stay declarable, so an
                // absence nothing interrupts still prints as one row.
                Some(last) if last.1.succ_opt() == Some(slice_from) => last.1 = slice_to,
                _ => segments.push((slice_from, slice_to)),
            }
        }
        month_start = next_month;
    }
    segments
}

/// Payroll-relevant absence days from earlier months that no report has ever
/// shown, printed under their own real dates.
///
/// The absence twin of [`build_late_entry_rows`], and it exists for a sharper
/// reason. `AbsenceCategory::is_payroll_relevant` is `auto_approve_past OR
/// unpaid`, so a sick-like absence filed for *past* dates is approved
/// immediately: it never sits in `requested`, never trips
/// `PendingAbsences::PayrollRelevant`, and so cannot hold a report back. It
/// simply turns up after the month it belongs to has been filed. Without this
/// those days reach no document at all, and continued pay is never claimed.
///
/// A row carries the real dates payroll books against, not a clamp to the
/// reported month — but it covers only days no other report will show: nothing
/// from the reported month onwards, and nothing from a month that still owes
/// its own report (see [`reportable_segments`]). Days before the person's start
/// date are dropped here as everywhere else.
///
/// Assistants are skipped exactly as in [`build_absence_rows`]: they are paid
/// by the hour, so continued pay does not apply to them.
///
/// Returns the rows and the ids of the absences they came from. The sender
/// marks exactly those ids, so what was declared and what is recorded as
/// declared cannot come apart.
async fn build_late_absence_rows(
    app_state: &AppState,
    carried: Option<&CarriedDays>,
    members: &[User],
    relevant_categories: &[AbsenceCategory],
    language: &Language,
) -> AppResult<(Vec<PayrollAbsenceRow>, Vec<i64>)> {
    let empty = || (Vec::new(), Vec::new());
    let Some(carried) = carried else {
        return Ok(empty());
    };
    // Reading a delivered month back from its marker is the entries' trick and
    // does not transfer: an absence carries the *first* period that showed any
    // part of it, so it cannot answer "what did period P carry". A delivered
    // month therefore shows no catch-up absences rather than a wrong set.
    if carried.reported_as.is_some() {
        return Ok(empty());
    }
    // Only people whose absences the document prints — never an assistant.
    let printed: std::collections::HashMap<i64, &User> = members
        .iter()
        .filter(|member| !is_assistant_role(&member.role))
        .map(|member| (member.id, member))
        .collect();
    if printed.is_empty() {
        return Ok(empty());
    }

    let absences = app_state
        .db
        .reports
        .unreported_payroll_absences_before(carried.since, carried.before)
        .await?;
    if absences.is_empty() {
        return Ok(empty());
    }

    let selected: Vec<(String, String, usize, bool)> = relevant_categories
        .iter()
        .enumerate()
        .map(|(index, category)| {
            (
                category.slug.clone(),
                category.name.clone(),
                index,
                category.medical_certificate_relevant,
            )
        })
        .collect();
    let any_medical_certificate_category = relevant_categories
        .iter()
        .any(|c| c.medical_certificate_relevant);

    // Workday counting needs the holidays of the span the rows actually cover,
    // which is earlier than the reported month.
    let earliest = absences
        .iter()
        .map(|(_, _, start, _, _, _)| *start)
        .min()
        .unwrap_or(carried.since);
    let latest = absences
        .iter()
        .map(|(_, _, _, end, _, _)| *end)
        .max()
        .unwrap_or(carried.before);
    let holidays = app_state.db.reports.holiday_set(earliest, latest).await?;

    // (category rank, display name, user id, row) — the shape the shared
    // illness merge below works on.
    let mut rows: Vec<(usize, String, i64, PayrollAbsenceRow)> = Vec::new();
    // The absences this document actually declares something for. They, and
    // only they, are marked afterwards, so the marked set cannot drift from
    // the printed one.
    let mut declared_ids: Vec<i64> = Vec::new();
    for (user_id, absence_id, start_date, end_date, slug, _stored_name) in absences {
        let Some(member) = printed.get(&user_id) else {
            continue;
        };
        let Some((_, category_name, category_rank, tracks_medical_certificate)) = selected
            .iter()
            .find(|(selected_slug, _, _, _)| selected_slug == &slug)
        else {
            continue;
        };
        // Only the part that predates this report. An absence spanning into the
        // reported month has its remaining days printed by the ordinary clamped
        // path, so carrying the whole range here would declare those days
        // twice; carrying nothing would lose the earlier ones for good, because
        // the ordinary path marks the absence as reported either way.
        let row_from = start_date.max(member.start_date).max(carried.since);
        let row_to = end_date.min(carried.before.pred_opt().unwrap_or(carried.before));
        if row_from > row_to {
            continue;
        }
        // Months whose own report is still to come print their own days, so
        // they are cut out — but only they. See [`reportable_segments`].
        let segments = reportable_segments(row_from, row_to, &carried.owed_periods);
        if segments.is_empty() {
            continue;
        }
        let medical_certificate_required =
            if any_medical_certificate_category && *tracks_medical_certificate {
                crate::services::medical_certificate::required_map_for_user(app_state, member.id)
                    .await?
                    .get(&absence_id)
                    .copied()
                    .unwrap_or(false)
            } else {
                false
            };
        let mut declared_anything = false;
        for (segment_from, segment_to) in segments {
            let days = count_workdays(
                segment_from,
                segment_to,
                &holidays,
                member.workdays_per_week,
            );
            // A stretch covering only weekends or holidays has no payroll
            // effect — leave it out instead of printing a 0 row.
            if days <= 0.0 {
                continue;
            }
            declared_anything = true;
            rows.push((
                *category_rank,
                employee_name(member),
                member.id,
                PayrollAbsenceRow {
                    user_id: member.id,
                    employee: employee_name(member),
                    category: i18n::absence_kind_label(language, &slug, category_name),
                    from: segment_from,
                    to: segment_to,
                    days,
                    medical_certificate_required: tracks_medical_certificate
                        .then_some(medical_certificate_required),
                },
            ));
        }
        if declared_anything {
            declared_ids.push(absence_id);
        }
    }
    // Category, then person, then chronological — the order the main absence
    // table uses, so the reader is not asked to learn a second one, and the
    // order the illness merge below needs to look only at neighbours.
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.from.cmp(&right.3.from))
    });
    // One illness filed as two requests over a weekend is one period, and the
    // certificate verdict was earned on that whole period. The main table folds
    // such rows together for exactly that reason; a catch-up row printed
    // unmerged would carry the same misleading "certificate required" on a span
    // too short to have earned it. Segments of one absence separated by a month
    // that still owes its own report are never adjacent, so they cannot be
    // folded back together here.
    Ok((merge_continuous_illness_rows(rows, &holidays), declared_ids))
}

/// One row per absence period of a payroll-relevant category, clamped to the
/// reported month and to the employee's start date.
async fn build_absence_rows(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    relevant_categories: &[AbsenceCategory],
    language: &Language,
    reported_as: Option<&str>,
) -> AppResult<AbsenceRows> {
    // Category order in the PDF follows `list_all()`'s order, so all sick days
    // stay together, then all unpaid days, and so on.
    let selected: Vec<(String, String, usize, bool)> = relevant_categories
        .iter()
        .enumerate()
        .map(|(index, category)| {
            (
                category.slug.clone(),
                category.name.clone(),
                index,
                category.medical_certificate_relevant,
            )
        })
        .collect();
    // Skip the per-member AU chain lookup entirely when no selected category
    // tracks it — the common case for orgs that haven't opted into the flag.
    let any_medical_certificate_category = relevant_categories
        .iter()
        .any(|c| c.medical_certificate_relevant);

    let holidays = app_state.db.reports.holiday_set(from, to).await?;

    // (category rank, display name, user id, row) — the id is carried so
    // merging can group by person rather than by a name two people may share.
    let mut rows: Vec<(usize, String, i64, PayrollAbsenceRow)> = Vec::new();
    let mut ids = Vec::new();
    for member in members {
        // Assistants are paid by the hour: continued pay does not apply to
        // them, so their absences are none of payroll's business. Only their
        // worked hours are, and those are a different table.
        if is_assistant_role(&member.role) {
            continue;
        }
        let medical_certificate_required = if any_medical_certificate_category {
            crate::services::medical_certificate::required_map_for_user(app_state, member.id)
                .await?
        } else {
            std::collections::HashMap::new()
        };
        let absences = app_state
            .db
            .reports
            .approved_absence_rows_as_reported(member.id, from, to, reported_as)
            .await?;
        for (absence_id, start_date, end_date, slug, _stored_name) in absences {
            let Some((_, category_name, category_rank, tracks_medical_certificate)) = selected
                .iter()
                .find(|(selected_slug, _, _, _)| selected_slug == &slug)
            else {
                continue;
            };
            // Clamp to the reported month and to the employee's start date:
            // days before the contract started are not payroll-relevant and are
            // hidden everywhere else in the app too.
            let row_from = start_date.max(from).max(member.start_date);
            let row_to = end_date.min(to);
            if row_from > row_to {
                continue;
            }
            let days = count_workdays(row_from, row_to, &holidays, member.workdays_per_week);
            // An absence that only covers non-working days (weekend, holiday)
            // has no payroll effect — leave it out instead of printing a 0 row.
            if days <= 0.0 {
                continue;
            }
            ids.push(absence_id);
            rows.push((
                *category_rank,
                employee_name(member),
                member.id,
                PayrollAbsenceRow {
                    user_id: member.id,
                    employee: employee_name(member),
                    category: i18n::absence_kind_label(language, &slug, category_name),
                    from: row_from,
                    to: row_to,
                    days,
                    medical_certificate_required: tracks_medical_certificate.then(|| {
                        medical_certificate_required
                            .get(&absence_id)
                            .copied()
                            .unwrap_or(false)
                    }),
                },
            ));
        }
    }

    // Category, then employee, then chronological within one employee. The
    // chronological part is what lets the merge below look only at neighbours.
    // Sorting by id before date keeps one person's rows contiguous even when
    // somebody else shares their name, which is what lets the merge below look
    // only at its immediate neighbour.
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.from.cmp(&right.3.from))
    });
    ids.sort_unstable();
    ids.dedup();
    Ok(AbsenceRows {
        rows: merge_continuous_illness_rows(rows, &holidays),
        ids,
    })
}

/// Fold absences that are one continuous illness period into a single row.
///
/// A certificate is required for the *illness*, not for each request it was
/// filed in, so two sick notes with only a weekend between them produce one
/// verdict. Printed as separate rows that verdict looks wrong — a two-day row
/// marked "required" under a four-day threshold — because the span the reader
/// sees is not the span it was judged on. Merging makes the row the period.
///
/// Only categories that track certificates are merged, using the very rule
/// that built the verdict (`medical_certificate::bridges`), so the two can
/// never disagree. Days are summed rather than recomputed: the days bridged
/// over are by definition weekends or holidays, which never counted anyway.
///
/// Rows merge only within one category, because a single row cannot honestly
/// carry two category names. The verdict, however, is chain-wide across every
/// certificate-tracking category. So an installation that flags a second such
/// category (say "sick child") can still show a short row of one category
/// carrying a verdict earned next to the other — the original confusion, in
/// its last remaining corner. Merging across categories would trade a
/// confusing row for a wrong one, so it is left alone.
fn merge_continuous_illness_rows(
    rows: Vec<(usize, String, i64, PayrollAbsenceRow)>,
    holidays: &std::collections::HashSet<NaiveDate>,
) -> Vec<PayrollAbsenceRow> {
    let mut merged: Vec<(usize, String, i64, PayrollAbsenceRow)> = Vec::with_capacity(rows.len());
    for (rank, employee_key, user_id, row) in rows {
        let continues_previous = merged
            .last()
            .is_some_and(|(last_rank, _, last_user_id, last)| {
                *last_rank == rank
                    // By id, not by the display name that happens to sit
                    // beside it. A unique index on (first name, last name)
                    // means two people cannot share one today, so this is not
                    // fixing a live bug — it just refuses to depend on a
                    // guarantee made three modules away for its correctness.
                    && *last_user_id == user_id
                    // `Some` marks a category that tracks certificates; a
                    // category that does not has no notion of a continuous
                    // period.
                    && last.medical_certificate_required.is_some()
                    && row.medical_certificate_required.is_some()
                    && crate::services::medical_certificate::bridges(last.to, row.from, holidays)
            });
        if continues_previous {
            let (_, _, _, last) = merged.last_mut().expect("checked above");
            last.to = last.to.max(row.to);
            last.days += row.days;
            // Both belong to one chain, so the verdict is the same on each;
            // OR-ing is simply the total-safe way to combine them.
            last.medical_certificate_required = Some(
                last.medical_certificate_required.unwrap_or(false)
                    || row.medical_certificate_required.unwrap_or(false),
            );
        } else {
            merged.push((rank, employee_key, user_id, row));
        }
    }
    merged.into_iter().map(|(_, _, _, row)| row).collect()
}

/// Working days and worked minutes per person of one group (assistants or
/// everyone else). Uses the same month report the timesheet PDF is built from,
/// so the numbers match the archived timesheets exactly.
async fn build_hours_rows(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    assistants: bool,
    drop_zero_rows: bool,
) -> AppResult<HoursRows> {
    let mut rows = Vec::new();
    let mut declared_work_days = Vec::new();
    for member in members {
        if is_assistant_role(&member.role) != assistants {
            continue;
        }
        let auth_user = crate::services::users::repo_user_to_auth_user(member.clone());
        let report = crate::services::reports::build_range_with_user(
            &app_state.pool,
            &auth_user,
            from,
            to,
            "",
        )
        .await?;
        let work_days = report.days.iter().filter(|day| day.actual_min > 0).count() as i64;
        let minutes = report.actual_min;
        declared_work_days.extend(
            report
                .days
                .iter()
                .filter(|day| {
                    day.entries
                        .iter()
                        .any(|entry| entry.status == "approved" && entry.counts_as_work)
                })
                .map(|day| PayrollDeclaredWorkDay {
                    user_id: member.id,
                    date: day.date,
                    minutes: day.actual_min,
                }),
        );
        // A row stating zero days and zero hours only says something when the
        // month it covers is over; see the call sites for who drops it.
        if drop_zero_rows && work_days == 0 && minutes == 0 {
            continue;
        }
        rows.push(PayrollHoursRow {
            user_id: member.id,
            employee: employee_name(member),
            work_days,
            minutes,
        });
    }
    // `members` already arrives ordered by last name; keep that order explicit
    // so the payroll list is alphabetical by surname on every run.
    rows.sort_by(|left, right| left.employee.cmp(&right.employee));
    declared_work_days.sort_by_key(|day| (day.user_id, day.date));
    Ok(HoursRows {
        rows,
        declared_work_days,
    })
}

/// Surname-first name, the ordering convention payroll lists use.
fn employee_name(user: &User) -> String {
    format!("{}, {}", user.last_name, user.first_name)
}

/// How many distinct people the assembled report names.
///
/// Deliberately read back off the finished document rather than taken from the
/// member list: a person can be covered by the report and still not appear in
/// it, because only approved entries and approved absences produce rows.
/// Callers use it both to describe the report honestly and to recognise a
/// document that would go out with nothing in it — which is only sound because
/// every row the document does contain is meaningful for the mode that built
/// it (see `build_hours_rows`'s `drop_zero_rows`).
///
/// Counts distinct *names*, which is safe because a unique index on (first
/// name, last name) means two people cannot share one. Were that ever relaxed
/// this would undercount the notice — but it still could not make an empty
/// report look non-empty, which is the property the guard relies on.
pub fn people_in_report(data: &PayrollReportData) -> usize {
    let mut names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    if let Some(rows) = &data.absence_rows {
        names.extend(rows.iter().map(|row| row.employee.as_str()));
    }
    for section in &data.hours_sections {
        names.extend(section.rows.iter().map(|row| row.employee.as_str()));
    }
    // A month whose only content is somebody's catch-up day is still worth
    // sending — without this it would count as empty and be settled unsent.
    names.extend(data.late_entry_rows.iter().map(|row| row.employee.as_str()));
    names.extend(
        data.late_absence_rows
            .iter()
            .map(|row| row.employee.as_str()),
    );
    names.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    /// With every month already reported, the whole stretch is one row — the
    /// reader should not be handed an absence chopped up at month boundaries
    /// for no reason.
    #[test]
    fn reportable_segments_keeps_an_uninterrupted_absence_whole() {
        assert_eq!(
            reportable_segments(day(2029, 10, 29), day(2029, 12, 3), &[]),
            vec![(day(2029, 10, 29), day(2029, 12, 3))]
        );
        // A single day is still a stretch.
        assert_eq!(
            reportable_segments(day(2029, 10, 29), day(2029, 10, 29), &[]),
            vec![(day(2029, 10, 29), day(2029, 10, 29))]
        );
    }

    /// The case that loses days if it is got wrong: an absence running from a
    /// month whose report has gone out into one that is still owed. Only the
    /// reported month's days may be declared here — the owed month prints its
    /// own — but dropping the absence outright would lose the earlier days for
    /// good, because the owed month's report marks it as declared when it
    /// prints its half.
    #[test]
    fn reportable_segments_cuts_out_a_month_that_still_owes_its_report() {
        assert_eq!(
            reportable_segments(
                day(2029, 10, 29),
                day(2029, 11, 5),
                &["2029-11".to_string()]
            ),
            vec![(day(2029, 10, 29), day(2029, 10, 31))]
        );
        // And the other way round: the owed month comes first.
        assert_eq!(
            reportable_segments(
                day(2029, 10, 29),
                day(2029, 11, 5),
                &["2029-10".to_string()]
            ),
            vec![(day(2029, 11, 1), day(2029, 11, 5))]
        );
    }

    /// An owed month in the middle leaves two separate stretches. Treating them
    /// as one would declare the owed month's days twice.
    #[test]
    fn reportable_segments_splits_around_an_owed_month_in_the_middle() {
        assert_eq!(
            reportable_segments(
                day(2029, 10, 29),
                day(2029, 12, 3),
                &["2029-11".to_string()]
            ),
            vec![
                (day(2029, 10, 29), day(2029, 10, 31)),
                (day(2029, 12, 1), day(2029, 12, 3)),
            ]
        );
    }

    /// Every month owed means this report declares nothing, and the absence is
    /// left unmarked so those months' own reports still carry it.
    #[test]
    fn reportable_segments_is_empty_when_every_month_is_owed() {
        assert!(reportable_segments(
            day(2029, 11, 2),
            day(2029, 12, 3),
            &["2029-11".to_string(), "2029-12".to_string()]
        )
        .is_empty());
    }

    /// A December-to-January absence has to roll the year over correctly, or
    /// the loop would ask whether "2029-13" is owed and never terminate.
    #[test]
    fn reportable_segments_crosses_the_turn_of_the_year() {
        assert_eq!(
            reportable_segments(day(2029, 12, 28), day(2030, 1, 3), &[]),
            vec![(day(2029, 12, 28), day(2030, 1, 3))]
        );
        assert_eq!(
            reportable_segments(
                day(2029, 12, 28),
                day(2030, 1, 3),
                &["2030-01".to_string()]
            ),
            vec![(day(2029, 12, 28), day(2029, 12, 31))]
        );
    }

    #[test]
    fn parse_recipient_list_trims_and_deduplicates_case_insensitively() {
        assert_eq!(
            parse_recipient_list(" a@example.com , B@Example.com ,a@example.com, "),
            vec!["a@example.com".to_string(), "B@Example.com".to_string()]
        );
        assert!(parse_recipient_list("").is_empty());
        assert!(parse_recipient_list("  ,  ").is_empty());
    }

    #[test]
    fn format_recipient_list_round_trips_the_stored_value() {
        let recipients = vec!["a@example.com".to_string(), "b@example.com".to_string()];
        assert_eq!(
            format_recipient_list(&recipients),
            "a@example.com,b@example.com"
        );
        assert_eq!(
            parse_recipient_list(&format_recipient_list(&recipients)),
            recipients
        );
        assert_eq!(format_recipient_list(&[]), "");
    }

    #[test]
    fn parse_excluded_ids_drops_blanks_duplicates_and_junk() {
        assert_eq!(parse_excluded_ids(" 3 , 7,3 , ,x, 11 "), vec![3, 7, 11]);
        assert!(parse_excluded_ids("").is_empty());
        assert!(parse_excluded_ids(" , ").is_empty());
        // A hard-deleted user leaves a stale ID behind; it simply matches nobody.
        assert_eq!(parse_excluded_ids("999999"), vec![999999]);
    }

    #[test]
    fn format_excluded_ids_round_trips_the_stored_value() {
        assert_eq!(format_excluded_ids(&[3, 7, 11]), "3,7,11");
        assert_eq!(format_excluded_ids(&[]), "");
        // Duplicates are normalized away on save.
        assert_eq!(format_excluded_ids(&[3, 3, 7]), "3,7");
        assert_eq!(
            parse_excluded_ids(&format_excluded_ids(&[3, 7])),
            vec![3, 7]
        );
    }

    /// The three-colour scale the dashboard tile paints: everything that only
    /// waits for a decision is amber, everything still missing is red.
    #[test]
    fn readiness_maps_onto_the_traffic_light_scale() {
        use status_value::*;
        assert_eq!(unambiguous_status(MonthExportReadiness::Ready), Some(READY));
        assert_eq!(
            unambiguous_status(MonthExportReadiness::WeeksNotSubmitted),
            Some(NOT_SUBMITTED)
        );
        assert_eq!(
            unambiguous_status(MonthExportReadiness::UnresolvedTimeEntries),
            Some(NOT_SUBMITTED)
        );
        assert_eq!(
            unambiguous_status(MonthExportReadiness::PreStartContent),
            Some(NOT_SUBMITTED)
        );
    }

    /// Two readiness values cannot pick their own colour and must stay in
    /// `status_for_member`'s hands:
    ///
    /// * a pending absence request is returned before week submission is
    ///   checked, so somebody who *also* owes weeks would be painted amber
    ///   instead of red;
    /// * unapproved entries cover drafts as well as submitted rows, and a
    ///   draft was never handed to an approver.
    #[test]
    fn ambiguous_readiness_needs_the_extra_lookup() {
        assert_eq!(
            unambiguous_status(MonthExportReadiness::PendingAbsenceRequests),
            None
        );
        assert_eq!(
            unambiguous_status(MonthExportReadiness::UnapprovedTimeEntries),
            None
        );
    }

    /// Every `MonthExportReadiness` variant must have a reason key, so a new
    /// variant cannot silently show up as "ready" on the tile.
    #[test]
    fn every_non_ready_readiness_has_a_reason() {
        for readiness in [
            MonthExportReadiness::PreStartContent,
            MonthExportReadiness::UnresolvedTimeEntries,
            MonthExportReadiness::PendingAbsenceRequests,
            MonthExportReadiness::WeeksNotSubmitted,
            MonthExportReadiness::UnapprovedTimeEntries,
        ] {
            assert!(
                readiness_reason_key(readiness).is_some(),
                "{readiness:?} must explain itself"
            );
        }
        assert!(readiness_reason_key(MonthExportReadiness::Ready).is_none());
    }

    fn config(include_assistant_hours: bool, include_employee_hours: bool) -> PayrollReportConfig {
        PayrollReportConfig {
            enabled: true,
            recipients: vec!["payroll@example.com".into()],
            day_of_month: 5,
            include_assistant_hours,
            include_employee_hours,
            excluded_user_ids: Vec::new(),
        }
    }

    fn category(
        slug: &str,
        cost_type: &str,
        auto_approve_past: bool,
        unpaid: bool,
    ) -> AbsenceCategory {
        AbsenceCategory {
            id: 1,
            slug: slug.to_string(),
            name: slug.to_string(),
            color: "#000000".to_string(),
            sort_order: 0,
            active: true,
            cost_type: cost_type.to_string(),
            auto_approve_past,
            unpaid,
            medical_certificate_relevant: false,
            leave_account_default_days: (cost_type == "vacation").then_some(30),
            leave_account_carryover_expiry: (cost_type == "vacation")
                .then_some("03-31".to_string()),
            leave_account_start_year: (cost_type == "vacation").then_some(2026),
        }
    }

    /// The snapshot send refuses a document nobody appears in, and describes
    /// itself by who it actually names — booking time is not the same as
    /// having anything approved to report.
    #[test]
    fn people_in_report_counts_only_who_actually_appears() {
        let empty = PayrollReportData {
            period_label: "August 2026".into(),
            organization_name: String::new(),
            // Section present but with no rows: what a mid-month snapshot
            // looks like before anything has been approved.
            absence_rows: Some(vec![]),
            hours_sections: vec![PayrollHoursSection {
                heading_key: ASSISTANT_HOURS_HEADING_KEY,
                rows: vec![],
            }],
            late_entry_rows: Vec::new(),
            declared_work_days: Vec::new(),
            carried_work_days: Vec::new(),
            reported_absence_ids: Vec::new(),
            late_absence_rows: Vec::new(),
            late_absence_ids: Vec::new(),
            created_on: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            provisional: None,
        };
        assert_eq!(
            people_in_report(&empty),
            0,
            "an empty document covers nobody and must not be sent"
        );

        // Every row present is counted, zero-valued or not. That is only
        // sound because the builder never emits a meaningless row in the mode
        // whose emptiness is actually checked: an interim report drops
        // employees' "0 days, 0:00" rows (`build_hours_rows`'s
        // `drop_zero_rows`), and a finished month keeps them deliberately,
        // where they genuinely mean "worked nothing". The end-to-end guarantee
        // lives in `payroll_snapshot_with_employee_hours_never_reports_empty_rows`.
        let zero_rows_for_a_finished_month = PayrollReportData {
            hours_sections: vec![PayrollHoursSection {
                heading_key: EMPLOYEE_HOURS_HEADING_KEY,
                rows: vec![PayrollHoursRow {
                    user_id: 1,
                    employee: "Doe, Jane".into(),
                    work_days: 0,
                    minutes: 0,
                }],
            }],
            ..PayrollReportData {
                period_label: "August 2026".into(),
                organization_name: String::new(),
                absence_rows: Some(vec![]),
                hours_sections: vec![],
                late_entry_rows: Vec::new(),
                declared_work_days: Vec::new(),
                carried_work_days: Vec::new(),
                reported_absence_ids: Vec::new(),
                late_absence_rows: Vec::new(),
                late_absence_ids: Vec::new(),
                created_on: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
                provisional: None,
            }
        };
        assert_eq!(
            people_in_report(&zero_rows_for_a_finished_month),
            1,
            "a finished month's zero row is a statement about that person"
        );

        let populated = PayrollReportData {
            absence_rows: Some(vec![PayrollAbsenceRow {
                user_id: 1,
                employee: "Doe, Jane".into(),
                category: "Sick".into(),
                from: NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 8, 4).unwrap(),
                days: 2.0,
                medical_certificate_required: None,
            }]),
            hours_sections: vec![PayrollHoursSection {
                heading_key: ASSISTANT_HOURS_HEADING_KEY,
                rows: vec![
                    PayrollHoursRow {
                        // Same person as the absence row above — counted once.
                        user_id: 1,
                        employee: "Doe, Jane".into(),
                        work_days: 3,
                        minutes: 600,
                    },
                    PayrollHoursRow {
                        user_id: 2,
                        employee: "Roe, Sam".into(),
                        work_days: 2,
                        minutes: 400,
                    },
                ],
            }],
            ..empty
        };
        assert_eq!(people_in_report(&populated), 2);
    }

    #[test]
    fn has_no_content_only_when_every_section_is_off() {
        let sick = category(
            "sick",
            crate::repository::absence_categories::COST_TYPE_NONE,
            true,
            false,
        );
        assert!(config(false, false).has_no_content(&[]));
        assert!(!config(false, false).has_no_content(&[sick]));
        assert!(!config(true, false).has_no_content(&[]));
        assert!(!config(false, true).has_no_content(&[]));
    }

    #[test]
    fn includes_hours_for_splits_assistants_from_everyone_else() {
        let assistants_only = config(true, false);
        assert!(assistants_only.includes_hours_for("assistant"));
        assert!(!assistants_only.includes_hours_for("employee"));
        assert!(!assistants_only.includes_hours_for("team_lead"));

        let employees_only = config(false, true);
        assert!(!employees_only.includes_hours_for("assistant"));
        assert!(employees_only.includes_hours_for("employee"));
        assert!(employees_only.includes_hours_for("admin"));

        let neither = config(false, false);
        assert!(!neither.includes_hours_for("assistant"));
        assert!(!neither.includes_hours_for("employee"));
    }
}
