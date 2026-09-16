function parseIsoDate(value) {
  if (!value) return new Date(NaN);
  const parts = String(value).trim().split("-").map(Number);
  if (parts.length < 3 || parts.some((n) => !Number.isFinite(n))) {
    return new Date(NaN);
  }
  const [year, month, day] = parts;
  if (month < 1 || month > 12 || day < 1 || day > 31) return new Date(NaN);
  return new Date(year, month - 1, day, 12, 0, 0, 0);
}

function addCalendarDays(value, days) {
  const next = new Date(value);
  next.setDate(next.getDate() + days);
  return next;
}

function formatIsoDate(value) {
  return [
    value.getFullYear(),
    String(value.getMonth() + 1).padStart(2, "0"),
    String(value.getDate()).padStart(2, "0"),
  ].join("-");
}

function potentialWorkdaysPerWeek(workdaysPerWeek = 5) {
  const value = Number(workdaysPerWeek);
  if (!Number.isFinite(value) || value <= 0) return 0;
  if (value <= 5) return 5;
  if (value === 6) return 6;
  return 7;
}

function weekMonday(value) {
  const current = new Date(value);
  const isoWeekday = (current.getDay() + 6) % 7;
  current.setDate(current.getDate() - isoWeekday);
  current.setHours(0, 0, 0, 0);
  return current;
}

function isPotentialWorkday(value, workdaysPerWeek = 5) {
  const isoWeekday = (value.getDay() + 6) % 7;
  const potential = potentialWorkdaysPerWeek(workdaysPerWeek);
  return isoWeekday < potential;
}

export function holidayDateSet(holidays = []) {
  return new Set(holidays.map((holiday) => holiday.holiday_date));
}

/**
 * The days a set of absence ranges actually costs inside a window, as ISO date
 * strings. Mirrors the backend `time_calc::counted_workdays`, and returns the
 * days rather than just their number for the same reason it does: the weekly
 * quota has to be applied ONCE across the union of every range, with the
 * per-absence or per-bucket split cut out of the result afterwards. Counting
 * each range on its own re-applies the quota to each of them, so two bookings
 * sharing one calendar week bill a part-timer more days than a week can ever
 * cost.
 *
 * A schedule with no potential workday pool (irregular) counts every calendar
 * day that is not a public holiday, uncapped — the backend's irregular branch.
 */
export function countedWorkdays(
  ranges,
  windowStart,
  windowEnd,
  holidays = new Set(),
  workdaysPerWeek = 5,
) {
  const start = parseIsoDate(windowStart);
  const end = parseIsoDate(windowEnd);
  const configuredDays = Number(workdaysPerWeek);
  if (
    Number.isNaN(start.getTime()) ||
    Number.isNaN(end.getTime()) ||
    end < start ||
    !Number.isFinite(configuredDays)
  ) {
    return [];
  }

  // Clamp every range to the window; one that falls outside it entirely (or is
  // inverted) contributes nothing.
  const clamped = [];
  for (const range of ranges || []) {
    const rangeStart = parseIsoDate(range?.[0]);
    const rangeEnd = parseIsoDate(range?.[1]);
    if (Number.isNaN(rangeStart.getTime()) || Number.isNaN(rangeEnd.getTime())) {
      continue;
    }
    const from = rangeStart > start ? rangeStart : start;
    const to = rangeEnd < end ? rangeEnd : end;
    if (from <= to) clamped.push([formatIsoDate(from), formatIsoDate(to)]);
  }
  if (clamped.length === 0) return [];

  const irregular = potentialWorkdaysPerWeek(configuredDays) === 0;
  const counted = [];
  const usedInWeek = new Map();
  for (
    let current = new Date(start);
    current <= end;
    current = addCalendarDays(current, 1)
  ) {
    const currentDate = formatIsoDate(current);
    const isCandidate =
      !holidays.has(currentDate) &&
      (irregular || isPotentialWorkday(current, configuredDays));
    if (!isCandidate) continue;
    if (!clamped.some(([from, to]) => currentDate >= from && currentDate <= to)) {
      continue;
    }
    if (irregular) {
      counted.push(currentDate);
      continue;
    }
    const weekKey = formatIsoDate(weekMonday(current));
    const used = usedInWeek.get(weekKey) || 0;
    if (used < configuredDays) {
      usedInWeek.set(weekKey, used + 1);
      counted.push(currentDate);
    }
  }
  return counted;
}

