import { describe, it, expect } from "vitest";
import {
  parseDate,
  isoDate,
  dateKey,
  monday,
  addDays,
  minToHM,
  durMin,
  isoWeek,
} from "./format.js";

describe("parseDate", () => {
  it("parses ISO date strings without timezone shift", () => {
    const d = parseDate("2024-03-11");
    expect(d.getFullYear()).toBe(2024);
    expect(d.getMonth()).toBe(2); // March = 2
    expect(d.getDate()).toBe(11);
  });

  it("clones Date objects", () => {
    const original = new Date(2024, 5, 15);
    const parsed = parseDate(original);
    parsed.setDate(1);
    expect(original.getDate()).toBe(15); // original unchanged
  });

  it("passes through timestamps", () => {
    const ts = new Date(2024, 0, 1).getTime();
    const d = parseDate(ts);
    expect(d.getFullYear()).toBe(2024);
  });
});

describe("isoDate", () => {
  it("formats Date as YYYY-MM-DD", () => {
    expect(isoDate(new Date(2024, 0, 5))).toBe("2024-01-05");
  });

  it("pads single-digit months and days", () => {
    expect(isoDate(new Date(2024, 2, 3))).toBe("2024-03-03");
  });

  it("round-trips ISO date strings", () => {
    expect(isoDate("2024-03-11")).toBe("2024-03-11");
    expect(isoDate("2024-12-31")).toBe("2024-12-31");
  });
});

describe("dateKey", () => {
  it("keeps plain ISO dates unchanged", () => {
    expect(dateKey("2026-05-01")).toBe("2026-05-01");
  });

  it("extracts the calendar date from ISO date-time strings", () => {
    expect(dateKey("2026-05-01T00:00:00Z")).toBe("2026-05-01");
    expect(dateKey("2026-05-01 08:15:00")).toBe("2026-05-01");
  });

  it("formats Date instances from local date parts", () => {
    expect(dateKey(new Date(2026, 4, 1, 23, 59))).toBe("2026-05-01");
  });
});

describe("monday", () => {
  it("returns the same day when given a Monday", () => {
    // 2024-03-11 is a Monday
    expect(isoDate(monday(new Date(2024, 2, 11)))).toBe("2024-03-11");
  });

  it("rounds Wednesday back to Monday", () => {
    expect(isoDate(monday(new Date(2024, 2, 13)))).toBe("2024-03-11");
  });

  it("rounds Sunday back to Monday of that week", () => {
    // 2024-03-17 is a Sunday → Monday is 2024-03-11
    expect(isoDate(monday(new Date(2024, 2, 17)))).toBe("2024-03-11");
  });

  it("handles Saturday", () => {
    // 2024-03-16 is a Saturday → Monday is 2024-03-11
    expect(isoDate(monday(new Date(2024, 2, 16)))).toBe("2024-03-11");
  });
});

describe("addDays", () => {
  it("adds positive days", () => {
    expect(isoDate(addDays(new Date(2024, 0, 1), 5))).toBe("2024-01-06");
  });

  it("subtracts with negative days", () => {
    expect(isoDate(addDays(new Date(2024, 0, 10), -3))).toBe("2024-01-07");
  });

  it("crosses month boundaries", () => {
    expect(isoDate(addDays(new Date(2024, 0, 31), 1))).toBe("2024-02-01");
  });

  it("crosses year boundaries", () => {
    expect(isoDate(addDays(new Date(2024, 11, 31), 1))).toBe("2025-01-01");
  });
});

describe("minToHM", () => {
  it("formats zero", () => {
    expect(minToHM(0)).toBe("0:00");
  });

  it("formats positive minutes", () => {
    expect(minToHM(125)).toBe("2:05");
    expect(minToHM(60)).toBe("1:00");
    expect(minToHM(9)).toBe("0:09");
  });

  it("formats negative minutes with sign", () => {
    expect(minToHM(-30)).toBe("-0:30");
    expect(minToHM(-90)).toBe("-1:30");
  });
});

describe("durMin", () => {
  it("computes duration between two times", () => {
    expect(durMin("08:00", "12:30")).toBe(270);
  });

  it("handles same start and end", () => {
    expect(durMin("09:00", "09:00")).toBe(0);
  });

  it("returns negative for reversed times", () => {
    expect(durMin("12:00", "08:00")).toBe(-240);
  });
});

describe("isoWeek", () => {
  it("returns week 1 for Jan 1, 2024", () => {
    expect(isoWeek(new Date(2024, 0, 1))).toBe(1);
  });

  it("returns week 1 for Dec 30, 2024 (ISO week belongs to 2025-W01)", () => {
    expect(isoWeek(new Date(2024, 11, 30))).toBe(1);
  });

  it("returns correct mid-year week", () => {
    // 2024-07-01 is in week 27
    expect(isoWeek(new Date(2024, 6, 1))).toBe(27);
  });

  it("handles week 53 years", () => {
    // 2020-12-31 is Thursday in ISO week 53
    expect(isoWeek(new Date(2020, 11, 31))).toBe(53);
  });
});
