//! Which weekdays a contract places work on, for the calculations that ask.
//!
//! Every flextime and leave figure in Zerf is recomputed from scratch on each
//! query, so the answer has to be the one that was true on the day being
//! judged. [`crate::time_calc::WorkScheduleHistory`] carries that timeline.
//! This module is the only place a calculation should obtain one.

use crate::db::DatabasePool;
use crate::error::{AppError, AppResult};
use crate::repository::WorkScheduleDb;
use crate::time_calc::{WeeklyHoursHistory, WorkSchedule, WorkScheduleHistory};
use chrono::NaiveDate;

/// One contract's working-day timeline together with the day it begins.
///
/// The two travel as a pair because every calculation needs both. The timeline
/// says which days the contract works; the start date says from when it exists
/// at all. A day before it carries no target and costs no leave day, the same
/// way every view in the app hides content stored before a start date.
pub struct ContractSchedule {
    pub history: WorkScheduleHistory,
    /// The weekly hours in force on each day, asked per day like `history`.
    pub weekly_hours: WeeklyHoursHistory,
    pub start_date: NaiveDate,
}

/// The timeline of working days a calculation must judge this contract by.
///
/// **A contract with no work target never has fixed working days**, whatever
/// rows it may carry. That is not a tidying rule; it is the difference between
/// two kinds of contract. An assistant is paid for the hours they are present,
/// so no weekday of theirs carries a target and every calendar day of an
/// absence is charged to them. Reading a leftover pattern for them would give
/// them a target on some days and none on others, and would make their leave
/// days cost differently depending on which weekday they fall on.
///
/// Somebody who moves from employee to assistant keeps their pattern rows on
/// the record rather than losing them, because those rows describe months they
/// really did work that way. What stops them being applied is this function,
/// not their deletion. That also matches the rest of the app: role, weekly
/// hours and day count are not dated either, so a report over an old period
/// already reads today's contract. Deleting the rows would additionally throw
/// the pattern away for good if the same person moved back.
///
/// `workdays_per_week` answers for dates no recorded pattern reaches, and for
/// everybody who has none. It is the old stored count, so those contracts keep
/// reading exactly as they read before any of this existed — see
/// [`WorkSchedule::weekly_leave_cap`], which is what makes that true rather
/// than merely intended.
pub async fn history_for(
    pool: &DatabasePool,
    user_id: i64,
    role: &str,
    tracks_time: bool,
    workdays_per_week: i16,
) -> AppResult<WorkScheduleHistory> {
    let no_fixed_days = WorkSchedule::without_fixed_days(workdays_per_week);
    if !crate::roles::has_work_target(role, tracks_time) {
        // The rows are deliberately not even read. Loading them and then
        // discarding them would leave a later edit one `if` away from applying
        // an assistant's old employee pattern to them.
        return Ok(WorkScheduleHistory::without_history(no_fixed_days));
    }
    WorkScheduleDb::new(pool.clone())
        .history_for_user(user_id, workdays_per_week)
        .await
}

/// The timeline and start date of one contract, looked up by user id.
///
/// For callers that hold nothing but the id — the leave-day counters, which are
/// reached from queries rather than from a loaded user. Callers that already
/// have the person's role and hours should use [`history_for`] and spend no
/// second query on the contract.
pub async fn contract_schedule(pool: &DatabasePool, user_id: i64) -> AppResult<ContractSchedule> {
    let contract = WorkScheduleDb::new(pool.clone())
        .contract_for_user(user_id)
        .await?;
    let history = history_for(
        pool,
        user_id,
        &contract.role,
        contract.tracks_time,
        contract.workdays_per_week,
    )
    .await?;
    let weekly_hours = weekly_hours_for(
        pool,
        user_id,
        &contract.role,
        contract.tracks_time,
        contract.weekly_hours,
    )
    .await?;
    Ok(ContractSchedule {
        history,
        weekly_hours,
        start_date: contract.start_date,
    })
}

/// The weekly hours a calculation must apply on each day of this contract.
///
/// A contract with no work target carries no minutes whatever its stored hours
/// (legacy data holds non-zero hours on assistants), so it has no target for
/// all time. Everybody else reads their recorded changes, falling back to the
/// current hours when none were ever recorded.
pub async fn weekly_hours_for(
    pool: &DatabasePool,
    user_id: i64,
    role: &str,
    tracks_time: bool,
    current_weekly_hours: f64,
) -> AppResult<WeeklyHoursHistory> {
    if !crate::roles::has_work_target(role, tracks_time) {
        return Ok(WeeklyHoursHistory::no_target());
    }
    WorkScheduleDb::new(pool.clone())
        .hours_history_for_user(user_id, current_weekly_hours)
        .await
}

/// Sort, de-duplicate and check a set of working days named by a request, so a
/// handler can refuse a bad one before anything is written.
pub fn normalised_weekdays(weekdays: &[i16]) -> AppResult<Vec<i16>> {
    WorkScheduleDb::normalised_weekdays(weekdays)
}

/// What one save of the user form says about a contract's working time.
///
/// Only built for a contract that has a work target after the save; one
/// without a target records no hours and no working days at all.
pub struct WorkingTimeUpdate<'a> {
    pub user_id: i64,
    /// The contract start after the save.
    pub contract_start: NaiveDate,
    pub today: NaiveDate,
    /// The weekly hours the weeks before this save were judged by: the stored
    /// hours, or `None` for a contract that had no work target until now.
    pub previous_hours: Option<f64>,
    /// The weekly hours the form sent, if any.
    pub new_hours: Option<f64>,
    /// The weekdays the form named, already normalised.
    pub named_weekdays: Option<&'a [i16]>,
    pub previous_day_count: i16,
    /// The day count after the save.
    pub new_day_count: i16,
    /// The first day the changed hours and working days apply to. Omitted
    /// means today, or the contract start for a contract not yet begun.
    pub changes_from: Option<NaiveDate>,
    pub changed_by: i64,
}

