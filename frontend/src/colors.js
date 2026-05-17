// Single source of truth for all calendar, chart, and report colors.
// Import from here in every component that needs these colors.

// Amber: reserved for public holidays everywhere.
export const HOLIDAY_COLOR = "#f59e0b";

// Neutral gray for weekend background bands.
export const WEEKEND_COLOR = "#9ca3af";

// One canonical color per absence kind. All views must use these values.
export const ABSENCE_COLORS = Object.freeze({
  vacation:           "#3b82f6", // blue
  sick:               "#ef4444", // red
  training:           "#0d9488", // teal
  special_leave:      "#a855f7", // purple
  unpaid:             "#64748b", // slate
  general_absence:    "#6b7280", // gray
  flextime_reduction: "#6D4C41", // brown
  absent:             "#78716c", // stone (catch-all fallback, distinct from weekend gray)
});

// Fallback palette for work categories that have no color stored in the DB.
// buildColorMap reserves all ABSENCE_COLORS and HOLIDAY_COLOR via exact-match,
// so any entry here that duplicates a reserved color is automatically skipped.
// Index 5 uses lime (#84cc16) instead of the original slate that duplicated "unpaid".
// Index 11 (#0d9488) is the exact value of "training" and will be skipped when
// training absence is present; it acts as a last-resort slot before HSL generation.
export const FALLBACK_COLORS = [
  "#2563eb", // blue-600
  "#10b981", // emerald-500
  "#8b5cf6", // violet-500
  "#14b8a6", // teal-400
  "#ec4899", // pink-500
  "#84cc16", // lime-400  (replaces #64748b which duplicated "unpaid")
  "#0f766e", // teal-700
  "#7c3aed", // violet-700
  "#0891b2", // cyan-600
  "#d946ef", // fuchsia-500
  "#4f46e5", // indigo-600
  "#0d9488", // teal-500  (exact duplicate of "training"; skipped by reserved-color check)
];
