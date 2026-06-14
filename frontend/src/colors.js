// Single source of truth for all calendar, chart, and report colors.
// Import from here in every component that needs these colors.

// Amber: reserved for public holidays everywhere.
export const HOLIDAY_COLOR = "#f59e0b";

// Neutral gray for weekend background bands.
export const WEEKEND_COLOR = "#9ca3af";

// Color used when an absence slug can't be resolved to a category in the
// store — e.g. an absence whose category was deactivated and dropped from
// the active-only frontend cache, or an entry returned by an API path
// that doesn't carry the joined category color. The calendar's visibility
// scope (admin sees all, lead sees self+reports, employee sees only own)
// is enforced server-side, so this is no longer a privacy-mask color in
// practice — just a "category unknown" fallback.
export const MASKED_ABSENCE_COLOR = "#78716c"; // stone

// Fallback palette for work categories that have no color stored in the DB.
// buildColorMap reserves MASKED_ABSENCE_COLOR, HOLIDAY_COLOR, and all DB-stored
// absence category colors, so any entry here that duplicates a reserved color is
// automatically skipped.
// Index 5 uses lime (#84cc16) instead of the original slate that duplicated "unpaid".
// Index 11 (#0d9488) is teal-500 and will be skipped if any absence category uses
// that color; it acts as a last-resort slot before HSL generation.
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
