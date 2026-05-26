import {
  addDays,
  dateKey,
  durMin,
  fmtDateShort,
  isoDate,
  monday,
  parseDate,
} from "../../format.js";
import { absenceKindLabel } from "../../i18n.js";
import { sortByIsoDateAndStartTime } from "./dates.js";
import { entryCountsAsWork } from "./time.js";
import { userNameFromRows } from "./users.js";

function monthKey(year, month) {
  return `${year}-${String(month).padStart(2, "0")}`;
}

export function monthFullySubmitted(report) {
  return report?.weeks_all_submitted === true;
}

export function allMonthsToCheck(userStart, today) {
  if (!userStart) return [];
  const startYear = parseInt(userStart.slice(0, 4), 10);
  const startMonth = parseInt(userStart.slice(5, 7), 10);
  const endYear = today.getFullYear();
  const endMonth = today.getMonth() + 1;
  if (startYear > endYear || (startYear === endYear && startMonth > endMonth)) {
    return [];
  }
  const months = [];
  for (let year = startYear; year <= endYear; year++) {
    const fromMonth = year === startYear ? startMonth : 1;
    const toMonth = year === endYear ? endMonth : 12;
    for (let month = fromMonth; month <= toMonth; month++) {
      months.push(monthKey(year, month));
    }
  }
  return months;
}

const OPEN_WEEK_STATUSES = new Set(["draft", "partial", "rejected"]);

export function buildSubmissionChecks(months, reports) {
  return months.map((month, index) => ({
    month,
    submitted: monthFullySubmitted(reports[index]),
    approved: reports[index]?.weeks_all_approved === true,
    currentWeekStatus: reports[index]?.current_week_status ?? null,
  }));
}

export function currentWeekIsOpen(checks) {
  return (checks || []).some((c) => OPEN_WEEK_STATUSES.has(c.currentWeekStatus));
}

export function entryMinutes(entry, categories = []) {
  if (
    !entry?.start_time ||
    !entry?.end_time ||
    !entryCountsAsWork(entry, categories)
  ) {
    return 0;
  }
  const start = entry.start_time.slice(0, 5);
  const end = entry.end_time.slice(0, 5);
  return Math.max(0, durMin(start, end));
}

export function weekStartOf(entryDate) {
  const day = dateKey(entryDate);
  if (!day) return "";
  return isoDate(monday(parseDate(day)));
}

export function buildPendingWeeks(submittedEntries, userRows, categories = []) {
  const weekGroupsByKey = new Map();
  for (const entry of submittedEntries || []) {
    const weekStart = weekStartOf(entry.entry_date);
    if (!weekStart) continue;
    const key = `${entry.user_id}:${weekStart}`;
    const existing = weekGroupsByKey.get(key) || {
      key,
      user_id: entry.user_id,
      week_start: weekStart,
      week_end: isoDate(addDays(parseDate(weekStart), 6)),
      entries: [],
      total_min: 0,
    };
    existing.entries.push(entry);
    existing.total_min += entryMinutes(entry, categories);
    weekGroupsByKey.set(key, existing);
  }

  const sortedWeekGroups = Array.from(weekGroupsByKey.values()).map(
    (group) => ({
      ...group,
      entries: sortByIsoDateAndStartTime(group.entries),
    }),
  );

  sortedWeekGroups.sort((a, b) => {
    const weekDiff = b.week_start.localeCompare(a.week_start);
    if (weekDiff !== 0) return weekDiff;
    return userNameFromRows(a.user_id, userRows).localeCompare(
      userNameFromRows(b.user_id, userRows),
    );
  });

  return sortedWeekGroups;
}

export function absenceDiffRows(absence, translate) {
  if (absence.review_type !== "change") return [];
  const rows = [];
  if (absence.previous_kind && absence.previous_kind !== absence.kind) {
    rows.push({
      field: translate("Type"),
      before: absenceKindLabel(absence.previous_kind),
      after: absenceKindLabel(absence.kind),
    });
  }
  if (
    absence.previous_start_date &&
    absence.previous_start_date !== absence.start_date
  ) {
    rows.push({
      field: translate("From"),
      before: fmtDateShort(absence.previous_start_date),
      after: fmtDateShort(absence.start_date),
    });
  }
  if (
    absence.previous_end_date &&
    absence.previous_end_date !== absence.end_date
  ) {
    rows.push({
      field: translate("To"),
      before: fmtDateShort(absence.previous_end_date),
      after: fmtDateShort(absence.end_date),
    });
  }
  if ((absence.previous_comment || "") !== (absence.comment || "")) {
    rows.push({
      field: translate("Comment"),
      before: absence.previous_comment || translate("Empty"),
      after: absence.comment || translate("Empty"),
    });
  }
  return rows;
}

export function absenceRequestTypeLabelKey(absence) {
  if (
    absence.status === "cancellation_pending" ||
    absence.review_type === "cancellation"
  ) {
    return "Cancellation";
  }
  if (absence.review_type === "change") return "Change";
  return "Approval";
}

export function notificationTarget(notification, now = Date.now()) {
  const query = `n=${notification.id}-${now}`;
  if (
    notification.kind === "timesheet_submitted" ||
    notification.reference_type === "time_entries"
  ) {
    return `/dashboard?focus=timesheets&${query}`;
  }
  if (
    notification.kind === "reopen_request_created" ||
    notification.reference_type === "reopen_request"
  ) {
    return `/dashboard?focus=reopen&${query}`;
  }
  if (
    notification.kind === "absence_requested" ||
    notification.reference_type === "absences"
  ) {
    return `/dashboard?focus=absences&${query}`;
  }
  if (notification.kind === "submission_reminder") {
    return `/dashboard?${query}`;
  }
  return "";
}
