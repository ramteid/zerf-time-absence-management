import { describe, expect, it } from "vitest";

import {
  countWorkdays,
  holidayDateSet,
  normalizeMonthReport,
} from "./apiMappers.js";

describe("countWorkdays", () => {
  it("skips weekends and holidays", () => {
    const holidays = holidayDateSet([{ holiday_date: "2026-05-01" }]);

    expect(countWorkdays("2026-04-30", "2026-05-04", false, holidays)).toBe(2);
  });

  it("supports single-day half days", () => {
    expect(countWorkdays("2026-05-04", "2026-05-04", true)).toBe(0.5);
  });
});

describe("normalizeMonthReport", () => {
  it("flattens entries and groups absence spans", () => {
    const report = {
      days: [
        {
          date: "2026-05-01",
          weekday: "Friday",
          holiday: "Labour Day",
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-02",
          weekday: "Saturday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-03",
          weekday: "Sunday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-04",
          weekday: "Monday",
          holiday: null,
          absence: "vacation",
          entries: [
            {
              start_time: "08:00",
              end_time: "12:00",
              minutes: 240,
              category: "Planning",
              status: "approved",
              comment: "Prep",
            },
          ],
        },
        {
          date: "2026-05-05",
          weekday: "Tuesday",
          holiday: null,
          absence: null,
          entries: [],
        },
      ],
    };

    expect(normalizeMonthReport(report)).toEqual({
      ...report,
      entries: [
        {
          entry_date: "2026-05-04",
          start_time: "08:00",
          end_time: "12:00",
          minutes: 240,
          category_name: "Planning",
          status: "approved",
          comment: "Prep",
        },
      ],
      absences: [
        {
          kind: "vacation",
          start_date: "2026-05-01",
          end_date: "2026-05-04",
          days: 1,
        },
      ],
    });
  });
});