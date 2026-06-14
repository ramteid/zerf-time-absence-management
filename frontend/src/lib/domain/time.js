import {
  addDays,
  dateKey,
  durMin,
  formatTimeValue,
  isoDate,
} from "../../format.js";
import { get } from "svelte/store";
import { absenceCategories } from "../../stores.js";
import { MASKED_ABSENCE_COLOR } from "../../colors.js";

export const WEEKDAY_NAMES = Object.freeze([
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
  "Sunday",
]);

const TARGET_REMOVING_ABSENCE_STATUSES = ["approved", "cancellation_pending"];

// A requested (pending) non-sick absence also blocks entry creation: once a user
// has submitted an absence request, logging time on that day would make approval
// impossible (ensure_no_time_conflict_tx rejects it on the backend).
const ENTRY_BLOCKING_ABSENCE_STATUSES = [
  ...TARGET_REMOVING_ABSENCE_STATUSES,
  "requested",
];

export function categoryById(categoryId, categoryRows) {
  return (
    (categoryRows || []).find((category) => category.id === categoryId) || {
      name: "?",
      color: "#999",
    }
  );
}

export function categoryCountsAsWork(categoryId, categoryRows) {
  const category = (categoryRows || []).find((item) => item.id === categoryId);
  return category?.counts_as_work !== false;
}

export function entryCountsAsWork(entry, categoryRows) {
  if (entry?.counts_as_work === false) return false;
  if (entry?.counts_as_work === true) return true;
  // Look up by id first (the common case for server entries).
  if (entry?.category_id != null) {
    const byId = (categoryRows || []).find((c) => c.id === entry.category_id);
    if (byId) return byId.counts_as_work !== false;
  }
  // Fall back to name-based lookup (used by dashboard pending-week entries
  // that carry a `category` string field instead of a numeric id).
  if (entry?.category) {
    const byName = (categoryRows || []).find((c) => c.name === entry.category);
    if (byName) return byName.counts_as_work !== false;
  }
  return true;
}

export function creditedEntryMinutes(entry, categoryRows) {
  if (
    !entry?.start_time ||
    !entry?.end_time ||
    entry.status === "rejected" ||
    !entryCountsAsWork(entry, categoryRows)
  ) {
    return 0;
  }
  return Math.max(0, durMin(entry.start_time.slice(0, 5), entry.end_time.slice(0, 5)));
}

/**
 * Parses an "HH:MM" or "HH:MM:SS" time string into total minutes since midnight.
 * Returns 0 for null/undefined/empty input.
 */
function parseHHMM(s) {
  if (!s) return 0;
  const parts = s.split(":");
  return parseInt(parts[0], 10) * 60 + parseInt(parts[1] || "0", 10);
}

/**
 * Builds an ordered list of break rules from the app settings object.
 * Returns an empty array when the feature is disabled or no tier-1 rule is configured.
 * Rules are sorted ascending by threshold so callers can find the highest applicable
 * rule by scanning from the end.
 *
 * @param {Object} settings - The app settings from the /settings endpoint.
 * @returns {{thresholdHours: number, deductionMinutes: number}[]}
 */
export function buildBreakRules(settings) {
  if (!settings?.auto_break_enabled) return [];
  const rules = [];
  if (settings.auto_break_threshold_hours && settings.auto_break_deduction_minutes) {
    rules.push({
      thresholdHours: Number(settings.auto_break_threshold_hours),
      deductionMinutes: Number(settings.auto_break_deduction_minutes),
    });
  }
  if (settings.auto_break_threshold_hours_2 && settings.auto_break_deduction_minutes_2) {
    rules.push({
      thresholdHours: Number(settings.auto_break_threshold_hours_2),
      deductionMinutes: Number(settings.auto_break_deduction_minutes_2),
    });
  }
  rules.sort((a, b) => a.thresholdHours - b.thresholdHours);
  return rules;
}

