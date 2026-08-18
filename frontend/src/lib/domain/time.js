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
  // Category not found in the caller's rows — e.g. the categories store only
  // ever holds the current user's *active, assigned* categories, so an entry
  // booked before a reassignment or deactivation won't resolve here even
  // though the backend still counts it as work. Default to true, matching
  // categoryCountsAsWork()'s default above: understating someone's logged
  // hours (silently dropping real worked time from the total) is a worse
  // failure than occasionally crediting a non-work category shown as "?".
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
  const minutes = durMin(
    entry.start_time.slice(0, 5),
    entry.end_time.slice(0, 5),
  );
  if (!Number.isFinite(minutes) || minutes < 0) return 0;
  return Math.max(0, minutes);
}

/**
 * Parses an "HH:MM" or "HH:MM:SS" time string into total minutes since midnight.
 * Returns NaN for invalid input, 0 for null/empty.
 */
function parseHHMM(s) {
  if (!s) return 0;
  const parts = String(s).trim().split(":");
  if (parts.length < 2 || parts.length > 3) return NaN;
  const h = Number(parts[0]);
  const m = Number(parts[1]);
  if (!Number.isFinite(h) || !Number.isFinite(m)) return NaN;
  if (h < 0 || h > 23 || m < 0 || m > 59) return NaN;
  if (parts[2] !== undefined) {
    // Entries are minute-granular (the UI only ever offers HH:MM); a
    // trailing ":SS" only shows up because the backend's TIME column
    // serializes with seconds. Validate it's a real time, then discard it —
    // there's no fraction-of-a-minute to add, seconds don't round up here.
    const sec = Number(parts[2]);
    if (!Number.isFinite(sec) || sec < 0 || sec > 59) return NaN;
  }
  return h * 60 + m;
}

function exclusiveThresholdMinutes(thresholdHours) {
  return Math.floor(Number(thresholdHours) * 60 + 1e-9);
}

/**
 * Builds an ordered list of break rules from the app settings object.
 * Returns an empty array when the feature is disabled or no tier-1 rule is configured.
 * Rules are sorted ascending by threshold so callers can find the highest applicable
 * rule by scanning from the end.
 *
 * @param {Object} settings - The app settings from the /settings endpoint.
 * @returns {{thresholdHours: number, thresholdMinutes: number, deductionMinutes: number}[]}
 */
export function buildBreakRules(settings) {
  if (!settings?.auto_break_enabled) return [];
  const rules = [];
  const t1 = Number(settings.auto_break_threshold_hours);
  const d1 = Number(settings.auto_break_deduction_minutes);
  if (Number.isFinite(t1) && t1 > 0 && Number.isFinite(d1) && d1 > 0) {
    rules.push({
      thresholdHours: t1,
      thresholdMinutes: exclusiveThresholdMinutes(t1),
      deductionMinutes: d1,
    });
  }
  const t2 = Number(settings.auto_break_threshold_hours_2);
  const d2 = Number(settings.auto_break_deduction_minutes_2);
  if (Number.isFinite(t2) && t2 > 0 && Number.isFinite(d2) && d2 > 0) {
    // Avoid duplicate thresholds – last wins would be surprising.
    if (!rules.some((r) => r.thresholdHours === t2)) {
      rules.push({
        thresholdHours: t2,
        thresholdMinutes: exclusiveThresholdMinutes(t2),
        deductionMinutes: d2,
      });
    }
  }
  rules.sort((a, b) => a.thresholdHours - b.thresholdHours);
  // Ensure strictly increasing thresholds and positive deductions
  return rules.filter((r, idx) => {
    if (idx === 0) return true;
    return r.thresholdHours > rules[idx - 1].thresholdHours;
  });
}

