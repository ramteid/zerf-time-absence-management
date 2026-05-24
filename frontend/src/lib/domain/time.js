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
  return categoryCountsAsWork(entry?.category_id, categoryRows);
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
