export function summarizeAbsences(absences) {
  const summary = {};
  for (const absence of absences || []) {
    summary[absence.kind] = (summary[absence.kind] || 0) + (absence.days || 0);
  }
  return summary;
}

export function categoryNamesFromTeamReport(rows) {
  return [
    ...new Set(
      (rows || []).flatMap((row) =>
        (row.categories || []).map((category) => category.category),
      ),
    ),
  ];
}

export function categoryColumnsFromTeamReport(rows) {
  const totals = new Map();
  for (const row of rows || []) {
    for (const categoryEntry of row.categories || []) {
      const entryTotals = totals.get(categoryEntry.category) || {
        color: categoryEntry.color,
        total: 0,
      };
      entryTotals.total += categoryEntry.minutes || 0;
      totals.set(categoryEntry.category, entryTotals);
    }
  }
  return [...totals.entries()]
    .sort((a, b) => b[1].total - a[1].total || a[0].localeCompare(b[0]))
    .map(([category, { color }]) => ({ category, color }));
}

export function filterCategories(rows, selectedCategories) {
  if (!Array.isArray(selectedCategories)) return rows || [];
  if (selectedCategories.length === 0) return [];
  return (rows || []).filter((row) =>
    selectedCategories.includes(row.category),
  );
}

export function filterTeamCategoryColumns(columns, selectedCategories) {
  if (!Array.isArray(selectedCategories)) return columns || [];
  if (selectedCategories.length === 0) return [];
  return (columns || []).filter((column) =>
    selectedCategories.includes(column.category),
  );
}

export function teamCategoryMinutes(row, category) {
  return (
    (row?.categories || []).find((item) => item.category === category)
      ?.minutes || 0
  );
}

export function teamCategoryRowTotal(row, selectedCategories = null) {
  return (row?.categories || []).reduce(
    (total, category) =>
      selectedCategories && !selectedCategories.includes(category.category)
        ? total
        : total + (category.minutes || 0),
    0,
  );
}

export function totalCategoryMinutes(rows) {
  return (rows || []).reduce((total, row) => total + (row.minutes || 0), 0);
}

export function dedupeAbsences(absences) {
  const seen = new Set();
  return (absences || []).filter((absence) => {
    if (seen.has(absence.id)) return false;
    seen.add(absence.id);
    return true;
  });
}

export function absenceKindTotals(absences) {
  const totals = {};
  for (const absence of absences || []) {
    const kind = absence.kind || "unknown";
    totals[kind] = (totals[kind] || 0) + (absence.days || 0);
  }
  // Exclude kinds whose total is zero so stat cards don't display "Sick: 0".
  return Object.fromEntries(Object.entries(totals).filter(([, days]) => days > 0));
}

export function totalAbsenceDays(absences) {
  return (absences || []).reduce(
    (total, absence) => total + (absence.days || 0),
    0,
  );
}
