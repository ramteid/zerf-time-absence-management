import { describe, expect, it } from "vitest";
import {
  buildWeekDays,
  creditedEntryMinutes,
  filterWeekAbsences,
  weekStatus,
  weekTargetMinutes,
} from "./time.js";

describe("time domain helpers", () => {
  it("filters invalid week absences and deduplicates cross-year loads", () => {
    const rows = filterWeekAbsences(
      [
        [
          {
            id: 1,
            start_date: "2026-01-01",
            end_date: "2026-01-02",
            status: "approved",
          },
          {
            id: 2,
            start_date: "2026-01-01",
            end_date: "2026-01-02",
            status: "cancelled",
          },
        ],
        [
          {
            id: 1,
            start_date: "2026-01-01",
            end_date: "2026-01-02",
            status: "approved",
          },
          {
            id: 3,
            start_date: "2025-12-01",
            end_date: "2025-12-02",
            status: "approved",
          },
        ],
      ],
      "2026-01-01",
      "2026-01-07",
    );

    expect(rows.map((row) => row.id)).toEqual([1]);
  });

  it("uses entry counts_as_work before category fallback", () => {
    expect(
      creditedEntryMinutes(
        {
          start_time: "09:00:00",
          end_time: "10:30:00",
          counts_as_work: true,
          category_id: 1,
          status: "draft",
        },
        [{ id: 1, counts_as_work: false }],
      ),
    ).toBe(90);

    expect(
      creditedEntryMinutes(
        {
          start_time: "09:00:00",
          end_time: "10:30:00",
          counts_as_work: false,
          category_id: 1,
          status: "draft",
        },
        [{ id: 1, counts_as_work: true }],
      ),
    ).toBe(0);
  });

  it("builds target minutes from eligible contract days only", () => {
    const { weekdays, weekendDays } = buildWeekDays(
      new Date(2026, 0, 5),
      [],
      [
        {
          id: 1,
          start_date: "2026-01-06",
          end_date: "2026-01-06",
          status: "approved",
          kind: "vacation",
        },
      ],
      [{ holiday_date: "2026-01-07", name: "Holiday" }],
    );

    expect(
      weekTargetMinutes({
        weekdays,
        weekendDays,
        currentUser: { weekly_hours: 40, workdays_per_week: 5 },
        todayIso: "2026-01-09",
      }),
    ).toBe(3 * 8 * 60);
  });

  it("keeps partial status for mixed draft and non-draft weeks", () => {
    const entries = [{ status: "draft" }, { status: "approved" }];
    expect(
      weekStatus(
        entries,
        entries.filter((entry) => entry.status === "draft"),
      ),
    ).toBe("partial");
  });
});
