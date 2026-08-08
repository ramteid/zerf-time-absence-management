// Period math shared by the Reports page's toolbar, the person/team report
// sections, and the CSV/PDF export. Centralising it keeps "what date range am I
// actually querying?" consistent everywhere instead of re-deriving it per file.
import { monthEnd, monthStart } from "./dates.js";

// The raw selected bounds, regardless of whether they lie in the past or future.
// Month mode expands the "YYYY-MM" into first/last day of that month.
export function periodBounds({ mode, month, from, to }) {
  if (mode === "range") {
    if (!from || !to) return { from: "", to: "" };
    return { from, to };
  }
  if (!month || typeof month !== "string" || !/^\d{4}-\d{2}$/.test(month)) {
    return { from: "", to: "" };
  }
  return { from: monthStart(month), to: monthEnd(month) };
}

// Bounds for TIME-based queries (worked hours, flextime, categories, exports).
// Time data has no future, so `to` is capped at today; `active` is false when
// the whole selected range lies in the future (then callers skip the fetch and
// show an empty state). Absence queries deliberately do NOT use this — planned
// absences look forward — they use periodBounds() directly.
export function timeQueryRange(period, todayIso) {
  const { from, to } = periodBounds(period);
  if (!from || !to) return { from: "", to: "", active: false };
  const cappedTo = to && todayIso && to > todayIso ? todayIso : to;
  const active = from && todayIso ? from <= todayIso : false;
  return { from, to: cappedTo, active };
}

// The single calendar year a leave-account card should report on, or null when
// the range spans more than one year (annual entitlement is a per-year concept
// that can't be shown for a multi-year span). Month mode is always one year.
export function leaveYearForPeriod({ mode, month, from, to }) {
  if (mode === "month") {
    if (!month || typeof month !== "string" || month.length < 4) return null;
    const y = month.slice(0, 4);
    return /^\d{4}$/.test(y) ? y : null;
  }
  const fromYear = String(from || "").slice(0, 4);
  const toYear = String(to || "").slice(0, 4);
  if (!/^\d{4}$/.test(fromYear) || !/^\d{4}$/.test(toYear)) return null;
  return fromYear === toYear ? fromYear : null;
}