/**
 * Computes the day's break requirement and how much of it is already covered.
 * Mirrors the backend `compute_day_auto_break` (`backend/src/time_calc.rs`) exactly.
 *
 * Adjacent entries (end time == start time of next) are treated as one continuous
 * work block; overlapping entries are merged too. The day's *total* worked time
 * (summed across all blocks) — not each block independently — is what's tested
 * against the rule tiers, matching German labor law (ArbZG §4: a break is required
 * for a day's work of "mehr als sechs Stunden insgesamt" — more than six hours **in
 * total**). Thresholds are exclusive: a day of exactly 6h00m worked does not trigger
 * the 6-hour rule; only 6h01m or more does. The **highest applicable rule** is
 * selected — rules are not cumulative.
 *
 * There is no separate "break" category in this app — a break is always just
 * unlogged time between entries. Any such real gap within the day's overall span is
 * credited against the requirement; only the shortfall (if any) is deducted. A day
 * with a single continuous block (no gaps) has nothing to credit, so the full
 * requirement is deducted.
 *
 * Applies to all non-rejected entries (including drafts) so the time tracking page
 * shows the expected deduction before entries are approved.
 *
 * @param {Array}  items       - All time entries for the day.
 * @param {Array}  categories  - Full category list for counts-as-work lookup.
 * @param {{thresholdHours: number, thresholdMinutes?: number, deductionMinutes: number}[]} rules
 *   Break rules sorted ascending by thresholdHours.
 * @returns {{
 *   blocks: {start: number, end: number}[],
 *   workedMin: number,
 *   requiredMin: number,
 *   takenMin: number,
 *   deductionMin: number,
 *   appliedRule: {thresholdHours: number, thresholdMinutes?: number, deductionMinutes: number}|null,
 * }}
 */
export function computeDayBreakInfo(items, categories, rules) {
  const empty = {
    blocks: [],
    workedMin: 0,
    requiredMin: 0,
    takenMin: 0,
    deductionMin: 0,
    appliedRule: null,
  };
  if (!items?.length || !rules?.length) return empty;

  // Only non-rejected entries that count as work, sorted by start time.
  // Filter invalid intervals (NaN, end <= start, negative).
  const eligible = items
    .filter((e) => e.status !== "rejected" && entryCountsAsWork(e, categories))
    .map((e) => ({
      start: parseHHMM(e.start_time),
      end: parseHHMM(e.end_time),
    }))
    .filter(
      (r) =>
        Number.isFinite(r.start) &&
        Number.isFinite(r.end) &&
        r.end > r.start &&
        r.start >= 0 &&
        r.end <= 24 * 60,
    )
    .sort((a, b) => a.start - b.start);

  if (!eligible.length) return empty;

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

  if (!blocks.length) return empty;

  // Day total worked time, summed across all blocks — this is what ArbZG §4 tests
  // against, not each block's own duration.
  const workedMin = blocks.reduce((sum, b) => sum + (b.end - b.start), 0);

  // Wall-clock span from the first entry's start to the last entry's end, minus the
  // worked time, is the total real rest time already taken between blocks today.
  const spanMin = blocks[blocks.length - 1].end - blocks[0].start;
  const takenMin = Math.max(0, spanMin - workedMin);
  if (!Number.isFinite(takenMin) || !Number.isFinite(workedMin)) return empty;

  // Highest applicable rule wins; stays null when no rule threshold is strictly
  // exceeded by the day's total worked time. Rules are sorted ascending, so the
  // last match encountered is the highest one.
  let appliedRule = null;
  for (const rule of rules) {
    const thresholdMinutes = Number.isFinite(rule.thresholdMinutes)
      ? rule.thresholdMinutes
      : exclusiveThresholdMinutes(rule.thresholdHours);
    if (workedMin > thresholdMinutes) {
      appliedRule = rule;
    }
  }
  const requiredMin = appliedRule?.deductionMinutes ?? 0;
  const deductionMin = Math.max(0, requiredMin - takenMin);

  return {
    blocks,
    workedMin,
    requiredMin,
    takenMin,
    deductionMin,
    appliedRule,
  };
}

/**
 * Total automatic break deduction in minutes for all entries on a single day.
 * Thin wrapper around `computeDayBreakInfo` for callers that only need the number.
 *
 * @returns {number} Total break deduction in minutes (>= 0).
 */
export function computeDayBreakDeduction(items, categories, rules) {
  return computeDayBreakInfo(items, categories, rules).deductionMin;
}

// Look up the absence category by slug from the store. Any caller that already
// has access to the categories array should pass it in to avoid the store read.
// When store is empty (not loaded yet) we default to non-blocking to avoid
// false UI disabling.
function categoryFor(kind) {
  const cats = get(absenceCategories);
  if (!cats?.length) return null;
  return cats.find((c) => c.slug === kind) || null;
}

