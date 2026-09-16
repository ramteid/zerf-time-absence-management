import { api } from "../../api.js";
import { addDays, dateKey, isoDate, parseDate } from "../../format.js";
import { tracksOwnTime } from "../../rolePolicy.js";
import { REPORT_RANGE_MAX_DAY_DIFFERENCE } from "../domain/dates.js";
import {
  normalizeFlextimeResponse,
  normalizeOvertimeResponse,
} from "../domain/reports.js";

// Approved entries on the days the pending submissions touch.
//
// A day can already carry approved hours — nothing stops someone booking more
// time into a week that was signed off — and the automatic break is worked out
// over the whole day, not over the entries being approved. Without these rows
// the approval queue cannot say what a week will actually credit.
//
// The window is the span of the submissions themselves, so it is as narrow as
// the queue is. It is capped at the range the endpoint accepts: a submission
// left pending for more than a year is pathological, and covering the most
// recent year of it beats failing the whole dashboard load.
async function getApprovedEntriesForPendingDays(submittedEntries) {
  const days = (submittedEntries || [])
    .map((entry) => dateKey(entry.entry_date))
    .filter(Boolean)
    .sort();
  if (days.length === 0) return [];
  const to = days[days.length - 1];
  const earliestAllowed = isoDate(
    addDays(parseDate(to), -REPORT_RANGE_MAX_DAY_DIFFERENCE),
  );
  const from = days[0] > earliestAllowed ? days[0] : earliestAllowed;
  return api(`/time-entries/all?status=approved&from=${from}&to=${to}`);
}

export async function getApprovalDashboard() {
  const [
    submittedTimeEntries,
    requestedAbsences,
    pendingReopenRequests,
    users,
  ] = await Promise.all([
    api("/time-entries/all?status=submitted"),
    api("/absences/all?status=pending_review"),
    api("/reopen-requests/pending"),
    api("/users"),
  ]);
  // Depends on the submissions, so it cannot join the batch above. Failing it
  // must not cost the approver their queue: without these rows the week totals
  // fall back to counting the submitted entries alone, which is what they did
  // before this existed.
  const approvedTimeEntries = await getApprovedEntriesForPendingDays(
    submittedTimeEntries,
  ).catch(() => []);
  return {
    submittedTimeEntries,
    approvedTimeEntries,
    requestedAbsences,
    pendingReopenRequests,
    // Pure-admin users (tracks_time=false) have no time/absence data of their
    // own, so they are excluded from the team roster used by approval queues
    // and the team-members count. Inactive users are also excluded.
    users: (users || []).filter((u) => tracksOwnTime(u) && u.active !== false),
  };
}

// Both endpoints answer with `{ rows|days, balance_as_of }`. Normalizing right
// here means every caller sees `{ days|rows, balanceAsOf }` and none of them
// can accidentally treat the envelope as a plain array.
export async function getFlextime({ from, to }) {
  return normalizeFlextimeResponse(
    await api(`/reports/flextime?from=${from}&to=${to}`),
  );
}

export async function getOvertimeSummary(year) {
  return normalizeOvertimeResponse(await api(`/reports/overtime?year=${year}`));
}

export function getMonthSubmissionReport(month) {
  return api(`/reports/month?month=${month}`);
}

export function getTeamAbsences(params) {
  return api(`/absences/all?${params}`);
}

export function approveWeek(ids) {
  return api("/time-entries/batch-approve", {
    method: "POST",
    body: { ids },
  });
}

export function rejectWeek(ids, reason) {
  return api("/time-entries/batch-reject", {
    method: "POST",
    body: { ids, reason },
  });
}

export function approveAbsenceById(absence) {
  const endpoint =
    absence.status === "cancellation_pending"
      ? `/absences/${absence.id}/approve-cancellation`
      : `/absences/${absence.id}/approve`;
  return api(endpoint, { method: "POST" });
}

export function rejectAbsenceById(absence, reason) {
  if (absence.status === "cancellation_pending") {
    return api(`/absences/${absence.id}/reject-cancellation`, {
      method: "POST",
    });
  }
  return api(`/absences/${absence.id}/reject`, {
    method: "POST",
    body: { reason },
  });
}

export function approveReopen(id) {
  return api(`/reopen-requests/${id}/approve`, { method: "POST", body: {} });
}

export function rejectReopen(id, reason) {
  return api(`/reopen-requests/${id}/reject`, {
    method: "POST",
    body: { reason },
  });
}

// Submission progress for the dashboard tile (team leads and admins).
// `current` is the tile's transient, non-persistent "show this month" peek —
// it reports the in-progress month instead of the tracked previous period.
export function getSubmissionStatus(current = false) {
  return api(`/reports/submission-status${current ? "?current=true" : ""}`);
}

// What the payroll report for the tracked month holds — or, while the month is
// still running, what it is shaping up to hold. Same period logic as above.
export function getPayrollContent(current = false) {
  return api(`/reports/payroll-content${current ? "?current=true" : ""}`);
}
