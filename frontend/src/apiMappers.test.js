import { describe, expect, it } from "vitest";
import {
  countedWorkdays,
  countWorkdays,
  holidayDateSet,
  normalizeMonthReport,
  withAbsenceDays,
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
    expect(countWorkdays("2026-04-30", "2026-05-04", holidays)).toBe(2);
  });

  it("returns 1 for a single weekday", () => {
    expect(countWorkdays("2026-05-04", "2026-05-04")).toBe(1);
  });

  it("returns 0 for a single weekend day", () => {
    expect(countWorkdays("2026-05-09", "2026-05-09")).toBe(0); // Saturday
  });

  it("returns 0 when end < start", () => {
    expect(countWorkdays("2026-05-10", "2026-05-04")).toBe(0);
  });

  it("returns 0 for invalid dates", () => {
    expect(countWorkdays("invalid", "2026-05-04")).toBe(0);
  });

  it("respects custom workdays_per_week for 4-day schedules", () => {
    // Mon-Fri contains 5 potential days, capped to 4 configured days.
    expect(countWorkdays("2026-05-04", "2026-05-08", new Set(), 4)).toBe(4);
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
        days: 2,
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

  it("does not count weekends or holidays toward absence days", () => {
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
          entries: [],
        },
      ],
    };

    const result = normalizeMonthReport(report);
    expect(result.absences).toEqual([
      {
        kind: "vacation",
        start_date: "2026-05-01",
        end_date: "2026-05-04",
        days: 1,
      },
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
        days: 1,
      },
    ]);
  });

  it("respects custom workdays_per_week for absence aggregation", () => {
    const report = {
      days: [
        {
          date: "2026-05-08",
          weekday: "Friday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
        {
          date: "2026-05-09",
          weekday: "Saturday",
          holiday: null,
          absence: "vacation",
          entries: [],
        },
      ],
    };

    const result = normalizeMonthReport(report, 4);
    expect(result.absences).toEqual([
      {
        kind: "vacation",
        start_date: "2026-05-08",
        end_date: "2026-05-09",
        days: 1,
      },
    ]);
  });
});

describe("countedWorkdays", () => {
  // 2026-05-04 is a Monday.
  it("charges one week's quota once across several ranges", () => {
    const split = countedWorkdays(
      [
        ["2026-05-04", "2026-05-05"],
        ["2026-05-06", "2026-05-08"],
      ],
      "2026-05-04",
      "2026-05-08",
      new Set(),
      3,
    );
    expect(split).toEqual(["2026-05-04", "2026-05-05", "2026-05-06"]);
    // ... which is exactly what the same week booked as one range costs.
    expect(countWorkdays("2026-05-04", "2026-05-08", new Set(), 3)).toBe(3);
  });

  it("ignores the part of a range that falls outside the window", () => {
    expect(
      countedWorkdays(
        [["2026-04-27", "2026-05-15"]],
        "2026-05-04",
        "2026-05-05",
        new Set(),
        5,
      ),
    ).toEqual(["2026-05-04", "2026-05-05"]);
  });

  it("counts every non-holiday day for an irregular schedule", () => {
    expect(
      countedWorkdays(
        [["2026-05-08", "2026-05-10"]],
        "2026-05-08",
        "2026-05-10",
        new Set(["2026-05-09"]),
        0,
      ),
    ).toEqual(["2026-05-08", "2026-05-10"]);
  });
});

describe("withAbsenceDays", () => {
  // Two bookings inside one calendar week on a three-day contract. Counted
  // separately they cost 2 + 3 = 5 days; the week can never cost more than 3.
  const week = [
    { id: 1, start_date: "2026-05-04", end_date: "2026-05-05" },
    { id: 2, start_date: "2026-05-06", end_date: "2026-05-08" },
  ];

  it("splits one week's quota between the absences that share it", () => {
    const rows = withAbsenceDays(week, {
      from: "2026-05-01",
      to: "2026-05-31",
      workdaysPerWeek: 3,
    });
    expect(rows.map((row) => row.days)).toEqual([2, 1]);
    expect(rows.reduce((total, row) => total + row.days, 0)).toBe(3);
  });

  it("charges the later booking only what the week has left", () => {
    // Order in the input must not change the answer.
    const rows = withAbsenceDays([week[1], week[0]], {
      from: "2026-05-01",
      to: "2026-05-31",
      workdaysPerWeek: 3,
    });
    expect(rows.map((row) => ({ id: row.id, days: row.days }))).toEqual([
      { id: 2, days: 1 },
      { id: 1, days: 2 },
    ]);
  });

  it("leaves a full-quota week unchanged", () => {
    const rows = withAbsenceDays(week, {
      from: "2026-05-01",
      to: "2026-05-31",
      workdaysPerWeek: 5,
    });
    expect(rows.map((row) => row.days)).toEqual([2, 3]);
  });

  it("clamps each absence to the window", () => {
    const rows = withAbsenceDays(
      [{ id: 1, start_date: "2026-04-27", end_date: "2026-05-08" }],
      { from: "2026-05-04", to: "2026-05-05", workdaysPerWeek: 5 },
    );
    expect(rows[0].days).toBe(2);
  });
});
