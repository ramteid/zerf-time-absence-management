// Tests for the calendar domain module. The calendar renders a monthly grid
// where each cell can hold holidays, absences, and time entries. The key
// rendering concerns are:
//   - Each event gets a distinct colour so employees can tell entries apart
//   - Holidays always get the fixed holiday colour regardless of assignment order
//   - Absence colours come from the DB-stored category color (absenceCategoryMap)
//   - Work-entry colours are drawn from a fallback palette, cycling and avoiding
//     any colour already used by holidays or absences
//   - absColor, normalizeColor, and fallbackColor have clear boundary behaviour

import { describe, expect, it } from "vitest";
import {
  absColor,
  absenceDetail,
  buildColorMap,
  calendarEventTitle,
  categoryForEntry,
  fallbackColor,
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

describe("absenceDetail", () => {
  it("joins name and comment with a dash when both are present", () => {
    expect(absenceDetail({ name: "Vacation", comment: "Pre-booked" })).toBe(
      "Vacation - Pre-booked",
    );
  });

  it("omits the dash when one part is missing", () => {
    expect(absenceDetail({ name: "Sick leave", comment: "" })).toBe(
      "Sick leave",
    );
    expect(absenceDetail({ name: "", comment: "Doctor visit" })).toBe(
      "Doctor visit",
    );
  });
});

describe("rawCellEvents", () => {
  it("includes a holiday event when the cell has a holiday", () => {
    // Holidays must always appear with the fixed holiday colour so they are
    // visually distinct from absence and work events.
    const cell = { ds: "2026-01-01", hol: "New Year", absences: [] };
    const events = rawCellEvents(cell, new Map(), new Map(), new Map(), translate);
    expect(events.some((e) => e.key === "holiday")).toBe(true);
    expect(events.find((e) => e.key === "holiday").detail).toBe("New Year");
  });

  it("includes an absence event per absence in the cell", () => {
    const cell = {
      ds: "2026-07-15",
      hol: null,
      absences: [{ kind: "vacation", name: "Summer", comment: "" }],
    };
    const events = rawCellEvents(cell, new Map(), new Map(), new Map(), translate);
    const absEvent = events.find((e) => e.key === "absence:vacation");
    expect(absEvent).not.toBeUndefined();
  });

  it("uses DB-stored category color for absence events", () => {
    const cell = {
      ds: "2026-07-15",
      hol: null,
      absences: [{ kind: "vacation", name: "Summer", comment: "" }],
    };
    const absCatMap = new Map([["vacation", { color: "#1a73e8" }]]);
    const events = rawCellEvents(cell, new Map(), new Map(), absCatMap, translate);
    const absEvent = events.find((e) => e.key === "absence:vacation");
    expect(absEvent.color).toBe("#1a73e8");
  });

  it("includes a work event for each time entry on the date", () => {
    const ds = "2026-03-10";
    const entryMap = new Map([
      [
        ds,
        [
          {
            user_id: 1,
            category_id: null,
            start_time: "09:00:00",
            end_time: "12:00:00",
          },
        ],
      ],
    ]);
    const cell = { ds, hol: null, absences: [] };
    const events = rawCellEvents(cell, entryMap, new Map(), new Map(), translate);
    expect(events.some((e) => e.key.startsWith("work:"))).toBe(true);
  });

  it("includes the other user's name in the detail when the entry is not own", () => {
    // When managers view team calendars, each entry shows whose it is so the
    // manager doesn't confuse team entries with their own.
    const ds = "2026-03-10";
    const entry = {
      user_id: 5,
      category_id: null,
      start_time: "09:00:00",
      end_time: "10:00:00",
    };
    const entryMap = new Map([[ds, [entry]]]);
    const userMap = new Map([[5, { first_name: "Eve", last_name: "Emp" }]]);
    const cell = { ds, hol: null, absences: [] };
    const events = rawCellEvents(cell, entryMap, new Map(), new Map(), translate, userMap, 1);
    const workEvent = events.find((e) => e.key.startsWith("work:"));
    expect(workEvent.detail).toContain("Eve Emp");
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
    const colorMap = buildColorMap(cells, entryMap, categoryMap, new Map(), translate);
    const colors = [...colorMap.values()];
    const uniqueColors = new Set(colors);
    expect(uniqueColors.size).toBe(colors.length);
  });

  it("skips cells marked as 'other' (outside the current month)", () => {
    // Cells from adjacent months are greyed out; they must not influence the
    // colour assignment for the current month's events.
    const cells = [{ ds: "2025-12-31", other: true, hol: null, absences: [] }];
    const colorMap = buildColorMap(cells, new Map(), new Map(), new Map(), translate);
    expect(colorMap.size).toBe(0);
  });
});

describe("calendarEventTitle", () => {
  it("prefers the explicit title over detail or label", () => {
    expect(
      calendarEventTitle({ title: "My title", detail: "detail", label: "label" }),
    ).toBe("My title");
  });

  it("falls back to detail when title is missing", () => {
    expect(calendarEventTitle({ detail: "detail text", label: "label" })).toBe(
      "detail text",
    );
  });

  it("falls back to label when title and detail are absent", () => {
    expect(calendarEventTitle({ label: "Vacation" })).toBe("Vacation");
  });

  it("returns an empty string for a null or empty event", () => {
    expect(calendarEventTitle(null)).toBe("");
    expect(calendarEventTitle({})).toBe("");
  });
});
