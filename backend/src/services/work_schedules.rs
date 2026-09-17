//! Which weekdays a contract places work on, for the calculations that ask.
//!
//! Every flextime and leave figure in Zerf is recomputed from scratch on each
//! query, so the answer has to be the one that was true on the day being
//! judged. [`crate::time_calc::WorkScheduleHistory`] carries that timeline.
//! This module is the only place a calculation should obtain one.

use crate::db::DatabasePool;
use crate::error::AppResult;
use crate::repository::WorkScheduleDb;
use crate::time_calc::{WorkSchedule, WorkScheduleHistory};
use chrono::NaiveDate;

/// One contract's working-day timeline together with the day it begins.
///
/// The two travel as a pair because every calculation needs both. The timeline
/// says which days the contract works; the start date says from when it exists
/// at all. A day before it carries no target and costs no leave day, the same
/// way every view in the app hides content stored before a start date.
pub struct ContractSchedule {
    pub history: WorkScheduleHistory,
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
pub async fn contract_schedule(
    pool: &DatabasePool,
    user_id: i64,
) -> AppResult<ContractSchedule> {
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
    Ok(ContractSchedule {
        history,
        start_date: contract.start_date,
    })
}
