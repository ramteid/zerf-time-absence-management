import { describe, expect, it } from "vitest";
import {
  buildBreakRules,
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

describe("buildBreakRules", () => {
  it("returns empty array when feature is disabled", () => {
    expect(buildBreakRules({ auto_break_enabled: false })).toEqual([]);
    expect(buildBreakRules(null)).toEqual([]);
    expect(buildBreakRules({})).toEqual([]);
  });

  it("returns single rule when only tier 1 is configured", () => {
    const rules = buildBreakRules({
      auto_break_enabled: true,
      auto_break_threshold_hours: 6,
      auto_break_deduction_minutes: 30,
    });
    expect(rules).toEqual([{ thresholdHours: 6, deductionMinutes: 30 }]);
  });

  it("returns two rules sorted ascending when both tiers are configured", () => {
    const rules = buildBreakRules({
      auto_break_enabled: true,
      auto_break_threshold_hours: 6,
      auto_break_deduction_minutes: 30,
      auto_break_threshold_hours_2: 9,
      auto_break_deduction_minutes_2: 45,
    });
    expect(rules).toEqual([
      { thresholdHours: 6, deductionMinutes: 30 },
      { thresholdHours: 9, deductionMinutes: 45 },
    ]);
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
  // Convenience single-tier rule set.
  const rules1 = [{ thresholdHours: 6, deductionMinutes: 30 }];
  // Two-tier rules (German law: 6 h → 30 min, 9 h → 45 min total).
  const rules2 = [
    { thresholdHours: 6, deductionMinutes: 30 },
    { thresholdHours: 9, deductionMinutes: 45 },
  ];

  it("returns 0 when no items are provided", () => {
    expect(computeDayBreakDeduction([], workCat, rules1)).toBe(0);
    expect(computeDayBreakDeduction(null, workCat, rules1)).toBe(0);
  });

  it("returns 0 when rules array is empty or missing", () => {
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, workCat, [])).toBe(0);
    expect(computeDayBreakDeduction(items, workCat, null)).toBe(0);
  });

  it("deducts once when a single block meets the threshold", () => {
    // 7-hour continuous block, threshold 6 h → one 30-minute deduction.
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(30);
  });

  it("does not deduct when block is shorter than the threshold", () => {
    // 5-hour block, threshold 6 h → no deduction.
    const items = [entry("08:00", "13:00")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(0);
  });

  it("treats adjacent entries as one continuous block", () => {
    // 3 h + 3 h with end == start of next → 6-hour block, deduction triggered.
    const items = [entry("08:00", "11:00"), entry("11:00", "14:00")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(30);
  });

  it("treats a one-minute gap as two separate blocks", () => {
    // 3 h + 3 h with a 1-minute gap → each block is 3 h, neither triggers.
    const items = [entry("08:00", "11:00"), entry("11:01", "14:01")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(0);
  });

  it("merges overlapping entries into one block", () => {
    // 08:00–14:00 and 10:00–16:00 → one block 08:00–16:00 (8 h), deduction triggered.
    const items = [entry("08:00", "14:00"), entry("10:00", "16:00")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(30);
  });

  it("deducts once per qualifying block independently", () => {
    // Morning: 08:00–14:30 (6.5 h). Gap. Afternoon: 15:00–21:30 (6.5 h). Two blocks.
    const items = [entry("08:00", "14:30"), entry("15:00", "21:30")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(60);
  });

  it("excludes rejected entries from block computation", () => {
    const items = [
      entry("08:00", "11:00", { status: "rejected" }),
      entry("11:00", "14:00"),
    ];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(0);
  });

  it("excludes non-crediting entries from block computation", () => {
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, nonWorkCat, rules1)).toBe(0);
  });

  it("respects entry-level counts_as_work override over category", () => {
    const items = [entry("08:00", "15:00", { counts_as_work: false })];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(0);
  });

  it("handles HH:MM:SS format time strings", () => {
    const items = [entry("08:00:00", "15:00:00")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(30);
  });

  it("deducts when block duration equals the threshold exactly", () => {
    const items = [entry("08:00", "14:00")];
    expect(computeDayBreakDeduction(items, workCat, rules1)).toBe(30);
  });

  it("two-tier: applies tier-2 deduction (not cumulative) for long block", () => {
    // 10 h block → tier 2 (9 h) applies → 45 min total, NOT 30 + 45 = 75
    const items = [entry("08:00", "18:00")];
    expect(computeDayBreakDeduction(items, workCat, rules2)).toBe(45);
  });

  it("two-tier: applies tier-1 deduction when below tier-2 threshold", () => {
    // 7 h block → only tier 1 (6 h) applies → 30 min
    const items = [entry("08:00", "15:00")];
    expect(computeDayBreakDeduction(items, workCat, rules2)).toBe(30);
  });

  it("two-tier: no deduction when below both thresholds", () => {
    const items = [entry("08:00", "13:00")];
    expect(computeDayBreakDeduction(items, workCat, rules2)).toBe(0);
  });

  it("two-tier: each block applies its own highest rule independently", () => {
    // Block 1 (10 h) → tier 2 → 45 min. Block 2 (7 h) → tier 1 → 30 min. Total = 75.
    const items = [entry("00:00", "10:00"), entry("11:00", "18:00")];
    expect(computeDayBreakDeduction(items, workCat, rules2)).toBe(75);
  });
});
