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

function isWeekend(value) {
  const weekday = value.getDay();
  return weekday === 0 || weekday === 6;
}

export function holidayDateSet(holidays = []) {
  return new Set(holidays.map((holiday) => holiday.holiday_date));
}

export function countWorkdays(startDate, endDate, holidays = new Set()) {
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
    if (!isWeekend(current) && !holidays.has(currentDate)) {
      days += 1;
    }
  }

  return days;
}

export function normalizeMonthReport(report) {
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

    const countedDay =
      !["Saturday", "Sunday"].includes(day.weekday) && !day.holiday;
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
