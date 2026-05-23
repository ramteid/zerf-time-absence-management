import { describe, expect, it } from "vitest";
import {
  allMonthsToCheck,
  buildPendingWeeks,
  buildSubmissionChecks,
  currentWeekIsOpen,
  notificationTarget,
} from "./dashboard.js";

describe("dashboard domain helpers", () => {
  it("builds month keys from user start through current month", () => {
    expect(allMonthsToCheck("2025-11-15", new Date(2026, 1, 10))).toEqual([
      "2025-11",
      "2025-12",
      "2026-01",
      "2026-02",
    ]);
  });

  it("maps backend submission flags into dashboard checks", () => {
    expect(
      buildSubmissionChecks(
        ["2026-01"],
        [
          {
            weeks_all_submitted: true,
            weeks_all_approved: false,
            current_week_status: "draft",
          },
        ],
      ),
    ).toEqual([
      {
        month: "2026-01",
        submitted: true,
        approved: false,
        currentWeekStatus: "draft",
      },
    ]);
  });

  it("treats draft, partial, and rejected as an open current week", () => {
    for (const status of ["draft", "partial", "rejected"]) {
      expect(currentWeekIsOpen([{ currentWeekStatus: status }])).toBe(true);
    }
  });

  it("treats submitted and approved current weeks as closed", () => {
    for (const status of ["submitted", "approved", null]) {
      expect(currentWeekIsOpen([{ currentWeekStatus: status }])).toBe(false);
    }
    expect(currentWeekIsOpen([])).toBe(false);
  });

  it("groups pending entries by user and week with category work rules", () => {
    const weeks = buildPendingWeeks(
      [
        {
          id: 1,
          user_id: 7,
          entry_date: "2026-01-06",
          start_time: "09:00:00",
          end_time: "10:00:00",
          category_id: 1,
        },
        {
          id: 2,
          user_id: 7,
          entry_date: "2026-01-07",
          start_time: "09:00:00",
          end_time: "10:00:00",
          category_id: 2,
        },
      ],
      [{ id: 7, first_name: "Ada", last_name: "Lead" }],
      [
        { id: 1, counts_as_work: true },
        { id: 2, counts_as_work: false },
      ],
    );

    expect(weeks).toHaveLength(1);
    expect(weeks[0].week_start).toBe("2026-01-05");
    expect(weeks[0].total_min).toBe(60);
  });

  it("routes notification references to dashboard focus targets", () => {
    expect(
      notificationTarget(
        { id: 3, kind: "absence_requested", reference_type: "absences" },
        10,
      ),
    ).toBe("/dashboard?focus=absences&n=3-10");
  });
});
