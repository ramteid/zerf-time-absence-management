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

/**
 * What each day of the week asks for, in minutes, as the server works it out.
 *
 * `target_min` counts only up to today, which is what a running week total is
 * built from; `full_target_min` ignores that cutoff, which is what a day card
 * shows. Both follow the weekdays the contract actually works, so the page no
 * longer needs a copy of that rule.
 *
 * A failure is not fatal: the targets simply go missing for that week rather
 * than taking the whole timesheet down with them.
 */
export async function getWeekTargets(from, to) {
  try {
    const report = await api(`/reports/range?from=${from}&to=${to}`);
    return new Map(
      (report?.days || []).map((day) => [
        day.date,
        {
          targetMin: day.target_min ?? 0,
          fullTargetMin: day.full_target_min ?? 0,
        },
      ]),
    );
  } catch {
    return new Map();
  }
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
    dayTargets,
  ] = await Promise.all([
    getWeekEntries(from, to),
    getReopenRequests().catch(() => []),
    getCategories().catch(() => fallbackCategories),
    Promise.all(years.map((year) => getAbsencesByYear(year).catch(() => []))),
    Promise.all(years.map((year) => getHolidaysByYear(year).catch(() => []))),
    getWeekTargets(from, to),
  ]);

  return {
    entries,
    reopenRows,
    categoryRows,
    absenceRowsByYear,
    holidayRowsByYear,
    dayTargets,
  };
}
