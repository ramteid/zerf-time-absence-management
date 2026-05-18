import { api } from "../../api.js";

export function getWeekEntries(from, to) {
  return api(`/time-entries?from=${from}&to=${to}`);
}

export function getReopenRequests() {
  return api("/reopen-requests");
}

export function getCategories() {
  return api("/categories");
}

export function getAbsencesByYear(year) {
  return api(`/absences?year=${year}`);
}

export function getHolidaysByYear(year) {
  return api(`/holidays?year=${year}`);
}

export function submitWeekEntries(ids) {
  return api("/time-entries/submit", { method: "POST", body: { ids } });
}

export function requestWeekReopen(weekStart, reason) {
  return api("/reopen-requests", {
    method: "POST",
    body: { week_start: weekStart, reason },
  });
}

export async function getWeekData({ from, to, years, fallbackCategories }) {
  const [
    entries,
    reopenRows,
    categoryRows,
    absenceRowsByYear,
    holidayRowsByYear,
  ] = await Promise.all([
    getWeekEntries(from, to),
    getReopenRequests().catch(() => []),
    getCategories().catch(() => fallbackCategories),
    Promise.all(years.map((year) => getAbsencesByYear(year).catch(() => []))),
    Promise.all(years.map((year) => getHolidaysByYear(year).catch(() => []))),
  ]);

  return {
    entries,
    reopenRows,
    categoryRows,
    absenceRowsByYear,
    holidayRowsByYear,
  };
}
