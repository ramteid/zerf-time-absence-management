import { addDays, dateKey, isoDate, parseDate } from "../../format.js";

export function monthKey(dateValue) {
  const date = parseDate(dateValue);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
}

// Returns "" for anything that isn't a real "YYYY-MM", mirroring monthEnd().
// Without this an empty month produced the string "-01", which JavaScript's
// Date parser silently accepts as the 1st of January 2001 — turning "no month
// selected yet" into a date 25 years in the past.
export function monthStart(month) {
  if (typeof month !== "string" || !/^\d{4}-\d{2}$/.test(month)) return "";
  return `${month}-01`;
}

export function monthEnd(month) {
  const [yearPart, monthPart] = String(month).split("-");
  const year = Number(yearPart);
  const monthNumber = Number(monthPart);
  if (!Number.isFinite(year) || !Number.isFinite(monthNumber)) return "";
  if (monthNumber < 1 || monthNumber > 12) return "";
  const lastDay = new Date(year, monthNumber, 0).getDate();
  if (!Number.isFinite(lastDay)) return "";
  return `${month}-${String(lastDay).padStart(2, "0")}`;
}

export function isoMonthStart(dateValue) {
  return `${monthKey(dateValue)}-01`;
}

// Callers fan out one API request per returned year, so a bogus bound must
// never widen this list: an unparseable date used to fall back to String(),
// yielding year 0 and a two-thousand-entry span. Three is the widest a
// legitimate range reaches — the 366-day cap can straddle three calendar
// years (e.g. 2025-12-31 → 2027-01-01).
const MAX_YEAR_SPAN = 3;

export function yearsBetweenDates(from, to) {
  // Accept YYYY-MM-DD strings or Date objects – extract year robustly via dateKey/parseDate
  const fromKey = dateKey(from);
  const toKey = dateKey(to);
  if (!fromKey || !toKey) return [];
  const startYear = Number(String(fromKey).slice(0, 4));
  const endYear = Number(String(toKey).slice(0, 4));
  if (!Number.isFinite(startYear) || !Number.isFinite(endYear)) return [];
  const minYear = Math.min(startYear, endYear);
  const maxYear = Math.max(startYear, endYear);
  if (maxYear - minYear + 1 > MAX_YEAR_SPAN) return [];
  return Array.from(
    { length: maxYear - minYear + 1 },
    (_, index) => minYear + index,
  );
}

export function daysBetweenIsoDates(from, to) {
  const start = parseDate(from);
  const end = parseDate(to);
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime())) {
    return null;
  }
  // Use UTC midnight to avoid DST 23h/25h artifacts
  const startUtc = Date.UTC(
    start.getFullYear(),
    start.getMonth(),
    start.getDate(),
  );
  const endUtc = Date.UTC(end.getFullYear(), end.getMonth(), end.getDate());
  return Math.round((endUtc - startUtc) / 86400000);
}

// Unparseable bounds count as "too long": callers use this to decide whether
// it is safe to fan out per-year requests, and a range they cannot even
// measure is never safe to expand.
export function isReportRangeTooLong(from, to, maxDays = 366) {
  const days = daysBetweenIsoDates(from, to);
  return days == null || days > maxDays;
}

export function yearsInWeek(weekStart) {
  const start = parseDate(weekStart);
  const end = addDays(start, 6);
  return Array.from(new Set([start.getFullYear(), end.getFullYear()]));
}

export function dateRangeOverlaps(rowStart, rowEnd, from, to) {
  // Normalize to ISO date keys to avoid Date vs string coercion bugs.
  const rs = dateKey(rowStart) || String(rowStart);
  const re = dateKey(rowEnd) || String(rowEnd);
  const fs = dateKey(from) || String(from);
  const ft = dateKey(to) || String(to);
  return re >= fs && rs <= ft;
}

export function sortByIsoDateAndStartTime(rows, dateField = "entry_date") {
  return [...(rows || [])].sort((a, b) => {
    const dateDiff = dateKey(a?.[dateField]).localeCompare(
      dateKey(b?.[dateField]),
    );
    if (dateDiff !== 0) return dateDiff;
    // Normalize times to HH:MM:SS for stable sort (handle "8:00" vs "08:00")
    const normalize = (t) => {
      if (!t) return "";
      const parts = String(t).split(":");
      const hh = String(parts[0] || "0").padStart(2, "0");
      const mm = String(parts[1] || "0").padStart(2, "0");
      const ss = parts[2] ? String(parts[2]).padStart(2, "0") : "00";
      return `${hh}:${mm}:${ss}`;
    };
    return normalize(a?.start_time).localeCompare(normalize(b?.start_time));
  });
}

export function isoWeekRange(weekStart) {
  const start = parseDate(weekStart);
  return {
    from: isoDate(start),
    to: isoDate(addDays(start, 6)),
  };
}