/// Record a change of weekly hours and working days, both from one date.
///
/// Hours and days are two halves of one contract change. Dating them apart is
/// what let them disagree: hours moved back to October with the days still
/// following from the day of saving gave the weeks in between the new hours
/// spread over the old days. One date for both makes that impossible.
///
/// Weeks before that date keep the hours and days they were worked under. The
/// date may lie in the past so a change entered late can still be dated
/// correctly, but never before the contract start and never in the future.
/// Only what actually changed is recorded: a save that leaves the hours or the
/// days as they are writes nothing for them, so an unrelated save can never
/// re-date an earlier change.
pub async fn record_working_time_tx(
    tx: &mut crate::db::PgConnection,
    update: WorkingTimeUpdate<'_>,
) -> AppResult<()> {
    let created_by = Some(update.changed_by);
    // A contract with no pattern at all gets a starting one. When the form
    // named the days, they are the starting pattern too: writing the
    // Monday-first guess for the earlier period and the named days from the
    // change on would invent a change that never happened.
    let starting_pattern = match update.named_weekdays {
        Some(days) => days.to_vec(),
        None => WorkScheduleDb::default_weekdays(update.new_day_count),
    };
    WorkScheduleDb::ensure_for_user_tx(
        tx,
        update.user_id,
        update.contract_start,
        &starting_pattern,
        created_by,
    )
    .await?;
    // A contract start moved earlier would otherwise leave the days between
    // the new start and the oldest record with no pattern and no hours.
    WorkScheduleDb::extend_earliest_to_tx(tx, update.user_id, update.contract_start).await?;
    WorkScheduleDb::extend_earliest_hours_to_tx(tx, update.user_id, update.contract_start).await?;

    // "Today" for a contract that has not begun yet is its first day: that is
    // the pattern the form shows, and the latest a change may start.
    let latest_allowed = update.today.max(update.contract_start);
    let current_pattern =
        WorkScheduleDb::weekdays_in_force_tx(tx, update.user_id, latest_allowed).await?;
    let wanted_pattern = wanted_weekdays(&update, current_pattern.as_deref());
    let changed_pattern = wanted_pattern.filter(|days| current_pattern.as_ref() != Some(days));
    let changed_hours = update
        .new_hours
        .filter(|hours| Some(*hours) != update.previous_hours);
    if changed_pattern.is_none() && changed_hours.is_none() {
        return Ok(());
    }

    let changes_from = update.changes_from.unwrap_or(latest_allowed);
    if changes_from < update.contract_start || changes_from > latest_allowed {
        return Err(AppError::BadRequest(
            "Changes to the working time must start between the contract start and today.".into(),
        ));
    }
    if let Some(new_hours) = changed_hours {
        WorkScheduleDb::record_hours_tx(
            tx,
            update.user_id,
            update.contract_start,
            update.previous_hours,
            changes_from,
            Some(new_hours),
            created_by,
        )
        .await?;
    }
    if let Some(days) = changed_pattern {
        WorkScheduleDb::record_weekdays_change_tx(
            tx,
            update.user_id,
            changes_from,
            &days,
            created_by,
        )
        .await?;
    }
    Ok(())
}

/// Record that a contract has no work target any more, from `from` on.
///
/// Every calculation already reads a contract without a target as having none
/// on any day, so this changes nothing today. It matters once the target comes
/// back (an assistant promoted again, time tracking switched back on): the
/// period in between then keeps no target and no flextime account, instead of
/// reading the hours from before and counting every week of it as hours not
/// worked.
pub async fn record_target_ended_tx(
    tx: &mut crate::db::PgConnection,
    user_id: i64,
    contract_start: NaiveDate,
    hours_until_then: f64,
    from: NaiveDate,
    changed_by: i64,
) -> AppResult<()> {
    WorkScheduleDb::record_hours_tx(
        tx,
        user_id,
        contract_start,
        Some(hours_until_then),
        from.max(contract_start),
        None,
        Some(changed_by),
    )
    .await
}

/// The working days the save asks for, or `None` when it says nothing about
/// them.
///
/// Named days are the authoritative statement. A changed day count alone (the
/// API still accepts one) has to change the pattern too: the count and the
/// pattern are two statements of the same fact, and left alone they drift, so
/// somebody moved from five days a week to three kept a Monday-to-Friday
/// pattern and went on being given a five-day target. A pattern that already
/// works that many days is kept as it is, so weekdays somebody picked by hand
/// are not overwritten with the Monday-first guess.
fn wanted_weekdays(
    update: &WorkingTimeUpdate<'_>,
    current_pattern: Option<&[i16]>,
) -> Option<Vec<i16>> {
    if let Some(days) = update.named_weekdays {
        return Some(days.to_vec());
    }
    if update.new_day_count == update.previous_day_count {
        return None;
    }
    let already_fits = current_pattern
        .is_some_and(|days| i16::try_from(days.len()).ok() == Some(update.new_day_count));
    if already_fits {
        return None;
    }
    Some(WorkScheduleDb::default_weekdays(update.new_day_count))
}
