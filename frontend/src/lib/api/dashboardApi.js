import { api } from "../../api.js";
import { tracksOwnTime } from "../../rolePolicy.js";

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

export function getFlextime({ from, to }) {
  return api(`/reports/flextime?from=${from}&to=${to}`);
}

export function getOvertimeSummary(year) {
  return api(`/reports/overtime?year=${year}`);
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