export function absenceRemovesTarget(absence) {
  if (!absence) return false;
  if (!TARGET_REMOVING_ABSENCE_STATUSES.includes(absence.status)) return false;
  const cat = categoryFor(absence.kind);
  if (!cat) return false; // store not loaded → don't remove target
  return cat.cost_type !== "flextime";
}

export function absenceBlocksEntry(absence) {
  if (!absence) return false;
  if (!ENTRY_BLOCKING_ABSENCE_STATUSES.includes(absence.status)) return false;
  const cat = categoryFor(absence.kind);
  if (!cat) return false; // store not loaded → don't block entry
  return cat.auto_approve_past !== true;
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

function potentialWorkdaysPerWeek(workdaysPerWeek) {
  const configured = Number(workdaysPerWeek || 0);
  if (!Number.isFinite(configured) || configured <= 0) return 0;
  if (configured <= 5) return 5;
  if (configured === 6) return 6;
  return 7;
}

function isPotentialDay(dayName, workdaysPerWeek) {
  const index = WEEKDAY_NAMES.indexOf(dayName);
  if (index < 0) return false;
  return index < potentialWorkdaysPerWeek(workdaysPerWeek);
}

export function weekTargetMinutes({
  weekdays,
  weekendDays,
  currentUser,
  todayIso,
}) {
  const weeklyHours = Number(currentUser?.weekly_hours || 0);
  const workdaysPerWeek = Number(currentUser?.workdays_per_week || 5);
  const potentialDays = potentialWorkdaysPerWeek(workdaysPerWeek);
  if (potentialDays <= 0 || workdaysPerWeek <= 0) return 0;
  // Daily target is weekly_hours / workdays_per_week for 1-5 day schedules
  // (so 8h for 1-day at 8h weekly), not weekly / potential (which would be 1.6h).
  const divisor =
    workdaysPerWeek >= 1 && workdaysPerWeek <= 5
      ? workdaysPerWeek
      : potentialDays;
  const perDayMinutes = (weeklyHours / divisor) * 60;
  if (!Number.isFinite(perDayMinutes) || perDayMinutes <= 0) return 0;
  const eligibleDays = [...(weekdays || []), ...(weekendDays || [])]
    .filter((day) => isPotentialDay(day.dayName, workdaysPerWeek))
    .filter((day) => {
      const isBeforeStart =
        currentUser?.start_date && day.ds < currentUser.start_date;
      const isFuture = day.ds > todayIso;
      return !(day.absentForTarget || day.holiday || isBeforeStart || isFuture);
    });

  // Use floor to avoid systematic +1 minute per day accumulation from rounding.
  const eligibleCount = Math.min(eligibleDays.length, workdaysPerWeek);
  return Math.floor(eligibleCount * perDayMinutes);
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

function isResolvedRejectedEntry(entry) {
  return entry?.status === "rejected" && !!entry.rejection_resolved_at;
}

export function workflowRelevantEntries(entries) {
  return (entries || []).filter((entry) => !isResolvedRejectedEntry(entry));
}

export function reopenableWeekEntries(entries) {
  return workflowRelevantEntries(entries).filter((entry) =>
    ["submitted", "approved", "rejected"].includes(entry.status),
  );
}

export function weekStatus(entries, drafts) {
  const relevantEntries = workflowRelevantEntries(entries);
  if (!relevantEntries.length) return "draft";
  const relevantDrafts = (drafts || []).filter((draft) =>
    relevantEntries.some((entry) => entry.id === draft.id),
  );
  const nonDraftEntries = relevantEntries.filter(
    (entry) => entry.status !== "draft",
  );
  if (relevantDrafts.length > 0) {
    return nonDraftEntries.length > 0 ? "partial" : "draft";
  }
  if (nonDraftEntries.length === 0) return "draft";
  if (
    nonDraftEntries.length === relevantEntries.length &&
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
  return (
    get(absenceCategories).find((c) => c.slug === kind)?.color ||
    MASKED_ABSENCE_COLOR
  );
}

export function canAddEntryForDay(day, currentUser, todayIso) {
  // Public holidays are not blocked: like the sick-leave exception, someone
  // may still work (or be on call) on a holiday. The daily target stays 0,
  // so logged hours become a pure flextime gain, matching backend validation.
  return !(
    day.absentForEntry ||
    day.ds > todayIso ||
    (currentUser?.start_date && day.ds < currentUser.start_date)
  );
}