/**
 * Computes the total automatic break deduction in minutes for all entries on a
 * single day. Mirrors the backend `compute_day_auto_break` Rust function exactly.
 *
 * Adjacent entries (end time == start time of next) are treated as one continuous
 * work block. Even a one-minute gap breaks continuity. Overlapping entries are merged.
 *
 * For each block, the **highest applicable rule** is selected and its deduction applied
 * exactly once — rules are not cumulative. A 10-hour block with rules [(6h→30min),
 * (9h→45min)] deducts 45 min, not 75 min.
 *
 * Applies to all non-rejected entries (including drafts) so the time tracking page
 * shows the expected deduction before entries are approved.
 *
 * @param {Array}  items       - All time entries for the day.
 * @param {Array}  categories  - Full category list for counts-as-work lookup.
 * @param {{thresholdHours: number, deductionMinutes: number}[]} rules
 *   Break rules sorted ascending by thresholdHours.
 * @returns {number} Total break deduction in minutes (>= 0).
 */
export function computeDayBreakDeduction(items, categories, rules) {
  if (!items?.length || !rules?.length) return 0;

  // Only non-rejected entries that count as work, sorted by start time.
  const eligible = items
    .filter((e) => e.status !== "rejected" && entryCountsAsWork(e, categories))
    .map((e) => ({
      start: parseHHMM(e.start_time),
      end: parseHHMM(e.end_time),
    }))
    .sort((a, b) => a.start - b.start);

  if (!eligible.length) return 0;

  // Merge adjacent (start == last.end) and overlapping intervals into continuous blocks.
  const blocks = [];
  for (const { start, end } of eligible) {
    const last = blocks[blocks.length - 1];
    if (last && start <= last.end) {
      if (end > last.end) last.end = end;
    } else {
      blocks.push({ start, end });
    }
  }

  // For each block: highest applicable rule wins (not cumulative).
  return blocks.reduce((total, block) => {
    const duration = block.end - block.start;
    const deduction = rules
      .filter((r) => duration >= r.thresholdHours * 60)
      .reduce((max, r) => Math.max(max, r.deductionMinutes), 0);
    return total + deduction;
  }, 0);
}

// Look up the absence category by slug from the store. Any caller that already
// has access to the categories array should pass it in to avoid the store read.
function categoryFor(kind) {
  return get(absenceCategories).find((c) => c.slug === kind);
}

export function absenceRemovesTarget(absence) {
  if (!absence) return false;
  if (!TARGET_REMOVING_ABSENCE_STATUSES.includes(absence.status)) return false;
  // keeps_work_target categories (e.g. flextime reduction) preserve the day's
  // work target — the absence "costs" flextime rather than removing the target.
  return categoryFor(absence.kind)?.keeps_work_target !== true;
}

export function absenceBlocksEntry(absence) {
  if (!absence) return false;
  if (!ENTRY_BLOCKING_ABSENCE_STATUSES.includes(absence.status)) return false;
  // auto_approve_past categories (sick-like) coexist with logged time on the
  // same day, so they must NOT block entry creation.
  return categoryFor(absence.kind)?.auto_approve_past !== true;
}

export function filterWeekAbsences(absenceRowsByYear, from, to) {
  const seenAbsenceIds = new Set();
  return (absenceRowsByYear || []).flat().filter((absence) => {
    if (seenAbsenceIds.has(absence.id)) return false;
    seenAbsenceIds.add(absence.id);
    return (
      absence.end_date >= from &&
      absence.start_date <= to &&
      absence.status !== "rejected" &&
      absence.status !== "cancelled"
    );
  });
}

