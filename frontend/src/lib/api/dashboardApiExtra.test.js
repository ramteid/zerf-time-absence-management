// Tests for the additional dashboardApi functions beyond getApprovalDashboard.
// These cover the mutation helpers (approve, reject) and the report endpoints
// used by the Dashboard and Reports pages. Correct URL construction is critical
// because the backend enforces role checks on the path itself; a wrong path
// would silently call the wrong endpoint or receive a 404 instead of an error.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";
import {
  approveAbsenceById,
  approveReopen,
  approveWeek,
  getFlextime,
  getMonthSubmissionReport,
  getOvertimeSummary,
  getTeamAbsences,
  rejectAbsenceById,
  rejectReopen,
  rejectWeek,
} from "./dashboardApi.js";

describe("dashboardApi — additional functions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.mockResolvedValue({});
  });

  it("getFlextime includes the date range in the query string", async () => {
    // The flextime chart on the Dashboard shows balance over a date range.
    // If the from/to parameters are wrong, the chart will show data for the
    // wrong period and mislead employees about their balance.
    await getFlextime({ from: "2026-01-01", to: "2026-01-31" });
    expect(api).toHaveBeenCalledWith(
      "/reports/flextime?from=2026-01-01&to=2026-01-31",
    );
  });

  it("getOvertimeSummary includes the year parameter", async () => {
    // Overtime summary is always annual. A missing or wrong year would show
    // cumulative data for the wrong calendar year.
    await getOvertimeSummary(2026);
    expect(api).toHaveBeenCalledWith("/reports/overtime?year=2026");
  });

  it("getMonthSubmissionReport includes the month parameter", async () => {
    // The month submission status card on the Dashboard uses this endpoint.
    // Month must be in YYYY-MM format; a wrong format returns a 400 from the backend.
    await getMonthSubmissionReport("2026-05");
    expect(api).toHaveBeenCalledWith("/reports/month?month=2026-05");
  });

  it("getTeamAbsences appends caller-provided query params verbatim", async () => {
    // The Dashboard absence overview uses different filter combinations
    // (year, status). The function passes params as-is so callers control
    // what is filtered without this layer needing to know all possible flags.
    await getTeamAbsences("year=2026&status=approved");
    expect(api).toHaveBeenCalledWith("/absences/all?year=2026&status=approved");
  });

  it("approveWeek sends all entry IDs in a single batch request", async () => {
    // Approving a week is atomic — the backend processes all IDs or none.
    // Splitting into multiple calls would risk partial approvals if one fails.
    await approveWeek([1, 2, 3]);
    expect(api).toHaveBeenCalledWith("/time-entries/batch-approve", {
      method: "POST",
      body: { ids: [1, 2, 3] },
    });
  });

  it("rejectWeek includes the rejection reason in the request body", async () => {
    // The rejection reason is stored in the audit log and shown to the employee
    // so they know why their week was rejected. Missing reason would leave the
    // employee without actionable feedback.
    await rejectWeek([4, 5], "Incorrect hours");
    expect(api).toHaveBeenCalledWith("/time-entries/batch-reject", {
      method: "POST",
      body: { ids: [4, 5], reason: "Incorrect hours" },
    });
  });

  it("approveAbsenceById uses the standard /approve path for a pending request", async () => {
    // Standard absence approval — the request was submitted normally and is
    // now awaiting manager review. A POST to /approve transitions it to 'approved'.
    await approveAbsenceById({ id: 10, status: "pending_review" });
    expect(api).toHaveBeenCalledWith("/absences/10/approve", { method: "POST" });
  });

  it("approveAbsenceById uses /approve-cancellation for a cancellation_pending request", async () => {
    // When an employee requests cancellation of an already-approved absence,
    // the review dialog shows the same Approve button but must call a different
    // endpoint to handle the cancellation workflow, not re-approve the absence.
    await approveAbsenceById({ id: 11, status: "cancellation_pending" });
    expect(api).toHaveBeenCalledWith("/absences/11/approve-cancellation", {
      method: "POST",
    });
  });

  it("rejectAbsenceById sends the rejection reason for a pending request", async () => {
    // Employees see the rejection reason displayed on their Absences page.
    // The reason is required by the backend for standard absence rejections.
    await rejectAbsenceById({ id: 12, status: "pending_review" }, "No quota");
    expect(api).toHaveBeenCalledWith("/absences/12/reject", {
      method: "POST",
      body: { reason: "No quota" },
    });
  });

  it("rejectAbsenceById uses /reject-cancellation for a cancellation_pending request", async () => {
    // Rejecting a cancellation request means the absence stays approved.
    // This path does not carry a reason body because the employee only needs
    // to know the cancellation was refused, not why specifically.
    await rejectAbsenceById({ id: 13, status: "cancellation_pending" }, "");
    expect(api).toHaveBeenCalledWith("/absences/13/reject-cancellation", {
      method: "POST",
    });
  });

  it("approveReopen posts to the correct approve endpoint", async () => {
    // Approving a reopen request allows the employee to edit their submitted
    // week again. The empty body is required because the backend uses POST
    // not PATCH, but takes no payload.
    await approveReopen(20);
    expect(api).toHaveBeenCalledWith("/reopen-requests/20/approve", {
      method: "POST",
      body: {},
    });
  });

  it("rejectReopen sends the rejection reason", async () => {
    // The reason tells the employee why their edit request was denied and
    // is shown in the notification and the reopen-request list on their screen.
    await rejectReopen(21, "Not valid");
    expect(api).toHaveBeenCalledWith("/reopen-requests/21/reject", {
      method: "POST",
      body: { reason: "Not valid" },
    });
  });
});
