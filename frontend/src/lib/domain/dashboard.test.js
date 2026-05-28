import { describe, expect, it } from "vitest";
import {
  absenceDiffRows,
  absenceRequestTypeLabelKey,
  allMonthsToCheck,
  buildPendingWeeks,
  buildSubmissionChecks,
  currentWeekIsOpen,
  entryMinutes,
  monthFullySubmitted,
  notificationTarget,
  weekStartOf,
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

  it("routes timesheet_submitted notifications to the timesheets focus", () => {
    expect(
      notificationTarget({ id: 5, kind: "timesheet_submitted" }, 0),
    ).toContain("focus=timesheets");
  });

  it("routes reopen_request_created notifications to the reopen focus", () => {
    expect(
      notificationTarget({ id: 6, kind: "reopen_request_created" }, 0),
    ).toContain("focus=reopen");
  });

  it("routes submission_reminder notifications to the dashboard without a focus", () => {
    expect(
      notificationTarget({ id: 7, kind: "submission_reminder" }, 0),
    ).not.toContain("focus=");
  });

  it("returns empty string for unrecognised notification kinds", () => {
    expect(notificationTarget({ id: 8, kind: "unknown" }, 0)).toBe("");
  });

  it("monthFullySubmitted returns true only when weeks_all_submitted is true", () => {
    expect(monthFullySubmitted({ weeks_all_submitted: true })).toBe(true);
    expect(monthFullySubmitted({ weeks_all_submitted: false })).toBe(false);
    expect(monthFullySubmitted(null)).toBe(false);
  });

  it("allMonthsToCheck returns empty when start is after today", () => {
    // Guards against future start dates (e.g. a pre-created account for a
    // new hire who hasn't started yet).
    expect(allMonthsToCheck("2030-01-01", new Date(2026, 0, 1))).toEqual([]);
  });

  it("entryMinutes counts credited minutes for a crediting entry", () => {
    const entry = {
      start_time: "09:00:00",
      end_time: "10:30:00",
      category_id: 1,
    };
    const categories = [{ id: 1, counts_as_work: true }];
    expect(entryMinutes(entry, categories)).toBe(90);
  });

  it("entryMinutes returns 0 for entries without start/end time", () => {
    expect(entryMinutes({ category_id: 1 }, [])).toBe(0);
  });

  it("weekStartOf maps an entry date to the Monday of its ISO week", () => {
    // 2026-01-07 is a Wednesday → week start is Monday 2026-01-05.
    expect(weekStartOf("2026-01-07")).toBe("2026-01-05");
  });

  it("weekStartOf returns empty string for invalid dates", () => {
    expect(weekStartOf(undefined)).toBe("");
  });

  it("absenceDiffRows returns empty for non-change review types", () => {
    // Only 'change' review requests have a before/after diff to display.
    // New requests and cancellations show no diff.
    expect(
      absenceDiffRows({ review_type: "new" }, (k) => k),
    ).toEqual([]);
  });

  it("absenceDiffRows detects kind, date, and comment changes", () => {
    const absence = {
      review_type: "change",
      kind: "sick",
      previous_kind: "vacation",
      start_date: "2026-07-01",
      previous_start_date: "2026-07-01",
      end_date: "2026-07-10",
      previous_end_date: "2026-07-05",
      comment: "updated",
      previous_comment: "original",
    };
    const rows = absenceDiffRows(absence, (k) => k);
    const fields = rows.map((r) => r.field);
    expect(fields).toContain("Type");
    expect(fields).toContain("To");
    expect(fields).toContain("Comment");
    expect(fields).not.toContain("From"); // start_date unchanged
  });

  it("absenceRequestTypeLabelKey identifies cancellations correctly", () => {
    expect(
      absenceRequestTypeLabelKey({ status: "cancellation_pending" }),
    ).toBe("Cancellation");
    expect(
      absenceRequestTypeLabelKey({ status: "pending", review_type: "cancellation" }),
    ).toBe("Cancellation");
  });

  it("absenceRequestTypeLabelKey identifies change requests", () => {
    expect(
      absenceRequestTypeLabelKey({ status: "pending", review_type: "change" }),
    ).toBe("Change");
  });

  it("absenceRequestTypeLabelKey defaults to Approval for new requests", () => {
    expect(
      absenceRequestTypeLabelKey({ status: "pending", review_type: "new" }),
    ).toBe("Approval");
  });

  it("buildPendingWeeks returns empty for no submitted entries", () => {
    expect(buildPendingWeeks([], [], [])).toEqual([]);
  });

  it("buildPendingWeeks sorts weeks newest first within the same user", () => {
    const entries = [
      {
        id: 1,
        user_id: 1,
        entry_date: "2026-01-05",
        start_time: "09:00",
        end_time: "10:00",
        category_id: 1,
      },
      {
        id: 2,
        user_id: 1,
        entry_date: "2026-01-19",
        start_time: "09:00",
        end_time: "10:00",
        category_id: 1,
      },
    ];
    const users = [{ id: 1, first_name: "A", last_name: "B" }];
    const categories = [{ id: 1, counts_as_work: true }];
    const weeks = buildPendingWeeks(entries, users, categories);
    expect(weeks[0].week_start).toBe("2026-01-19");
    expect(weeks[1].week_start).toBe("2026-01-05");
  });
});
