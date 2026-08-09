import { api } from "../../api.js";
import { tracksOwnTime } from "../../rolePolicy.js";
import { sortUsersByRoleThenName } from "../domain/users.js";

function paramsFrom(values) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== "") {
      params.set(key, value);
    }
  }
  return params.toString();
}

export async function getUsersForReports(canViewTeamReports, currentUser) {
  if (!canViewTeamReports) {
    return tracksOwnTime(currentUser) ? [currentUser] : [];
  }
  // Use /reports/users which scopes the list to users the requester can access
  // reports for: team leads see their direct reports + themselves; admins see
  // all active time-tracking users. This matches the team report table scope
  // exactly, so the employee dropdown and team table always show the same set.
  const reportUsers = await api("/reports/users");
  return sortUsersByRoleThenName(reportUsers || []);
}

export function getMonthReport({ userId, month }) {
  return api(`/reports/month?${paramsFrom({ user_id: userId, month })}`);
}

export function getLeaveBalances({ userId, year }) {
  return api(`/leave-balances/${userId}?${paramsFrom({ year })}`);
}

export function getFlextimeReport({ userId, from, to }) {
  return api(`/reports/flextime?${paramsFrom({ user_id: userId, from, to })}`);
}

export function getTeamReport({ month }) {
  return api(`/reports/team?${paramsFrom({ month })}`);
}

export function getTeamCategoryReport({ from, to }) {
  return api(`/reports/team-categories?${paramsFrom({ from, to })}`);
}

export function getAbsenceReport({ from, to }) {
  return api(`/absences/all?${paramsFrom({ from, to })}`);
}

export function getRangeReport({ userId, from, to }) {
  return api(`/reports/range?${paramsFrom({ user_id: userId, from, to })}`);
}

// Returns the raw fetch Response (PDF content-type) so callers can read it as
// a blob. Pass userId === undefined/null to request the combined "All" PDF
// (leads/admins only — backend scopes it to the requester's active team).
export function getTimesheetPdf({ userId, from, to }) {
  return api(`/reports/pdf?${paramsFrom({ user_id: userId, from, to })}`);
}

export function getUserAbsencesByYear(year) {
  return api(`/absences?year=${year}`);
}

export function getHolidaysByYear(year) {
  return api(`/holidays?year=${year}`);
}
