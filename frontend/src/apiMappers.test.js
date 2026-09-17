import { describe, expect, it } from "vitest";
import { normalizeMonthReport } from "./apiMappers.js";

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
        {
          date: "2026-05-04",
          weekday: "Monday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-05",
          weekday: "Tuesday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-06",
          weekday: "Wednesday",
          holiday: null,
          absence: null,
          entries: [],
        },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toEqual([
      {
        kind: "vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
      },
    ]);
  });

  it("splits different absence kinds into separate spans", () => {
    const report = {
      days: [
        {
          date: "2026-05-04",
          weekday: "Monday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-05",
          weekday: "Tuesday",
          holiday: null,
          absence: "sick",
          entries: [],
        },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toHaveLength(2);
    expect(result.absences[0].kind).toBe("vacation");
    expect(result.absences[1].kind).toBe("sick");
  });

  it("handles report with empty days array", () => {
    const result = normalizeMonthReport({ days: [] });
    expect(result.entries).toEqual([]);
    expect(result.absences).toEqual([]);
  });

  it("flushes trailing absence at end of days", () => {
    const report = {
      days: [
        {
          date: "2026-05-04",
          weekday: "Monday",
          holiday: null,
          absence: "sick",
          entries: [],
        },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toEqual([
      {
        kind: "sick",
        start_date: "2026-05-04",
        end_date: "2026-05-04",
      },
    ]);
  });
});