export function buildWeekDay(
  dayIndex,
  weekFrom,
  entryRows,
  absenceRows,
  holidayRows,
) {
  const dayDate = addDays(weekFrom, dayIndex);
  const dayDateStr = isoDate(dayDate);
  const matchingAbsence = (absenceRows || []).find(
    (absence) =>
      absence.start_date <= dayDateStr && absence.end_date >= dayDateStr,
  );
  const matchingHoliday = (holidayRows || []).find(
    (holiday) => holiday.holiday_date === dayDateStr,
  );
  return {
    d: dayDate,
    ds: dayDateStr,
    dayName: WEEKDAY_NAMES[dayIndex],
    absent: !!matchingAbsence,
    absentForEntry: absenceBlocksEntry(matchingAbsence),
    absentForTarget: absenceRemovesTarget(matchingAbsence),
    holiday: !!matchingHoliday,
    absenceKind: matchingAbsence?.kind || null,
    holidayName: matchingHoliday?.name || null,
    items: (entryRows || [])
      .filter((entry) => dateKey(entry.entry_date) === dayDateStr)
      .sort((a, b) => String(a.start_time).localeCompare(String(b.start_time))),
  };
}

export function buildWeekDays(weekFrom, entries, absences, holidays) {
  if (!weekFrom) return { weekdays: [], weekendDays: [] };
  return {
    weekdays: Array.from({ length: 5 }, (_, dayIndex) =>
      buildWeekDay(dayIndex, weekFrom, entries, absences, holidays),
    ),
    weekendDays: Array.from({ length: 2 }, (_, index) =>
      buildWeekDay(5 + index, weekFrom, entries, absences, holidays),
    ),
  };
}

export function weekTargetMinutes({
  weekdays,
  weekendDays,
  currentUser,
  todayIso,
}) {
  const weeklyHours = Number(currentUser?.weekly_hours || 0);
  const workdaysPerWeek = Number(currentUser?.workdays_per_week || 5);
  const perDayMinutes = Math.round((weeklyHours / workdaysPerWeek) * 60);
  if (perDayMinutes <= 0) return 0;
  return [...(weekdays || []), ...(weekendDays || [])]
    .slice(0, workdaysPerWeek)
    .reduce((totalMinutes, day) => {
      const isBeforeStart =
        currentUser?.start_date && day.ds < currentUser.start_date;
      const isFuture = day.ds > todayIso;
      if (day.absentForTarget || day.holiday || isBeforeStart || isFuture) {
        return totalMinutes;
      }
      return totalMinutes + perDayMinutes;
    }, 0);
}

export function entryDurationHours(startTime, endTime) {
  return durMin(startTime, endTime) / 60;
}

export function formatDisplayTime(rawTimeValue, timeFormat) {
  return formatTimeValue(rawTimeValue?.slice(0, 5) || "", timeFormat);
}

export function entryTimeRange(entry, timeFormat) {
  return `${formatDisplayTime(entry.start_time, timeFormat)} - ${formatDisplayTime(
    entry.end_time,
    timeFormat,
  )}`;
}

export function weekStatus(entries, drafts) {
  if (!entries?.length) return "draft";
  const nonDraftEntries = entries.filter((entry) => entry.status !== "draft");
  if (drafts.length > 0) {
    return nonDraftEntries.length > 0 ? "partial" : "draft";
  }
  if (nonDraftEntries.length === 0) return "draft";
  if (
    nonDraftEntries.length === entries.length &&
    nonDraftEntries.every((entry) => entry.status === "approved")
  ) {
    return "approved";
  }
  if (nonDraftEntries.some((entry) => entry.status === "submitted")) {
    return "submitted";
  }
  if (nonDraftEntries.every((entry) => entry.status === "rejected")) {
    return "rejected";
  }
  return "partial";
}

export function weekStatusColor(status) {
  switch (status) {
    case "draft":
      return "var(--danger-text)";
    case "submitted":
    case "partial":
      return "var(--warning-text)";
    case "approved":
      return "var(--success-text)";
    case "rejected":
      return "var(--danger-text)";
    default:
      return "var(--text-primary)";
  }
}

export function absenceColor(kind) {
  return get(absenceCategories).find((c) => c.slug === kind)?.color || MASKED_ABSENCE_COLOR;
}

export function canAddEntryForDay(day, currentUser, todayIso) {
  return !(
    day.absentForEntry ||
    day.holiday ||
    day.ds > todayIso ||
    (currentUser?.start_date && day.ds < currentUser.start_date)
  );
}
