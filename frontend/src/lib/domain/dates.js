import { addDays, dateKey, isoDate, parseDate } from "../../format.js";

export function monthKey(dateValue) {
  const date = parseDate(dateValue);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
}

export function monthStart(month) {
  return `${month}-01`;
}

export function monthEnd(month) {
  const [yearPart, monthPart] = String(month).split("-");
  const year = Number(yearPart);
  const monthNumber = Number(monthPart);
  const lastDay = new Date(year, monthNumber, 0).getDate();
  return `${month}-${String(lastDay).padStart(2, "0")}`;
}

export function isoMonthStart(dateValue) {
  return `${monthKey(dateValue)}-01`;
}

export function yearsBetweenDates(from, to) {
  const startYear = Number(String(from).slice(0, 4));
  const endYear = Number(String(to).slice(0, 4));
  if (!Number.isFinite(startYear) || !Number.isFinite(endYear)) return [];
  const minYear = Math.min(startYear, endYear);
  const maxYear = Math.max(startYear, endYear);
  return Array.from(
    { length: maxYear - minYear + 1 },
    (_, index) => minYear + index,
  );
}

export function yearsInWeek(weekStart) {
  const start = parseDate(weekStart);
  const end = addDays(start, 6);
  return Array.from(new Set([start.getFullYear(), end.getFullYear()]));
}

export function dateRangeOverlaps(rowStart, rowEnd, from, to) {
  return rowEnd >= from && rowStart <= to;
}

export function sortByIsoDateAndStartTime(rows, dateField = "entry_date") {
  return [...(rows || [])].sort((a, b) => {
    const dateDiff = dateKey(a?.[dateField]).localeCompare(
      dateKey(b?.[dateField]),
    );
    if (dateDiff !== 0) return dateDiff;
    return String(a?.start_time || "").localeCompare(
      String(b?.start_time || ""),
    );
  });
}

export function isoWeekRange(weekStart) {
  const start = parseDate(weekStart);
  return {
    from: isoDate(start),
    to: isoDate(addDays(start, 6)),
  };
}
