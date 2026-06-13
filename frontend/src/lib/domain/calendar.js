import { durMin, minToHM } from "../../format.js";
import { absenceKindLabel } from "../../i18n.js";
import {
  FALLBACK_COLORS,
  HOLIDAY_COLOR,
  MASKED_ABSENCE_COLOR,
} from "../../colors.js";

// Return the DB-stored color for a given absence slug from the category lookup,
// falling back to the masked-absence color for unknown/private kinds.
export function absColor(kind, absenceCategoryMap) {
  return absenceCategoryMap.get(kind)?.color || MASKED_ABSENCE_COLOR;
}

export function normalizeColor(color) {
  return /^#[0-9a-f]{6}$/i.test(color || "") ? color.toLowerCase() : null;
}

export function fallbackColor(offset = 0, used = new Set()) {
  for (let i = 0; i < FALLBACK_COLORS.length; i++) {
    const color = FALLBACK_COLORS[(offset + i) % FALLBACK_COLORS.length];
    if (!used.has(color.toLowerCase())) return color;
  }
  const hue = (offset * 47) % 360;
  return `hsl(${hue} 70% 38%)`;
}

export function categoryForEntry(entry, categoryMap) {
  return categoryMap.get(entry.category_id) || null;
}

export function workLabel(entry, categoryMap) {
  return categoryForEntry(entry, categoryMap)?.name || "Work time";
}

export function workBaseColor(entry, offset, categoryMap) {
  return (
    normalizeColor(categoryForEntry(entry, categoryMap)?.color) ||
    fallbackColor(offset)
  );
}

export function absenceDetail(absence) {
  return [absence.name, absence.comment].filter(Boolean).join(" - ");
}

export function rawCellEvents(
  cell,
  entryMap,
  categoryMap,
  absenceCategoryMap,
  translate,
  userMap = new Map(),
  currentUserId = null,
) {
  const events = [];
  if (cell.hol) {
    events.push({
      key: "holiday",
      color: HOLIDAY_COLOR,
      label: translate("Holiday"),
      detail: cell.hol,
    });
  }
  for (const absence of cell.absences) {
    const label = absenceKindLabel(absence.kind);
    events.push({
      key: `absence:${absence.kind}`,
      color: absColor(absence.kind, absenceCategoryMap),
      label,
      title: label,
      detail: absenceDetail(absence),
    });
  }
  for (const entry of entryMap.get(cell.ds) || []) {
    const startTime = entry.start_time?.slice(0, 5) || "";
    const endTime = entry.end_time?.slice(0, 5) || "";
    const durationLabel =
      startTime && endTime ? minToHM(durMin(startTime, endTime)) : "";
    const timeRange = startTime && endTime ? `${startTime} - ${endTime}` : "";
    const timeDetail = durationLabel ? `${timeRange} (${durationLabel})` : timeRange;
    const isOwn = entry.user_id === currentUserId;
    const entryUser = !isOwn ? userMap.get(entry.user_id) : null;
    const userName = entryUser
      ? `${entryUser.first_name} ${entryUser.last_name}`
      : null;
    const detail = userName ? `${userName} – ${timeDetail}` : timeDetail;
    events.push({
      key: `work:${entry.category_id ?? "unknown"}`,
      color: workBaseColor(entry, events.length, categoryMap),
      label: translate(workLabel(entry, categoryMap)),
      detail,
    });
  }
  return events;
}

export function buildColorMap(baseCells, entryMap, categoryMap, absenceCategoryMap, translate) {
  // Reserve the holiday color and all DB-stored absence colors so work-category
  // colors are never assigned a value that would clash with absence bands.
  const reservedColors = new Set([
    HOLIDAY_COLOR.toLowerCase(),
    MASKED_ABSENCE_COLOR.toLowerCase(),
    ...Array.from(absenceCategoryMap.values())
      .map((c) => c.color?.toLowerCase())
      .filter(Boolean),
  ]);
  const assigned = new Map();
  const used = new Set();
  for (const cell of baseCells) {
    if (cell.other) continue;
    for (const event of rawCellEvents(cell, entryMap, categoryMap, absenceCategoryMap, translate)) {
      if (assigned.has(event.key)) continue;
      const isWorkEvent = event.key.startsWith("work:");
      const blocked = new Set([...used, ...reservedColors]);
      let color =
        normalizeColor(event.color) || fallbackColor(assigned.size, blocked);
      if (isWorkEvent) {
        if (used.has(color) || reservedColors.has(color)) {
          color = fallbackColor(assigned.size, blocked);
        }
      } else if (used.has(color)) {
        color = fallbackColor(assigned.size, blocked);
      }
      assigned.set(event.key, color);
      used.add(color);
    }
  }
  return assigned;
}

export function cellEvents(
  cell,
  entryMap,
  categoryMap,
  colorMap,
  absenceCategoryMap,
  translate,
  userMap,
  currentUserId,
) {
  return rawCellEvents(
    cell,
    entryMap,
    categoryMap,
    absenceCategoryMap,
    translate,
    userMap,
    currentUserId,
  ).map((event) => ({
    ...event,
    color: colorMap.get(event.key) || event.color,
  }));
}

export function calendarEventTitle(event) {
  return String(event?.title || event?.detail || event?.label || "").trim();
}
