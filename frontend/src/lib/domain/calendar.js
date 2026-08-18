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

// Everything the calendar needs to turn a day cell into displayable events.
// Passed as one object because the day grid, the legend and the color map all
// need the same lookups and a positional argument list of this length is easy
// to get wrong at the call site.
function readContext(context) {
  const {
    entryMap = new Map(),
    categoryMap = new Map(),
    absenceCategoryMap = new Map(),
    translate = (key) => key,
    userMap = new Map(),
    currentUserId = null,
    currentUserName = null,
  } = context || {};
  return {
    entryMap,
    categoryMap,
    absenceCategoryMap,
    translate,
    userMap,
    currentUserId,
    currentUserName,
  };
}

// Full name of the person a time entry belongs to. Team leads and admins get
// the name from the users lookup; for the viewer's own entries the lookup may
// be empty (employees never load /users), so their own name is used instead.
function personNameForEntry(
  entry,
  { userMap, currentUserId, currentUserName },
) {
  const entryUser = userMap.get(entry.user_id);
  if (entryUser) {
    return `${entryUser.first_name} ${entryUser.last_name}`.trim() || null;
  }
  if (entry.user_id === currentUserId) return currentUserName || null;
  return null;
}

// Rank used to order event groups everywhere they are shown (legend, day
// cells, day popup): holidays first, then absences, then work categories.
export function eventGroupRank(colorKey) {
  if (colorKey === "holiday") return 0;
  if (String(colorKey ?? "").startsWith("absence:")) return 1;
  return 2;
}

// Shared comparator for anything carrying `colorKey` + `label` (legend items
// and day groups alike), so a category never changes position between views.
export function compareEventGroups(a, b) {
  const rankDifference =
    eventGroupRank(a.colorKey) - eventGroupRank(b.colorKey);
  if (rankDifference !== 0) return rankDifference;
  return String(a.label ?? "").localeCompare(String(b.label ?? ""));
}

export function rawCellEvents(cell, context) {
  const resolved = readContext(context);
  const { entryMap, categoryMap, absenceCategoryMap, translate } = resolved;
  const events = [];
  if (cell.hol) {
    events.push({
      key: "holiday",
      // `colorKey` groups events for shared coloring (see buildColorMap) and
      // for the one-chip-per-category grouping in the day cells: every event
      // of the same kind/category must resolve to the same color and collapse
      // into the same group regardless of how many records produced it.
      colorKey: "holiday",
      color: HOLIDAY_COLOR,
      label: translate("Holiday"),
      // The day cell shows the holiday's own name instead of the generic
      // category label — a day never has more than one holiday, so nothing is
      // hidden by being specific here.
      title: cell.hol,
      personName: null,
      detail: cell.hol,
    });
  }
  for (const absence of cell.absences) {
    // `category_name` is sent by the backend's calendar endpoint for entries
    // whose kind is visible to this requester. It allows `absenceKindLabel`
    // to translate the real name even when the category has been deactivated
    // and dropped from the active-only frontend store. For privacy-masked
    // entries the backend nulls it out; the label still falls through to the
    // generic placeholder.
    const label = absenceKindLabel(absence.kind, absence.category_name);
    events.push({
      // `key` must be unique per rendered event (it's the Svelte keyed-each
      // identity). Two different people can have overlapping same-category
      // absences on the same day (e.g. overlapping vacations) — keying only
      // by kind collides and throws `each_key_duplicate`, which aborts the
      // whole calendar's render.
      key: `absence:${absence.id}`,
      colorKey: `absence:${absence.kind}`,
      color: absColor(absence.kind, absenceCategoryMap),
      label,
      title: label,
      personName: absence.name || null,
      detail: absence.comment || "",
    });
  }
  for (const entry of entryMap.get(cell.ds) || []) {
    const startTime = entry.start_time?.slice(0, 5) || "";
    const endTime = entry.end_time?.slice(0, 5) || "";
    const durationLabel =
      startTime && endTime ? minToHM(durMin(startTime, endTime)) : "";
    const timeRange = startTime && endTime ? `${startTime} - ${endTime}` : "";
    const timeDetail = durationLabel
      ? `${timeRange} (${durationLabel})`
      : timeRange;
    const label = translate(workLabel(entry, categoryMap));
    events.push({
      // Same rationale as absences above: multiple team members can share a
      // work category on the same day in a team calendar.
      key: `work:${entry.id}`,
      colorKey: `work:${entry.category_id ?? "unknown"}`,
      color: workBaseColor(entry, events.length, categoryMap),
      label,
      // The day cell labels a work chip with its category, never with the
      // person — whose entry it is belongs in the popup, next to the times.
      title: label,
      personName: personNameForEntry(entry, resolved),
      detail: timeDetail,
    });
  }
  return events;
}

export function buildColorMap(baseCells, context) {
  const { absenceCategoryMap } = readContext(context);
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
    for (const event of rawCellEvents(cell, context)) {
      if (assigned.has(event.colorKey)) continue;
      const isWorkEvent = event.colorKey.startsWith("work:");
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
      assigned.set(event.colorKey, color);
      used.add(color);
    }
  }
  return assigned;
}

export function cellEvents(cell, context, colorMap = new Map()) {
  return rawCellEvents(cell, context).map((event) => ({
    ...event,
    color: colorMap.get(event.colorKey) || event.color,
  }));
}

// Collapse a day's events into one group per category. The day cell renders
// one chip per group (six people on vacation produce a single "Vacation"
// chip), and the day popup lists every underlying record inside its group,
// sorted by person so the same day always reads the same way.
export function groupDayEvents(events) {
  const groupsByColorKey = new Map();
  for (const event of events) {
    let group = groupsByColorKey.get(event.colorKey);
    if (!group) {
      group = {
        key: event.colorKey,
        colorKey: event.colorKey,
        color: event.color,
        label: event.label,
        title: event.title || event.label,
        items: [],
      };
      groupsByColorKey.set(event.colorKey, group);
    }
    group.items.push({
      key: event.key,
      // One popup row per record: the person on the left, the record's own
      // detail (time range or comment) on the right. Rows that have no person
      // — holidays — promote their detail into the primary column so every
      // row still starts at the same indent.
      primary: event.personName || event.detail || "",
      secondary: event.personName ? event.detail || "" : "",
    });
  }
  for (const group of groupsByColorKey.values()) {
    group.items.sort(
      (a, b) =>
        a.primary.localeCompare(b.primary) ||
        a.secondary.localeCompare(b.secondary),
    );
    group.count = group.items.length;
  }
  return [...groupsByColorKey.values()].sort(compareEventGroups);
}

export function calendarEventTitle(group) {
  return String(group?.title || group?.label || "").trim();
}
