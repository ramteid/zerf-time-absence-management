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

export function countWorkdays(
  startDate,
  endDate,
  holidays = new Set(),
  workdaysPerWeek = 5,
) {
  // Count effective workdays in a date range without fixed weekdays.
  // For irregular (workdays <=0) count calendar days excluding holidays,
  // matching backend's count_workdays irregular branch.
  const start = parseIsoDate(startDate);
  const end = parseIsoDate(endDate);
  const configuredDays = Number(workdaysPerWeek);
  if (
    Number.isNaN(start.getTime()) ||
    Number.isNaN(end.getTime()) ||
    end < start ||
    !Number.isFinite(configuredDays)
  ) {
    return 0;
  }
  if (configuredDays <= 0) {
    let total = 0;
    for (
      let current = new Date(start);
      current <= end;
      current = addCalendarDays(current, 1)
    ) {
      const currentDate = formatIsoDate(current);
      if (!holidays.has(currentDate)) total += 1;
    }
    return total;
  }

  const countedByWeek = new Map();
  for (
    let current = new Date(start);
    current <= end;
    current = addCalendarDays(current, 1)
  ) {
    const currentDate = formatIsoDate(current);
    if (
      isPotentialWorkday(current, configuredDays) &&
      !holidays.has(currentDate)
    ) {
      const weekKey = formatIsoDate(weekMonday(current));
      countedByWeek.set(weekKey, (countedByWeek.get(weekKey) || 0) + 1);
    }
  }

  let total = 0;
  for (const daysInWeek of countedByWeek.values()) {
    total += Math.min(daysInWeek, configuredDays);
  }
  return total;
}

export function normalizeMonthReport(report, workdaysPerWeek = 5) {
  if (!report || !Array.isArray(report.days)) {
    return report;
  }

  const entries = [];
  const absences = [];
  let activeAbsence = null;
  const holidaySet = new Set(
    report.days.filter((day) => !!day.holiday).map((day) => day.date),
  );

  function flushActiveAbsence() {
    if (!activeAbsence) return;
    absences.push({
      ...activeAbsence,
      days: countWorkdays(
        activeAbsence.start_date,
        activeAbsence.end_date,
        holidaySet,
        workdaysPerWeek,
      ),
    });
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

  return {
    ...report,
    entries,
    absences,
  };
}
