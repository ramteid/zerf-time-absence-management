import { describe, expect, it } from "vitest";
import {
  countWorkdays,
  holidayDateSet,
  normalizeMonthReport,
} from "./apiMappers.js";

describe("holidayDateSet", () => {
  it("creates a Set of date strings", () => {
    const set = holidayDateSet([
      { holiday_date: "2026-05-01" },
      { holiday_date: "2026-12-25" },
    ]);
    expect(set).toBeInstanceOf(Set);
    expect(set.has("2026-05-01")).toBe(true);
    expect(set.has("2026-12-25")).toBe(true);
    expect(set.size).toBe(2);
  });

  it("returns empty set for no input", () => {
    expect(holidayDateSet().size).toBe(0);
    expect(holidayDateSet([]).size).toBe(0);
  });
});

describe("countWorkdays", () => {
  it("counts weekdays in a range", () => {
    // Mon-Fri = 5 workdays
    expect(countWorkdays("2026-05-04", "2026-05-08")).toBe(5);
  });

  it("skips weekends", () => {
    // Mon-Sun (7 calendar days, 5 workdays)
    expect(countWorkdays("2026-05-04", "2026-05-10")).toBe(5);
  });

  it("skips holidays", () => {
    const holidays = holidayDateSet([{ holiday_date: "2026-05-01" }]);
    // Thu Apr 30 - Mon May 4: workdays are Apr 30, May 4 (May 1 is holiday, May 2-3 weekend)
    expect(countWorkdays("2026-04-30", "2026-05-04", false, holidays)).toBe(2);
  });

  it("returns 1 for a single weekday", () => {
    expect(countWorkdays("2026-05-04", "2026-05-04")).toBe(1);
  });

  it("returns 0 for a single weekend day", () => {
    expect(countWorkdays("2026-05-09", "2026-05-09")).toBe(0); // Saturday
  });

  it("supports half-day for single day", () => {
    expect(countWorkdays("2026-05-04", "2026-05-04", true)).toBe(0.5);
  });

  it("half-day flag has no effect on multi-day ranges", () => {
    expect(countWorkdays("2026-05-04", "2026-05-05", true)).toBe(2);
  });

  it("half-day on weekend returns 0", () => {
    expect(countWorkdays("2026-05-09", "2026-05-09", true)).toBe(0);
  });

  it("returns 0 when end < start", () => {
    expect(countWorkdays("2026-05-10", "2026-05-04")).toBe(0);
  });

  it("returns 0 for invalid dates", () => {
    expect(countWorkdays("invalid", "2026-05-04")).toBe(0);
  });
});

describe("normalizeMonthReport", () => {
  it("returns input unchanged for null/missing days", () => {
    expect(normalizeMonthReport(null)).toBe(null);
    expect(normalizeMonthReport({ other: 1 })).toEqual({ other: 1 });
  });

  it("flattens entries from days into top-level array", () => {
    const report = {
      days: [
        {
          date: "2026-05-04",
          weekday: "Monday",
          holiday: null,
          absence: null,
          entries: [
            {
              start_time: "08:00",
              end_time: "12:00",
              minutes: 240,
              category: "Dev",
              status: "approved",
              comment: "Work",
            },
            {
              start_time: "13:00",
              end_time: "17:00",
              minutes: 240,
              category: "Dev",
              status: "pending",
              comment: null,
            },
          ],
        },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.entries).toHaveLength(2);
    expect(result.entries[0]).toEqual({
      entry_date: "2026-05-04",
      start_time: "08:00",
      end_time: "12:00",
      minutes: 240,
      category_name: "Dev",
      status: "approved",
      comment: "Work",
    });
    expect(result.absences).toEqual([]);
  });

  it("groups consecutive same-kind absences into spans", () => {
    const report = {
      days: [
        { date: "2026-05-04", weekday: "Monday", holiday: null, absence: "vacation", entries: [] },
        { date: "2026-05-05", weekday: "Tuesday", holiday: null, absence: "vacation", entries: [] },
        { date: "2026-05-06", weekday: "Wednesday", holiday: null, absence: null, entries: [] },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toEqual([
      { kind: "vacation", start_date: "2026-05-04", end_date: "2026-05-05", days: 2 },
    ]);
  });

  it("splits different absence kinds into separate spans", () => {
    const report = {
      days: [
        { date: "2026-05-04", weekday: "Monday", holiday: null, absence: "vacation", entries: [] },
        { date: "2026-05-05", weekday: "Tuesday", holiday: null, absence: "sick", entries: [] },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toHaveLength(2);
    expect(result.absences[0].kind).toBe("vacation");
    expect(result.absences[1].kind).toBe("sick");
  });

  it("does not count weekends or holidays toward absence days", () => {
    const report = {
      days: [
        { date: "2026-05-01", weekday: "Friday", holiday: "Labour Day", absence: "vacation", entries: [] },
        { date: "2026-05-02", weekday: "Saturday", holiday: null, absence: "vacation", entries: [] },
        { date: "2026-05-03", weekday: "Sunday", holiday: null, absence: "vacation", entries: [] },
        { date: "2026-05-04", weekday: "Monday", holiday: null, absence: "vacation", entries: [] },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toEqual([
      { kind: "vacation", start_date: "2026-05-01", end_date: "2026-05-04", days: 1 },
    ]);
  });

  it("handles report with empty days array", () => {
    const result = normalizeMonthReport({ days: [] });
    expect(result.entries).toEqual([]);
    expect(result.absences).toEqual([]);
  });

  it("flushes trailing absence at end of days", () => {
    const report = {
      days: [
        { date: "2026-05-04", weekday: "Monday", holiday: null, absence: "sick", entries: [] },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toEqual([
      { kind: "sick", start_date: "2026-05-04", end_date: "2026-05-04", days: 1 },
    ]);
  });
});
