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

use chrono::{Datelike, NaiveDate};
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
    /// "YYYY-MM" — the month the card shows: the one asked for, or by default
    /// the one that needs attention (see [`build`]).
    pub period: String,
    /// Localized month name, e.g. "Juli 2026".
    pub period_label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// The month is still running: only weeks past their Friday are due, so a
    /// person who is not behind is "up to date" rather than done.
    pub in_progress: bool,
    pub total: usize,
    pub ready: usize,
    pub awaiting_approval: usize,
    pub not_submitted: usize,
    pub members: Vec<SubmissionStatusMember>,
}

/// Build the Team submissions card.
///
/// `show_current_month` picks the month: `Some(true)` the running one,
/// `Some(false)` the previous one. `None` — a dashboard opening without a
/// choice of its own — shows the month that needs attention: the previous one
/// while anybody on the card still has something open there, otherwise the
/// running one. Always opening on the previous month left a closed month on
/// the card for most of every month, and the reader had to switch over again
/// on every visit.
pub async fn build(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    language: &Language,
    show_current_month: Option<bool>,
) -> AppResult<SubmissionStatus> {
    let today = settings::app_today(&app_state.pool).await;
    match show_current_month {
        Some(current) => build_for_month(app_state, requester, language, today, current).await,
        None => {
            let previous = build_for_month(app_state, requester, language, today, false).await?;
            if previous.ready < previous.total {
                Ok(previous)
            } else {
                build_for_month(app_state, requester, language, today, true).await
            }
        }
    }
}

/// The card for the previous month, or for the running one when
/// `show_current_month` is set.
async fn build_for_month(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    language: &Language,
    today: NaiveDate,
    show_current_month: bool,
) -> AppResult<SubmissionStatus> {
    let period = if show_current_month {
        schedule::current_period(today)
    } else {
        schedule::previous_period(today)
    };
    let (from, to) = schedule::period_bounds(&period)?;
    let period_label = crate::i18n::format_month(language, from.year(), from.month());

    let open_items = OpenItems::load(app_state, from, to, today).await?;
    let members = members_for(app_state, requester, from, to, today, &open_items).await?;
    let mut status = SubmissionStatus {
        period,
        period_label,
        from,
        to,
        in_progress: show_current_month,
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
/// nothing yet — except administrators. Assistants work irregularly without a
/// target, so they only count when the month holds something of theirs: a
/// booking, or a request still waiting for a decision. Otherwise the approval
/// list could show an assistant's request that the card knows nothing about.
///
/// People who have left or no longer track time are not on the card. Their
/// rows are kept unchanged and hidden from every team and approval view, so
/// nobody could act on anything the card showed for them.
///
/// A team lead sees the people they approve — the same people their approval
/// lists cover, so the card and the lists agree. Not everybody else as
/// anonymous rows, which only added noise the lead could do nothing about, and
/// not themselves: their own pending items wait for *their* approver, so an
/// amber row would point at a decision they cannot make. Their own month is on
/// their personal dashboard. An admin sees everybody.
async fn members_for(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
    open: &OpenItems,
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
        members.retain(|member| member.id != requester.id && team.contains(&member.id));
    }
    let booked_something = reports_db
        .user_ids_with_time_entries_in_range(from, to)
        .await?;
    members.retain(|member| {
        !is_admin_role(&member.role)
            && (!is_assistant_role(&member.role)
                || booked_something.contains(&member.id)
                || open.awaiting_decision.contains(&member.id))
    });
    Ok(members)
}

/// What is still open in the month, for everybody at once — one query per
/// question instead of several per person. Rows before somebody's start date
/// are hidden everywhere in the app, so the queries leave them out.
struct OpenItems {
    from: NaiveDate,
    /// Last day that is due, see [`reports::due_through`].
    due_to: NaiveDate,
    today: NaiveDate,
    /// Drafts or uncorrected rejections on a day that is due.
    not_handed_in: HashSet<i64>,
    /// Anything the dashboard's approval lists offer for a decision.
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
        let due_to = reports::due_through(to, today);
        let not_handed_in = if due_to >= from {
            reports_db
                .user_ids_with_unsubmitted_entries_since_start(from, due_to)
                .await?
        } else {
            HashSet::new()
        };
        // Decisions count for the whole month, also in the one still running:
        // a request for a later week sits in the approval list right now and
        // can be decided right now, just like something handed in early.
        // Judging it only up to the week being worked made the card say "up
        // to date" next to a list that still held a decision for that person.
        let awaiting_decision = reports_db.user_ids_awaiting_a_decision(from, to).await?;
        Ok(Self {
            from,
            due_to,
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
/// * **Amber** — everything due is handed in, but something waits for an
///   approver's decision: a submitted entry, an absence or cancellation
///   request, or a request to reopen a week. The same things the approval
///   lists on the dashboard offer, so the card and the lists always agree.
/// * **Green** — nothing is open.
///
/// Assistants and contracts without weekly hours owe no weeks, so only their
/// actual bookings can turn them red.
async fn month_status(
    app_state: &AppState,
    user: &User,
    open: &OpenItems,
) -> AppResult<MonthStatus> {
    if open.not_handed_in.contains(&user.id) {
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
    Ok(if open.awaiting_decision.contains(&user.id) {
        MonthStatus::AwaitingApproval
    } else {
        MonthStatus::Ready
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
