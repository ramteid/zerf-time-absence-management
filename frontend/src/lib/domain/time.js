import {
  addDays,
  dateKey,
  durMin,
  formatTimeValue,
  isoDate,
} from "../../format.js";
import { ABSENCE_COLORS } from "../../colors.js";

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
 * Computes the total automatic break deduction in minutes for all entries on a
 * single day. Mirrors the backend `compute_day_auto_break` Rust function exactly.
 *
 * Adjacent entries (end time == start time of next) are treated as one
 * continuous work block. Even a one-minute gap breaks continuity. Overlapping
 * entries are merged. Each block whose duration meets or exceeds
 * `thresholdHours * 60` minutes triggers exactly one `deductionMinutes`
 * deduction. Only non-rejected, counts-as-work entries are considered.
 *
 * This function applies to all non-rejected entries (including drafts) so that
 * the daily-total display on the time tracking page reflects the expected
 * deduction before entries are approved.
 *
 * @param {Array}  items            - All time entries for the day.
 * @param {Array}  categories       - Full category list for counts-as-work lookup.
 * @param {number} thresholdHours   - Minimum consecutive crediting hours before deduction.
 * @param {number} deductionMinutes - Minutes deducted per qualifying work block.
 * @returns {number} Total break deduction in minutes (>= 0).
 */
export function computeDayBreakDeduction(
  items,
  categories,
  thresholdHours,
  deductionMinutes,
) {
  if (!items?.length || !thresholdHours || !deductionMinutes) return 0;
  const thresholdMin = thresholdHours * 60;

  // Only non-rejected entries whose category counts as work, sorted by start time.
  const eligible = items
    .filter((e) => e.status !== "rejected" && entryCountsAsWork(e, categories))
    .map((e) => ({
      start: parseHHMM(e.start_time),
      end: parseHHMM(e.end_time),
    }))
    .sort((a, b) => a.start - b.start);

  if (!eligible.length) return 0;

  // Merge adjacent (start == last.end) and overlapping intervals into
  // continuous work blocks, matching the backend merge rule precisely.
  const blocks = [];
  for (const { start, end } of eligible) {
    const last = blocks[blocks.length - 1];
    if (last && start <= last.end) {
      // Adjacent or overlapping: extend the current block's end if needed.
      if (end > last.end) last.end = end;
    } else {
      blocks.push({ start, end });
    }
  }

  // One deduction per block that meets or exceeds the threshold.
  return (
    blocks.filter((b) => b.end - b.start >= thresholdMin).length *
    deductionMinutes
  );
}

export function absenceRemovesTarget(absence) {
  return absence
    ? TARGET_REMOVING_ABSENCE_STATUSES.includes(absence.status) &&
        absence.kind !== "flextime_reduction"
    : false;
}

export function absenceBlocksEntry(absence) {
  return absence
    ? TARGET_REMOVING_ABSENCE_STATUSES.includes(absence.status) &&
        absence.kind !== "sick"
    : false;
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
  return ABSENCE_COLORS[kind] || "var(--text-tertiary)";
}

export function canAddEntryForDay(day, currentUser, todayIso) {
  return !(
    day.absentForEntry ||
    day.holiday ||
    day.ds > todayIso ||
    (currentUser?.start_date && day.ds < currentUser.start_date)
  );
}