/**
 * Effective workdays in one date range. The single-range case of
 * `countedWorkdays`, which is the only implementation of this calendar —
 * exactly as the backend's `count_workdays` delegates to `counted_workdays`.
 */
export function countWorkdays(
  startDate,
  endDate,
  holidays = new Set(),
  workdaysPerWeek = 5,
) {
  return countedWorkdays(
    [[startDate, endDate]],
    startDate,
    endDate,
    holidays,
    workdaysPerWeek,
  ).length;
}

/**
 * Attach a `days` count to every absence, charging one calendar week's quota
 * once across all of them instead of once per absence.
 *
 * Days are attributed in chronological order, so the first booking in a week
 * keeps its own cost and a later one in the same week is charged only what it
 * adds. That is the same rule the backend prices a new request by, and it is
 * what makes these rows add up to the leave balance shown beside them —
 * counting each absence alone reported a part-timer's split week as two full
 * weeks of leave.
 */
export function withAbsenceDays(
  absences,
  { from, to, holidays = new Set(), workdaysPerWeek = 5 },
) {
  const rows = absences || [];
  if (rows.length === 0) return [];
  const counted = countedWorkdays(
    rows.map((absence) => [absence.start_date, absence.end_date]),
    from,
    to,
    holidays,
    workdaysPerWeek,
  );
  // Stable chronological order; the id breaks ties so two absences starting on
  // the same day are always split the same way.
  const order = rows
    .map((_, index) => index)
    .sort((a, b) => {
      const byStart = String(rows[a].start_date).localeCompare(
        String(rows[b].start_date),
      );
      if (byStart !== 0) return byStart;
      return (rows[a].id ?? 0) - (rows[b].id ?? 0);
    });
  const dayCounts = new Array(rows.length).fill(0);
  for (const day of counted) {
    const owner = order.find(
      (index) => day >= rows[index].start_date && day <= rows[index].end_date,
    );
    if (owner !== undefined) dayCounts[owner] += 1;
  }
  return rows.map((absence, index) => ({ ...absence, days: dayCounts[index] }));
}

export function normalizeMonthReport(report, workdaysPerWeek = 5) {
  if (!report || !Array.isArray(report.days)) {
    return report;
  }

  const entries = [];
  const absenceRuns = [];
  let activeAbsence = null;
  const holidaySet = new Set(
    report.days.filter((day) => !!day.holiday).map((day) => day.date),
  );

  function flushActiveAbsence() {
    if (!activeAbsence) return;
    absenceRuns.push(activeAbsence);
    activeAbsence = null;
  }

  for (const day of report.days) {
    for (const entry of day.entries || []) {
      entries.push({
        entry_date: day.date,
        start_time: entry.start_time,
        end_time: entry.end_time,
        minutes: entry.minutes,
        category_name: entry.category,
        counts_as_work: entry.counts_as_work,
        status: entry.status,
        comment: entry.comment,
      });
    }

    if (!day.absence) {
      flushActiveAbsence();
      continue;
    }

    if (!activeAbsence || activeAbsence.kind !== day.absence) {
      flushActiveAbsence();
      activeAbsence = {
        kind: day.absence,
        start_date: day.date,
        end_date: day.date,
      };
      continue;
    }

    activeAbsence.end_date = day.date;
  }

  flushActiveAbsence();

  // The runs are counted together, not one by one: two stretches of leave in
  // the same calendar week share that week's quota (see `withAbsenceDays`).
  const reportDates = report.days.map((day) => day.date);
  const absences = withAbsenceDays(absenceRuns, {
    from: reportDates[0],
    to: reportDates[reportDates.length - 1],
    holidays: holidaySet,
    workdaysPerWeek,
  });

  return {
    ...report,
    entries,
    absences,
  };
}
