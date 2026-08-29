import { api } from "../../api.js";
import { tracksOwnTime } from "../../rolePolicy.js";
import {
  normalizeFlextimeResponse,
  normalizeOvertimeResponse,
} from "../domain/reports.js";

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
  return {
    submittedTimeEntries,
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
