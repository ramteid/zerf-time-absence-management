import { describe, expect, it } from "vitest";
import {
  buildWeekDays,
  computeDayBreakDeduction,
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

describe("computeDayBreakDeduction", () => {
  // Helper: build a minimal time entry object.
  function entry(startTime, endTime, opts = {}) {
    return {
      id: Math.random(),
      start_time: startTime,
      end_time: endTime,
      status: opts.status ?? "approved",
      category_id: opts.category_id ?? 1,
      counts_as_work: opts.counts_as_work,
    };
  }
  // A category that counts as work.
  const workCat = [{ id: 1, counts_as_work: true }];
  // A category that does NOT count as work.
  const nonWorkCat = [{ id: 1, counts_as_work: false }];

  it("returns 0 when no items are provided", () => {
    expect(computeDayBreakDeduction([], workCat, 6, 30)).toBe(0);
    expect(computeDayBreakDeduction(null, workCat, 6, 30)).toBe(0);
  });

  it("returns 0 when threshold or deduction is missing/zero", () => {
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, workCat, 0, 30)).toBe(0);
    expect(computeDayBreakDeduction(items, workCat, 6, 0)).toBe(0);
    expect(computeDayBreakDeduction(items, workCat, null, 30)).toBe(0);
  });

  it("deducts once when a single block meets the threshold", () => {
    // 7-hour continuous block, threshold 6 h → one 30-minute deduction.
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(30);
  });

  it("does not deduct when block is shorter than the threshold", () => {
    // 5-hour block, threshold 6 h → no deduction.
    const items = [entry("08:00", "13:00")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(0);
  });

  it("treats adjacent entries as one continuous block", () => {
    // 3 h + 3 h with end == start of next → 6-hour block, deduction triggered.
    const items = [entry("08:00", "11:00"), entry("11:00", "14:00")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(30);
  });

  it("treats a one-minute gap as two separate blocks", () => {
    // 3 h + 3 h with a 1-minute gap → each block is 3 h, neither triggers.
    const items = [entry("08:00", "11:00"), entry("11:01", "14:01")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(0);
  });

  it("merges overlapping entries into one block", () => {
    // 08:00–14:00 and 10:00–16:00 → one block 08:00–16:00 (8 h), deduction triggered.
    const items = [entry("08:00", "14:00"), entry("10:00", "16:00")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(30);
  });

  it("deducts once per qualifying block independently", () => {
    // Morning: 08:00–14:30 (6.5 h, qualifies). Gap. Afternoon: 15:00–21:30 (6.5 h, qualifies).
    const items = [
      entry("08:00", "14:30"),
      entry("15:00", "21:30"),
    ];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(60);
  });

  it("excludes rejected entries from block computation", () => {
    // Only the second 3-hour entry is valid; rejected entry is ignored.
    // Total valid work = 3 h, below 6-hour threshold.
    const items = [
      entry("08:00", "11:00", { status: "rejected" }),
      entry("11:00", "14:00"),
    ];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(0);
  });

  it("excludes non-crediting entries from block computation", () => {
    // Category does not count as work → no block → no deduction.
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, nonWorkCat, 6, 30)).toBe(0);
  });

  it("respects entry-level counts_as_work override over category", () => {
    // Entry explicitly not-crediting, category says work → entry wins → no deduction.
    const items = [entry("08:00", "15:00", { counts_as_work: false })];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(0);
  });

  it("handles HH:MM:SS format time strings", () => {
    const items = [entry("08:00:00", "15:00:00")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(30);
  });

  it("deducts when block duration equals the threshold exactly", () => {
    // 6.0-hour block, threshold 6 → one deduction (>= check, not >).
    const items = [entry("08:00", "14:00")];
    expect(computeDayBreakDeduction(items, workCat, 6, 30)).toBe(30);
  });
});
