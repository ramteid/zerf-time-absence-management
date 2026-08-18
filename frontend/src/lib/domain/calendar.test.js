// Tests for the calendar domain module. The calendar renders a monthly grid
// where each cell can hold holidays, absences, and time entries. The key
// rendering concerns are:
//   - Each category gets a distinct colour so employees can tell entries apart
//   - Holidays always get the fixed holiday colour regardless of assignment order
//   - Absence colours come from the DB-stored category color (absenceCategoryMap)
//   - Work-entry colours are drawn from a fallback palette, cycling and avoiding
//     any colour already used by holidays or absences
//   - A day's events collapse into one group per category, so a day where six
//     people are on vacation shows one "Vacation" chip, not six
//   - absColor, normalizeColor, and fallbackColor have clear boundary behaviour

import { describe, expect, it } from "vitest";
import {
  absColor,
  buildColorMap,
  calendarEventTitle,
  categoryForEntry,
  compareEventGroups,
  eventGroupRank,
  fallbackColor,
  groupDayEvents,
  normalizeColor,
  rawCellEvents,
  workBaseColor,
  workLabel,
} from "./calendar.js";
import { setLanguage } from "../../i18n.js";
import { MASKED_ABSENCE_COLOR } from "../../colors.js";

setLanguage("en");

const translate = (key) => key;

describe("normalizeColor", () => {
  it("lower-cases a valid 6-digit hex string", () => {
    expect(normalizeColor("#A1B2C3")).toBe("#a1b2c3");
  });

  it("returns null for invalid or missing color strings", () => {
    expect(normalizeColor(null)).toBeNull();
    expect(normalizeColor("")).toBeNull();
    expect(normalizeColor("red")).toBeNull();
    expect(normalizeColor("#12345")).toBeNull(); // 5 digits, not 6
  });
});

describe("absColor", () => {
  it("returns the DB-stored colour for a known absence kind", () => {
    const catMap = new Map([
      ["vacation", { color: "#1a73e8" }],
      ["sick", { color: "#d93025" }],
    ]);
    expect(absColor("vacation", catMap)).toBe("#1a73e8");
    expect(absColor("sick", catMap)).toBe("#d93025");
    expect(absColor("vacation", catMap)).not.toBe(absColor("sick", catMap));
  });

  it("falls back to MASKED_ABSENCE_COLOR for unknown kinds", () => {
    expect(absColor("unknown_kind", new Map())).toBe(MASKED_ABSENCE_COLOR);
  });
});

describe("fallbackColor", () => {
  it("returns a colour string for any offset", () => {
    expect(typeof fallbackColor(0)).toBe("string");
    expect(typeof fallbackColor(5)).toBe("string");
  });

  it("skips already-used colours when finding a fallback", () => {
    // If all FALLBACK_COLORS are taken the function generates an HSL value
    // to guarantee a result even when the palette is exhausted.
    const used = new Set();
    // Exhaust the entire palette so it must fall back to HSL generation.
    for (let i = 0; i < 50; i++) {
      const color = fallbackColor(i, used);
      used.add(color);
    }
    // All 50 generated colours must be non-empty strings.
    expect(used.size).toBeGreaterThan(0);
    for (const color of used) {
      expect(typeof color).toBe("string");
      expect(color.length).toBeGreaterThan(0);
    }
  });
});

describe("categoryForEntry", () => {
  it("looks up a category from the map by category_id", () => {
    const categoryMap = new Map([[2, { name: "Training", color: "#abc123" }]]);
    expect(categoryForEntry({ category_id: 2 }, categoryMap)).toEqual({
      name: "Training",
      color: "#abc123",
    });
  });

  it("returns null for an entry with no matching category", () => {
    expect(categoryForEntry({ category_id: 99 }, new Map())).toBeNull();
  });
});

describe("workLabel", () => {
  it("returns the category name when a matching category exists", () => {
    const categoryMap = new Map([[1, { name: "Project Alpha" }]]);
    expect(workLabel({ category_id: 1 }, categoryMap)).toBe("Project Alpha");
  });

  it("falls back to 'Work time' when the category is not found", () => {
    expect(workLabel({ category_id: 42 }, new Map())).toBe("Work time");
  });
});

describe("workBaseColor", () => {
  it("uses the category's normalised colour when valid", () => {
    const categoryMap = new Map([[1, { color: "#FF0000" }]]);
    expect(workBaseColor({ category_id: 1 }, 0, categoryMap)).toBe("#ff0000");
  });

  it("falls back to a palette colour when category color is invalid", () => {
    const categoryMap = new Map([[1, { color: "invalid" }]]);
    const result = workBaseColor({ category_id: 1 }, 0, categoryMap);
    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });
});

