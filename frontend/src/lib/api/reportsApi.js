import { api } from "../../api.js";

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
  return canViewTeamReports ? api("/users") : [currentUser];
}

export function getMonthReport({ userId, month }) {
  return api(`/reports/month?${paramsFrom({ user_id: userId, month })}`);
}

export function getLeaveBalance({ userId, year }) {
  return api(`/leave-balance/${userId}?${paramsFrom({ year })}`);
}

export function getOvertimeReport({ userId, year }) {
  return api(`/reports/overtime?${paramsFrom({ user_id: userId, year })}`);
}

export function getFlextimeReport({ userId, from, to }) {
  return api(`/reports/flextime?${paramsFrom({ user_id: userId, from, to })}`);
}

export function getTeamReport({ month }) {
  return api(`/reports/team?${paramsFrom({ month })}`);
}

export function getCategoryReport({ userId, from, to }) {
  return api(
    `/reports/categories?${paramsFrom({ user_id: userId, from, to })}`,
  );
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

export function getUserAbsencesByYear(year) {
  return api(`/absences?year=${year}`);
}

export function getHolidaysByYear(year) {
  return api(`/holidays?year=${year}`);
}
