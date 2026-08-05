import { get } from "svelte/store";
import { absenceCategories } from "../../stores.js";

// Build a slug→cost_type lookup map from the absenceCategories store.
// Used to identify flextime-cost categories that must be excluded from
// leave-day statistics (their days still require hours, so they are not
// "absences" in the leave-account sense).
function slugToCostType() {
  const costTypeBySlug = new Map();
  for (const category of get(absenceCategories)) {
    costTypeBySlug.set(category.slug, category.cost_type);
  }
  return costTypeBySlug;
}

// Delegates to absenceKindTotals for consistent exclusion of flextime categories.
export function summarizeAbsences(absences) {
  return absenceKindTotals(absences);
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

// Leave-account report data is keyed solely by category_id. Names are display
// metadata and are deliberately not used to join a row to its account column:
// two categories may legitimately have the same visible name.
export function leaveAccountUsage(row, categoryId) {
  return (
    (row?.leave_account_usage || []).find(
      (usage) => usage.category_id === categoryId,
    ) || { taken_days: 0, planned_days: 0 }
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
  const costTypeBySlug = slugToCostType();
  const totals = {};
  for (const absence of absences || []) {
    // Categories with cost_type="flextime" (e.g. flextime_reduction) are not
    // traditional leave: the day still counts toward the work requirement, so
    // these must not inflate leave-day statistics.
    if (costTypeBySlug.get(absence.kind) === "flextime") continue;
    const kind = absence.kind || "unknown";
    totals[kind] = (totals[kind] || 0) + (absence.days || 0);
  }
  // Exclude kinds whose total is zero so stat cards don't display "Sick: 0".
  return Object.fromEntries(
    Object.entries(totals).filter(([, days]) => days > 0),
  );
}

export function totalAbsenceDays(absences) {
  // Exclude cost_type="flextime" categories: those days still require hours, so
  // counting them as "absence days" would overstate the user's true leave.
  const costTypeBySlug = slugToCostType();
  return (absences || [])
    .filter((absence) => costTypeBySlug.get(absence.kind) !== "flextime")
    .reduce((total, absence) => total + (absence.days || 0), 0);
}