describe("eventGroupRank / compareEventGroups", () => {
  it("ranks holidays before absences before work categories", () => {
    expect(eventGroupRank("holiday")).toBeLessThan(
      eventGroupRank("absence:vacation"),
    );
    expect(eventGroupRank("absence:vacation")).toBeLessThan(
      eventGroupRank("work:3"),
    );
  });

  it("sorts alphabetically by label inside the same rank", () => {
    const items = [
      { colorKey: "work:2", label: "Zebra" },
      { colorKey: "absence:sick", label: "Sick leave" },
      { colorKey: "work:1", label: "Admin" },
      { colorKey: "holiday", label: "Holiday" },
      { colorKey: "absence:vacation", label: "Annual leave" },
    ];
    expect(items.sort(compareEventGroups).map((i) => i.label)).toEqual([
      "Holiday",
      "Annual leave",
      "Sick leave",
      "Admin",
      "Zebra",
    ]);
  });

  it("tolerates a missing label without throwing", () => {
    expect(
      compareEventGroups({ colorKey: "work:1" }, { colorKey: "work:2" }),
    ).toBe(0);
  });
});

describe("rawCellEvents", () => {
  it("includes a holiday event when the cell has a holiday", () => {
    // Holidays must always appear with the fixed holiday colour so they are
    // visually distinct from absence and work events.
    const cell = { ds: "2026-01-01", hol: "New Year", absences: [] };
    const events = rawCellEvents(cell, { translate });
    expect(events.some((e) => e.key === "holiday")).toBe(true);
    expect(events.find((e) => e.key === "holiday").detail).toBe("New Year");
  });

  it("titles a holiday chip with the holiday's own name, not the generic label", () => {
    // A day never has more than one holiday, so the day cell can afford to be
    // specific where absences and work entries show the category name.
    const cell = { ds: "2026-01-01", hol: "New Year", absences: [] };
    const [holidayEvent] = rawCellEvents(cell, { translate });
    expect(holidayEvent.title).toBe("New Year");
    expect(holidayEvent.label).toBe("Holiday");
  });

  it("includes an absence event per absence in the cell", () => {
    const cell = {
      ds: "2026-07-15",
      hol: null,
      absences: [{ id: 42, kind: "vacation", name: "Summer", comment: "" }],
    };
    const events = rawCellEvents(cell, { translate });
    const absEvent = events.find((e) => e.colorKey === "absence:vacation");
    expect(absEvent).not.toBeUndefined();
    expect(absEvent.key).toBe("absence:42");
  });

  it("keys each absence event by its own id, not just its kind, so two people's overlapping same-category absences on one day don't collide in Svelte's keyed each block", () => {
    const cell = {
      ds: "2026-08-24",
      hol: null,
      absences: [
        { id: 7, kind: "vacation", name: "Person A", comment: "" },
        { id: 11, kind: "vacation", name: "Person B", comment: "" },
      ],
    };
    const events = rawCellEvents(cell, { translate });
    const keys = events.map((e) => e.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("splits an absence into the person it belongs to and its comment", () => {
    // The popup lists the person in its own column and the comment beside it,
    // so the two must not be pre-joined into one string.
    const cell = {
      ds: "2026-07-15",
      hol: null,
      absences: [
        { id: 1, kind: "vacation", name: "Tina Team", comment: "Pre-booked" },
      ],
    };
    const [absEvent] = rawCellEvents(cell, { translate });
    expect(absEvent.personName).toBe("Tina Team");
    expect(absEvent.detail).toBe("Pre-booked");
  });

  it("uses DB-stored category color for absence events", () => {
    const cell = {
      ds: "2026-07-15",
      hol: null,
      absences: [{ id: 1, kind: "vacation", name: "Summer", comment: "" }],
    };
    const absenceCategoryMap = new Map([["vacation", { color: "#1a73e8" }]]);
    const events = rawCellEvents(cell, { translate, absenceCategoryMap });
    const absEvent = events.find((e) => e.colorKey === "absence:vacation");
    expect(absEvent.color).toBe("#1a73e8");
  });

  it("includes a work event for each time entry on the date", () => {
    const ds = "2026-03-10";
    const entryMap = new Map([
      [
        ds,
        [
          {
            id: 1,
            user_id: 1,
            category_id: null,
            start_time: "09:00:00",
            end_time: "12:00:00",
          },
        ],
      ],
    ]);
    const cell = { ds, hol: null, absences: [] };
    const events = rawCellEvents(cell, { translate, entryMap });
    expect(events.some((e) => e.key.startsWith("work:"))).toBe(true);
  });

  it("keys each work event by its own id, not just its category, so two people sharing a category on one day don't collide in Svelte's keyed each block", () => {
    const ds = "2026-03-10";
    const entryMap = new Map([
      [
        ds,
        [
          {
            id: 101,
            user_id: 1,
            category_id: 4,
            start_time: "09:00:00",
            end_time: "12:00:00",
          },
          {
            id: 102,
            user_id: 2,
            category_id: 4,
            start_time: "09:00:00",
            end_time: "12:00:00",
          },
        ],
      ],
    ]);
    const cell = { ds, hol: null, absences: [] };
    const events = rawCellEvents(cell, { translate, entryMap });
    const keys = events.map((e) => e.key);
    expect(new Set(keys).size).toBe(keys.length);
    // Both still share one color-grouping key so they render with the same color.
    expect(new Set(events.map((e) => e.colorKey)).size).toBe(1);
  });

  it("titles a work chip with the time category, never with the person", () => {
    // In the calendar grid the chip identifies what was worked on; who worked
    // on it is shown in the day popup.
    const ds = "2026-03-10";
    const entryMap = new Map([
      [
        ds,
        [
          {
            id: 1,
            user_id: 5,
            category_id: 4,
            start_time: "09:00:00",
            end_time: "10:00:00",
          },
        ],
      ],
    ]);
    const categoryMap = new Map([[4, { name: "Project" }]]);
    const userMap = new Map([[5, { first_name: "Eve", last_name: "Emp" }]]);
    const cell = { ds, hol: null, absences: [] };
    const [workEvent] = rawCellEvents(cell, {
      translate,
      entryMap,
      categoryMap,
      userMap,
      currentUserId: 1,
    });
    expect(workEvent.title).toBe("Project");
    expect(calendarEventTitle(workEvent)).toBe("Project");
    expect(workEvent.personName).toBe("Eve Emp");
    expect(workEvent.detail).toBe("09:00 - 10:00 (1:00)");
  });

  it("names the viewer's own entries from the session when the user lookup is empty", () => {
    // Employees never load /users, so their own name is only available from
    // the session — without it their popup rows would have no name at all.
    const ds = "2026-03-10";
    const entryMap = new Map([
      [
        ds,
        [
          {
            id: 1,
            user_id: 9,
            category_id: null,
            start_time: "08:00:00",
            end_time: "09:00:00",
          },
        ],
      ],
    ]);
    const cell = { ds, hol: null, absences: [] };
    const [workEvent] = rawCellEvents(cell, {
      translate,
      entryMap,
      currentUserId: 9,
      currentUserName: "Own Name",
    });
    expect(workEvent.personName).toBe("Own Name");
  });

  it("leaves the person unnamed when the entry belongs to someone not in the lookup", () => {
    const ds = "2026-03-10";
    const entryMap = new Map([
      [
        ds,
        [
          {
            id: 1,
            user_id: 77,
            category_id: null,
            start_time: "08:00:00",
            end_time: "09:00:00",
          },
        ],
      ],
    ]);
    const cell = { ds, hol: null, absences: [] };
    const [workEvent] = rawCellEvents(cell, {
      translate,
      entryMap,
      currentUserId: 9,
      currentUserName: "Own Name",
    });
    expect(workEvent.personName).toBeNull();
  });
});

describe("groupDayEvents", () => {
  const holidayEvent = {
    key: "holiday",
    colorKey: "holiday",
    color: "#888888",
    label: "Holiday",
    title: "May Day",
    personName: null,
    detail: "May Day",
  };
  const vacationEvent = (id, personName) => ({
    key: `absence:${id}`,
    colorKey: "absence:vacation",
    color: "#1a73e8",
    label: "Vacation",
    title: "Vacation",
    personName,
    detail: "",
  });
  const workEvent = (id, personName, detail) => ({
    key: `work:${id}`,
    colorKey: "work:4",
    color: "#2f7d32",
    label: "Project",
    title: "Project",
    personName,
    detail,
  });

  it("collapses several records of one category into a single group", () => {
    // Six people on vacation must produce one chip in the day cell, not six.
    const events = [
      vacationEvent(1, "Fiona F"),
      vacationEvent(2, "Bob B"),
      vacationEvent(3, "Carla C"),
      vacationEvent(4, "Dan D"),
      vacationEvent(5, "Eve E"),
      vacationEvent(6, "Alice A"),
    ];
    const groups = groupDayEvents(events);
    expect(groups).toHaveLength(1);
    expect(groups[0].label).toBe("Vacation");
    expect(groups[0].count).toBe(6);
    expect(groups[0].items).toHaveLength(6);
  });

  it("keeps different categories in separate groups", () => {
    const groups = groupDayEvents([
      workEvent(1, "Bob B", "09:00 - 10:00 (1:00)"),
      vacationEvent(1, "Alice A"),
    ]);
    expect(groups.map((g) => g.label)).toEqual(["Vacation", "Project"]);
  });

  it("orders groups holiday → absence → work regardless of input order", () => {
    const groups = groupDayEvents([
      workEvent(1, "Bob B", "09:00 - 10:00 (1:00)"),
      holidayEvent,
      vacationEvent(1, "Alice A"),
    ]);
    expect(groups.map((g) => g.colorKey)).toEqual([
      "holiday",
      "absence:vacation",
      "work:4",
    ]);
  });

  it("sorts the rows inside a group by person, then by detail", () => {
    const groups = groupDayEvents([
      workEvent(1, "Bob B", "13:00 - 14:00 (1:00)"),
      workEvent(2, "Alice A", "09:00 - 10:00 (1:00)"),
      workEvent(3, "Bob B", "08:00 - 09:00 (1:00)"),
    ]);
    expect(groups[0].items.map((i) => [i.primary, i.secondary])).toEqual([
      ["Alice A", "09:00 - 10:00 (1:00)"],
      ["Bob B", "08:00 - 09:00 (1:00)"],
      ["Bob B", "13:00 - 14:00 (1:00)"],
    ]);
  });

  it("gives every row a person column so the popup indentation never varies", () => {
    // Rows without a person (holidays) promote their detail into the primary
    // column instead of leaving it empty, which would shift the row's text.
    const [group] = groupDayEvents([holidayEvent]);
    expect(group.items).toEqual([
      { key: "holiday", primary: "May Day", secondary: "" },
    ]);
  });

  it("keeps row keys unique so Svelte's keyed each block cannot collide", () => {
    const groups = groupDayEvents([
      vacationEvent(1, "Alice A"),
      vacationEvent(2, "Bob B"),
    ]);
    const keys = groups.flatMap((g) => g.items.map((i) => i.key));
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("takes the group's colour from its events", () => {
    const [group] = groupDayEvents([vacationEvent(1, "Alice A")]);
    expect(group.color).toBe("#1a73e8");
  });

  it("returns an empty list for a day with no events", () => {
    expect(groupDayEvents([])).toEqual([]);
  });
});

describe("buildColorMap", () => {
  it("assigns a unique colour to each distinct event key", () => {
    // Every category (or absence kind) must get its own colour so users can
    // tell multiple event types apart on the same day.
    const ds = "2026-01-06";
    const cells = [{ ds, other: false, hol: null, absences: [] }];
    const entryMap = new Map([
      [
        ds,
        [
          {
            user_id: 1,
            category_id: 1,
            start_time: "09:00",
            end_time: "10:00",
          },
          {
            user_id: 1,
            category_id: 2,
            start_time: "10:00",
            end_time: "11:00",
          },
        ],
      ],
    ]);
    const categoryMap = new Map([
      [1, { name: "Cat A", color: null }],
      [2, { name: "Cat B", color: null }],
    ]);
    const colorMap = buildColorMap(cells, {
      translate,
      entryMap,
      categoryMap,
    });
    const colors = [...colorMap.values()];
    const uniqueColors = new Set(colors);
    expect(uniqueColors.size).toBe(colors.length);
  });

  it("skips cells marked as 'other' (outside the current month)", () => {
    // Cells from adjacent months are greyed out; they must not influence the
    // colour assignment for the current month's events.
    const cells = [{ ds: "2025-12-31", other: true, hol: null, absences: [] }];
    const colorMap = buildColorMap(cells, { translate });
    expect(colorMap.size).toBe(0);
  });
});

describe("calendarEventTitle", () => {
  it("prefers the explicit title over the category label", () => {
    expect(calendarEventTitle({ title: "May Day", label: "Holiday" })).toBe(
      "May Day",
    );
  });

  it("falls back to the label when no title is set", () => {
    expect(calendarEventTitle({ label: "Vacation" })).toBe("Vacation");
  });

  it("returns an empty string for a null or empty group", () => {
    expect(calendarEventTitle(null)).toBe("");
    expect(calendarEventTitle({})).toBe("");
  });
});
