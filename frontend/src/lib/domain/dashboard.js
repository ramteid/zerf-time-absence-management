import {
  addDays,
  dateKey,
  fmtDateShort,
  isoDate,
  monday,
  parseDate,
} from "../../format.js";
import { absenceKindLabel } from "../../i18n.js";
import { sortByIsoDateAndStartTime } from "./dates.js";
import { computeDayBreakDeduction, creditedEntryMinutes } from "./time.js";
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
  return (checks || []).some((c) =>
    OPEN_WEEK_STATUSES.has(c.currentWeekStatus),
  );
}

// Credited minutes of one entry. Delegates to the shared helper so a week
// total and the break deduction applied to it always agree on which entries
// exist: this used to count rejected entries, which `computeDayBreakDeduction`
// ignores, so a rejected row inflated the week shown to the approver.
export function entryMinutes(entry, categories = []) {
  return creditedEntryMinutes(entry, categories);
}

export function weekStartOf(entryDate) {
  const day = dateKey(entryDate);
  if (!day) return "";
  return isoDate(monday(parseDate(day)));
}

// Credited minutes of a day's entries after the automatic break the day as a
// whole attracts. The break is a property of the day, never of a single entry,
// which is why it can only be worked out from the full set.
function creditedDayMinutes(dayEntries, categories, breakRules) {
  const credited = dayEntries.reduce(
    (sum, entry) => sum + entryMinutes(entry, categories),
    0,
  );
  if (!breakRules.length) return credited;
  return credited - computeDayBreakDeduction(dayEntries, categories, breakRules);
}

export function buildPendingWeeks(
  submittedEntries,
  userRows,
  categories = [],
  breakRules = [],
  approvedEntries = [],
) {
  // Already-approved hours on the same day, keyed by person and date. They are
  // not part of the week being decided, but they decide how much break that day
  // attracts — see `creditedDayMinutes`.
  const approvedByUserDay = new Map();
  for (const entry of approvedEntries || []) {
    const day = dateKey(entry.entry_date);
    if (!day) continue;
    const key = `${entry.user_id}:${day}`;
    if (!approvedByUserDay.has(key)) approvedByUserDay.set(key, []);
    approvedByUserDay.get(key).push(entry);
  }

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
    weekGroupsByKey.set(key, existing);
  }

  // What approving this week actually adds: for every day it touches, the
  // credited minutes of the whole day minus what that day already credits
  // today. Counting the submitted entries on their own instead gets the break
  // wrong whenever the day already carries approved hours — four submitted
  // hours added to five approved ones push the day past the six-hour tier, so
  // they credit three and a half, not four.
  for (const group of weekGroupsByKey.values()) {
    const byDate = new Map();
    for (const entry of group.entries) {
      const day = dateKey(entry.entry_date) || String(entry.entry_date);
      if (!day) continue;
      if (!byDate.has(day)) byDate.set(day, []);
      byDate.get(day).push(entry);
    }
    let total = 0;
    for (const [day, dayEntries] of byDate) {
      const alreadyApproved =
        approvedByUserDay.get(`${group.user_id}:${day}`) || [];
      total +=
        creditedDayMinutes(
          [...alreadyApproved, ...dayEntries],
          categories,
          breakRules,
        ) - creditedDayMinutes(alreadyApproved, categories, breakRules);
    }
    // A submission can in principle credit less than nothing — a couple of
    // minutes that tip the day over a break tier cost more than they add — but
    // a negative figure on an approval card reads as an error rather than as
    // the edge case it is.
    group.total_min = Math.max(0, total);
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
    // Pass the stored display names as fallbacks so deactivated categories
    // (missing from the active-only frontend store) still render with their
    // real name instead of the raw slug. `category_name` / `previous_category_name`
    // are projected from the `absence_categories` join on the backend.
    rows.push({
      field: translate("Type"),
      before: absenceKindLabel(
        absence.previous_kind,
        absence.previous_category_name,
      ),
      after: absenceKindLabel(absence.kind, absence.category_name),
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
    notification.kind === "timesheet_approved" ||
    notification.kind === "timesheet_rejected" ||
    notification.reference_type === "time_entries"
  ) {
    return `/dashboard?focus=timesheets&${query}`;
  }
  if (
    notification.kind === "reopen_request_created" ||
    notification.kind === "reopen_request_approved" ||
    notification.kind === "reopen_request_rejected" ||
    notification.reference_type === "reopen_request" ||
    notification.reference_type === "reopen_requests"
  ) {
    return `/dashboard?focus=reopen&${query}`;
  }
  if (
    notification.kind === "absence_requested" ||
    notification.kind === "absence_approved" ||
    notification.kind === "absence_rejected" ||
    notification.kind === "absence_cancelled" ||
    notification.kind === "absence_updated" ||
    notification.reference_type === "absences"
  ) {
    return `/dashboard?focus=absences&${query}`;
  }
  if (
    notification.kind === "submission_reminder" ||
    notification.kind === "approval_reminder" ||
    notification.kind === "month_end_submission_reminder" ||
    notification.kind === "month_end_approval_reminder" ||
    notification.kind === "payroll_report_blocked" ||
    notification.kind === "system_error"
  ) {
    return `/dashboard?${query}`;
  }
  return "/dashboard";
}
