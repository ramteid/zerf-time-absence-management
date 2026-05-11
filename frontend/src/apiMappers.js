function parseIsoDate(value) {
  const [year, month, day] = String(value).split("-").map(Number);
  return new Date(year, month - 1, day);
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

function isWeekend(value, workdaysPerWeek = 5) {
  // Determine if a date is a non-contract day based on user's workdays_per_week.
  // 
  // JavaScript getDay() returns: 0=Sunday, 1=Monday, ..., 6=Saturday
  // ISO weekday (expected format): 0=Monday, 1=Tuesday, ..., 6=Sunday
  // 
  // Conversion: (getDay() + 6) % 7 converts JS day to ISO day
  // Examples:
  //   - Sunday (0) → 6 (ISO Sunday)
  //   - Monday (1) → 0 (ISO Monday)
  //   - Saturday (6) → 5 (ISO Saturday)
  //
  // A day is a "weekend" (non-contract) if: ISO_weekday >= workdaysPerWeek
  // Examples with workdaysPerWeek=5 (Mon-Fri):
  //   - Monday (0) < 5 → contract day
  //   - Friday (4) < 5 → contract day
  //   - Saturday (5) >= 5 → non-contract day
  // Examples with workdaysPerWeek=4 (Mon-Thu):
  //   - Thursday (3) < 4 → contract day
  //   - Friday (4) >= 4 → non-contract day (not worked)
  let isoWeekday = (value.getDay() + 6) % 7;
  return isoWeekday >= workdaysPerWeek;
}

export function holidayDateSet(holidays = []) {
  return new Set(holidays.map((holiday) => holiday.holiday_date));
}

export function countWorkdays(startDate, endDate, holidays = new Set(), workdaysPerWeek = 5) {
  // Count contract workdays in a date range.
  // 
  // Contract workdays depend on user's workdays_per_week configuration:
  //   - workdaysPerWeek=5: Mon-Fri (default 5-day week)
  //   - workdaysPerWeek=4: Mon-Thu (4-day week)
  //   - workdaysPerWeek=6: Mon-Sat (6-day week)
  //
  // A day is counted if:
  //   1. It is a contract workday (not in the non-contract tail of the week), AND
  //   2. It is not a public holiday
  //
  // Used to calculate vacation days taken, absence days, etc.
  // This matches the backend's workday_for_user() logic.
  const start = parseIsoDate(startDate);
  const end = parseIsoDate(endDate);
  if (
    Number.isNaN(start.getTime()) ||
    Number.isNaN(end.getTime()) ||
    end < start
  ) {
    return 0;
  }

  let days = 0;
  for (
    let current = new Date(start);
    current <= end;
    current = addCalendarDays(current, 1)
  ) {
    const currentDate = formatIsoDate(current);
    // Skip non-contract days (weekend tail) and holidays
    if (!isWeekend(current, workdaysPerWeek) && !holidays.has(currentDate)) {
      days += 1;
    }
  }

  return days;
}

const WEEKDAY_TO_INDEX = Object.freeze({
  Monday: 0,
  Tuesday: 1,
  Wednesday: 2,
  Thursday: 3,
  Friday: 4,
  Saturday: 5,
  Sunday: 6,
});

export function normalizeMonthReport(report, workdaysPerWeek = 5) {
  if (!report || !Array.isArray(report.days)) {
    return report;
  }

  const entries = [];
  const absences = [];
  let activeAbsence = null;

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
      if (activeAbsence) {
        absences.push(activeAbsence);
        activeAbsence = null;
      }
      continue;
    }

    const weekdayIndex = WEEKDAY_TO_INDEX[day.weekday];
    const countedDay =
      weekdayIndex == null
        ? !["Saturday", "Sunday"].includes(day.weekday) && !day.holiday
        : weekdayIndex < workdaysPerWeek && !day.holiday;
    if (!activeAbsence || activeAbsence.kind !== day.absence) {
      if (activeAbsence) {
        absences.push(activeAbsence);
      }
      activeAbsence = {
        kind: day.absence,
        start_date: day.date,
        end_date: day.date,
        days: countedDay ? 1 : 0,
      };
      continue;
    }

    activeAbsence.end_date = day.date;
    if (countedDay) {
      activeAbsence.days += 1;
    }
  }

  if (activeAbsence) {
    absences.push(activeAbsence);
  }

  return {
    ...report,
    entries,
    absences,
  };
}
