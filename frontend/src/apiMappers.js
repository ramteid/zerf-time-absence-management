export function normalizeMonthReport(report) {
  if (!report || !Array.isArray(report.days)) {
    return report;
  }

  const entries = [];
  const absenceRuns = [];
  let activeAbsence = null;

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

  return {
    ...report,
    entries,
    absences: absenceRuns,
  };
}
