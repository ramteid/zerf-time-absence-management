//! The dashboard's Submissions card: who has closed their month.
//!
//! The card answers one question for the people a lead is responsible for —
//! has everything that is due been handed in and decided? — and paints each
//! person in one of three colours. It is deliberately independent of the
//! payroll report. The report's exclusion list, its member rules and its
//! readiness gate all answer "what may this document contain", which is a
//! different question: somebody left out of the document still hands in weeks
//! and still needs them approved.

use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;

use crate::background::schedule;
use crate::error::AppResult;
use crate::i18n::Language;
use crate::repository::User;
use crate::roles::{has_submission_obligation, is_admin_role, is_assistant_role};
use crate::services::{reports, settings};
use crate::AppState;

/// Traffic-light colour of one person's month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MonthStatus {
    /// Everything due is handed in and decided.
    Ready,
    /// Everything due is handed in, but an entry or an absence request still
    /// waits for a decision — the approver's move.
    AwaitingApproval,
    /// Something due has not been handed in — the employee's move.
    NotSubmitted,
}

/// One row of the card's detail list.
#[derive(Serialize)]
pub struct SubmissionStatusMember {
    pub user_id: i64,
    pub name: String,
    pub status: MonthStatus,
}

/// Everything the Submissions card renders for the month it shows.
#[derive(Serialize)]
pub struct SubmissionStatus {
    /// "YYYY-MM" — the previous month by default, or the running month when
    /// the card's "show this month" toggle asked for it.
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

/// Build the Submissions card for the previous month, or for the running one
/// when `show_current_month` is set (a transient view choice, never stored).
pub async fn build(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    language: &Language,
    show_current_month: bool,
) -> AppResult<SubmissionStatus> {
    let today = settings::app_today(&app_state.pool).await;
    let period = if show_current_month {
        schedule::current_period(today)
    } else {
        schedule::previous_period(today)
    };
    let (from, to) = schedule::period_bounds(&period)?;
    let period_label = crate::i18n::format_month(language, from.year(), from.month());

    let members = members_for(app_state, requester, from, to, today).await?;
    let open_items = OpenItems::load(app_state, from, to, today).await?;
    let mut status = SubmissionStatus {
        period,
        period_label,
        from,
        to,
        total: members.len(),
        ready: 0,
        awaiting_approval: 0,
        not_submitted: 0,
        members: Vec::with_capacity(members.len()),
    };
    for member in members {
        let month_status = month_status(app_state, &member, &open_items).await?;
        match month_status {
            MonthStatus::Ready => status.ready += 1,
            MonthStatus::AwaitingApproval => status.awaiting_approval += 1,
            MonthStatus::NotSubmitted => status.not_submitted += 1,
        }
        status.members.push(SubmissionStatusMember {
            user_id: member.id,
            name: format!("{} {}", member.first_name, member.last_name),
            status: month_status,
        });
    }
    Ok(status)
}

/// The people whose month the requester gets to see.
///
/// Everybody who tracks time and has started by the end of the month — or by
/// today while it is still running, since somebody starting next week owes
/// nothing yet — except administrators, and except assistants who booked
/// nothing in the month: they work irregularly without a target, so an empty
/// month leaves them nothing to close.
///
/// People who have left or no longer track time are not on the card. Their
/// rows are kept unchanged and hidden from every team and approval view, so
/// nobody could act on anything the card showed for them.
///
/// A team lead sees their own team — themselves and their direct reports, the
/// same people the team report shows them: those are the people they can act
/// on. Counting everybody else as anonymous rows only added noise the lead
/// could do nothing about. An admin sees everybody.
async fn members_for(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
) -> AppResult<Vec<User>> {
    let reports_db = &app_state.db.reports;
    let mut members = reports_db
        .timesheet_members_for_period(to.min(today))
        .await?;
    if !requester.is_admin() {
        let team: HashSet<i64> = reports_db
            .active_team_members(requester.id, false)
            .await?
            .into_iter()
            .map(|member| member.id)
            .collect();
        members.retain(|member| team.contains(&member.id));
    }
    let booked_something = reports_db
        .user_ids_with_time_entries_in_range(from, to)
        .await?;
    members.retain(|member| {
        !is_admin_role(&member.role)
            && (!is_assistant_role(&member.role) || booked_something.contains(&member.id))
    });
    Ok(members)
}

/// What is still open in the month, for everybody at once — one query each
/// instead of three per person. Only somebody who started inside the month is
/// asked again individually (see [`month_status`]).
struct OpenItems {
    from: NaiveDate,
    /// Last day that is due, see [`due_until`].
    due_to: NaiveDate,
    /// Last day a pending decision is looked for, see [`reports::judged_period_end`].
    open_to: NaiveDate,
    today: NaiveDate,
    /// Drafts or uncorrected rejections on a day that is due.
    not_handed_in: HashSet<i64>,
    /// Submitted entries or absence requests waiting for a decision.
    awaiting_decision: HashSet<i64>,
}

impl OpenItems {
    async fn load(
        app_state: &AppState,
        from: NaiveDate,
        to: NaiveDate,
        today: NaiveDate,
    ) -> AppResult<Self> {
        let reports_db = &app_state.db.reports;
        let due_to = due_until(to, today);
        let not_handed_in = if due_to >= from {
            reports_db
                .user_ids_with_unsubmitted_time_entries_in_range(from, due_to)
                .await?
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        };
        // Decisions are judged up to the end of the week being worked, like
        // every other month check: a request for a later week is not open
        // business yet. Something handed in early counts, though — the
        // approver can act on it right away.
        let open_to = reports::judged_period_end(to, today);
        let mut awaiting_decision: HashSet<i64> = reports_db
            .user_ids_with_submitted_time_entries_in_range(from, open_to)
            .await?
            .into_iter()
            .collect();
        awaiting_decision.extend(
            reports_db
                .user_ids_with_requested_absences_in_range(from, open_to)
                .await?,
        );
        Ok(Self {
            from,
            due_to,
            open_to,
            today,
            not_handed_in,
            awaiting_decision,
        })
    }
}

/// One person's colour.
///
/// Red outranks amber: somebody who still owes a week is not "waiting for
/// approval" merely because they also have a request pending.
///
/// * **Red** — something due was not handed in: a week nobody submitted, or a
///   draft or an uncorrected rejection on a day that is due.
/// * **Amber** — everything due is handed in, but a submitted entry or an
///   absence request still waits for a decision.
/// * **Green** — nothing is open.
///
/// Assistants and contracts without weekly hours owe no weeks, so only their
/// actual bookings can turn them red.
async fn month_status(
    app_state: &AppState,
    user: &User,
    open: &OpenItems,
) -> AppResult<MonthStatus> {
    let reports_db = &app_state.db.reports;
    // Rows before somebody's start date are hidden everywhere in the app, so
    // nobody could hand them in or decide on them. The shared sets cover the
    // whole month; for somebody who started inside it, ask again from their
    // first day.
    let started_in_month = user.start_date > open.from;

    let not_handed_in = if started_in_month {
        user.start_date <= open.due_to
            && reports_db
                .has_unsubmitted_time_entries_in_range(user.id, user.start_date, open.due_to)
                .await?
    } else {
        open.not_handed_in.contains(&user.id)
    };
    if not_handed_in {
        return Ok(MonthStatus::NotSubmitted);
    }

    let owes_weeks = has_submission_obligation(&user.role, user.weekly_hours);
    if owes_weeks && open.due_to >= open.from {
        // Excuses days covered by a pending absence request: nobody can book
        // time on them, and the request itself makes the person amber below.
        let weeks_handed_in = reports::all_weeks_submitted_for_month_as_of(
            &app_state.pool,
            user.id,
            open.from,
            open.due_to,
            user.start_date,
            false,
            open.today,
        )
        .await?;
        if !weeks_handed_in {
            return Ok(MonthStatus::NotSubmitted);
        }
    }

    let awaiting_decision = if started_in_month {
        user.start_date <= open.open_to
            && (reports_db
                .has_pending_submitted_entries_in_range(user.id, user.start_date, open.open_to)
                .await?
                || reports_db
                    .has_requested_absences_in_period(user.id, user.start_date, open.open_to)
                    .await?)
    } else {
        open.awaiting_decision.contains(&user.id)
    };
    Ok(if awaiting_decision {
        MonthStatus::AwaitingApproval
    } else {
        MonthStatus::Ready
    })
}

/// Last day of the month `[.., to]` that is already due.
///
/// A finished month is due in full, including the days of its last week that
/// reach into the new month — handing those in is what closes it. In the
/// running month only the weeks that are already over are due: the week being
/// worked cannot be missing yet. Counting it painted almost everybody red on
/// any day before Friday, so the card could not tell "behind" from "Wednesday".
///
/// Returns a day before the month's start when no week of it is over yet.
fn due_until(to: NaiveDate, today: NaiveDate) -> NaiveDate {
    if to < today {
        return to;
    }
    let last_finished_sunday = crate::time_calc::week_monday(today) - Duration::days(1);
    to.min(last_finished_sunday)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn a_finished_month_is_due_in_full() {
        // Thursday, 1 October: September's last week is still running, but
        // the month is over, so its last days are due.
        assert_eq!(
            due_until(date(2026, 9, 30), date(2026, 10, 1)),
            date(2026, 9, 30)
        );
    }

    #[test]
    fn the_running_month_is_due_up_to_the_last_finished_week() {
        // Thursday, 15 October: the week of the 12th is still being worked.
        assert_eq!(
            due_until(date(2026, 10, 31), date(2026, 10, 15)),
            date(2026, 10, 11)
        );
        // On a Monday the week that ended yesterday is due.
        assert_eq!(
            due_until(date(2026, 10, 31), date(2026, 10, 19)),
            date(2026, 10, 18)
        );
        // The month's last day still belongs to the running month.
        assert_eq!(
            due_until(date(2026, 10, 31), date(2026, 10, 31)),
            date(2026, 10, 25)
        );
    }

    #[test]
    fn nothing_is_due_before_the_running_months_first_week_ends() {
        // Saturday, 3 October: no week of October is over yet.
        let from = date(2026, 10, 1);
        assert!(due_until(date(2026, 10, 31), date(2026, 10, 3)) < from);
    }

    #[test]
    fn statuses_keep_their_wire_names() {
        assert_eq!(serde_json::to_value(MonthStatus::Ready).unwrap(), "ready");
        assert_eq!(
            serde_json::to_value(MonthStatus::AwaitingApproval).unwrap(),
            "awaiting_approval"
        );
        assert_eq!(
            serde_json::to_value(MonthStatus::NotSubmitted).unwrap(),
            "not_submitted"
        );
    }
}
