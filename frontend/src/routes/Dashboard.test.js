import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Dashboard from "./Dashboard.svelte";
import { api } from "../api.js";
import { categories, currentUser, path } from "../stores.js";
import { setLanguage } from "../i18n.js";
import { appTodayDate, fmtMonthName } from "../format.js";

const mockState = vi.hoisted(() => ({
  monthReport: null,
  overtimeResponse: { rows: [], balance_as_of: null },
  flextimeResponse: { days: [], balance_as_of: null },
  payrollStatus: {
    enabled: true,
    period: "2026-07",
    period_label: "July 2026",
    from: "2026-07-01",
    to: "2026-07-31",
    sent: false,
    day_of_month: 5,
    total: 2,
    ready: 1,
    awaiting_approval: 0,
    not_submitted: 1,
    members: [],
  },
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

// "Approve all" waits on a confirmation modal that nothing clicks in jsdom;
// auto-confirming lets the approval path (and the reload it triggers) run.
vi.mock("../confirm.js", () => ({
  confirmDialog: vi.fn(async () => true),
}));

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath.startsWith("/reports/month?")) return mockState.monthReport;
    if (urlPath.startsWith("/reports/overtime?"))
      return mockState.overtimeResponse;
    if (urlPath.startsWith("/reports/flextime?"))
      return mockState.flextimeResponse;
    // Approver-only endpoints, reached when can_approve is set below.
    if (urlPath === "/reports/payroll-status") return mockState.payrollStatus;
    if (urlPath.startsWith("/time-entries/all")) return [];
    if (urlPath.startsWith("/absences/all")) return [];
    if (urlPath.startsWith("/reopen-requests/pending")) return [];
    if (urlPath === "/users") return [];
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Text not found within ${timeout}ms: ${text}`);
}

describe("Dashboard", () => {
  let target;
  let component;
  let originalResizeObserver;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    originalResizeObserver = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    path.set("/dashboard");
    api.mockClear();
    currentUser.set({
      id: 1,
      role: "employee",
      tracks_time: true,
      weekly_hours: 40,
      start_date: "2026-05-01",
      permissions: {
        can_approve: false,
      },
    });
    categories.set([
      { id: 1, name: "Core Duties", counts_as_work: true },
      { id: 2, name: "Flextime Reduction", counts_as_work: false },
    ]);
    setLanguage("en");
    mockState.monthReport = null;
    mockState.overtimeResponse = {
      rows: [{ month: "2026-05", cumulative_min: 0, diff_min: 0 }],
      balance_as_of: "2026-05-10",
    };
    mockState.flextimeResponse = { days: [], balance_as_of: "2026-05-10" };
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    globalThis.ResizeObserver = originalResizeObserver;
    target.remove();
  });

  it("marks the current month as submitted when only flextime reduction is submitted", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 480,
          submitted_min: 0,
          absence: null,
          entries: [
            {
              id: 10,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Flextime Reduction",
              status: "approved",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("marks the current month as submitted when the entry counts as work", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 480,
          submitted_min: 480,
          absence: null,
          entries: [
            {
              id: 11,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Core Duties",
              status: "approved",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("counts submitted entries even when category lookup is unavailable", async () => {
    categories.set([{ id: 1, name: "Core Duties", counts_as_work: true }]);
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 0,
          submitted_min: 0,
          absence: null,
          entries: [
            {
              id: 12,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Archived Flextime Reduction",
              counts_as_work: false,
              status: "approved",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("ignores current-week draft entries when elapsed weeks are submitted", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 480,
          submitted_min: 480,
          absence: null,
          entries: [
            {
              id: 13,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Core Duties",
              status: "approved",
            },
            {
              id: 14,
              entry_date: "2026-05-11",
              start_time: "16:00",
              end_time: "17:00",
              category: "Flextime Reduction",
              counts_as_work: false,
              status: "draft",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("marks missing when the backend reports elapsed weeks missing", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [],
      weeks_all_submitted: false,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "Weeks missing", 15000);
    expect(target.textContent).toContain("Weeks missing");
  });

  it("requests overtime with a concrete year", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    const overtimeCall = api.mock.calls.find(([pathValue]) =>
      String(pathValue).startsWith("/reports/overtime?year="),
    );
    expect(overtimeCall).toBeTruthy();
    expect(overtimeCall[0]).toMatch(/^\/reports\/overtime\?year=\d{4}$/);
  });

  it("shows the approved balance with the date it is stated as of", async () => {
    // The tile no longer mixes in submitted-but-unapproved hours, so the
    // number is only meaningful together with its cutoff date.
    mockState.monthReport = {
      month: "2026-05",
      days: [],
      weeks_all_submitted: true,
    };
    mockState.overtimeResponse = {
      rows: [{ month: "2026-05", cumulative_min: 120, diff_min: 30 }],
      balance_as_of: "2026-05-10",
    };

    component = mount(Dashboard, { target });
    await waitForText(target, "As of");

    expect(target.textContent).toContain("2.00h");
    expect(target.textContent).toContain("This month: 0.50h");
    // The removed "Approved: X" subtext must not come back.
    expect(target.textContent).not.toContain("Approved:");
  });

  describe("pure-admin (tracks_time=false)", () => {
    beforeEach(() => {
      currentUser.set({
        id: 1,
        role: "admin",
        tracks_time: false,
        weekly_hours: 0,
        start_date: "2026-01-01",
        permissions: {
          can_approve: true,
          can_view_dashboard: true,
          can_view_team_reports: true,
        },
      });
      api.mockImplementation(async (urlPath) => {
        if (urlPath === "/time-entries/all?status=submitted") return [];
        if (urlPath === "/absences/all?status=pending_review") return [];
        if (urlPath === "/reopen-requests/pending") return [];
        if (urlPath === "/users")
          return [
            {
              id: 2,
              first_name: "Tabea",
              last_name: "T",
              role: "team_lead",
              tracks_time: true,
              active: true,
            },
            {
              id: 3,
              first_name: "Eva",
              last_name: "E",
              role: "employee",
              tracks_time: true,
              active: true,
            },
          ];
        if (urlPath.startsWith("/reports/month?")) return mockState.monthReport;
        if (urlPath.startsWith("/reports/overtime?")) return [];
        if (urlPath.startsWith("/reports/flextime?")) return [];
        return [];
      });
    });

    it("does not call personal report endpoints (flextime, overtime, month)", async () => {
      component = mount(Dashboard, { target });
      await settle();
      await settle();

      const flextimeCall = api.mock.calls.find(([p]) =>
        String(p).startsWith("/reports/flextime?"),
      );
      const overtimeCall = api.mock.calls.find(([p]) =>
        String(p).startsWith("/reports/overtime?"),
      );
      const monthCall = api.mock.calls.find(([p]) =>
        String(p).startsWith("/reports/month?"),
      );
      expect(flextimeCall).toBeUndefined();
      expect(overtimeCall).toBeUndefined();
      expect(monthCall).toBeUndefined();
    });

    it("calls approval-related endpoints (time-entries/all, absences/all, reopen-requests)", async () => {
      component = mount(Dashboard, { target });
      await settle();
      await settle();

      const submittedCall = api.mock.calls.find(
        ([p]) => p === "/time-entries/all?status=submitted",
      );
      const absenceCall = api.mock.calls.find(
        ([p]) => p === "/absences/all?status=pending_review",
      );
      const reopenCall = api.mock.calls.find(
        ([p]) => p === "/reopen-requests/pending",
      );
      expect(submittedCall).toBeTruthy();
      expect(absenceCall).toBeTruthy();
      expect(reopenCall).toBeTruthy();
    });

    it("hides personal balance section", async () => {
      component = mount(Dashboard, { target });
      await settle();
      await settle();

      // The BalanceSection shows overtime balance text; it should not be present
      expect(target.textContent).not.toContain("Overtime balance");
      expect(target.textContent).not.toContain("Überstundensaldo");
    });
  });
  // The payroll card tracks exactly the approvals this page performs, so it
  // must refresh together with the approval queues — a card that keeps
  // claiming somebody is missing after their week was approved is worse than
  // no card at all.
  describe("payroll report card", () => {
    const submittedEntry = {
      id: 91,
      user_id: 3,
      entry_date: "2026-07-06",
      start_time: "09:00",
      end_time: "17:00",
      category_id: 1,
      status: "submitted",
    };

    function approverApi(overrides = {}) {
      return async (urlPath, opts = {}) => {
        if (urlPath === "/reports/payroll-status")
          return overrides.payrollStatus ?? mockState.payrollStatus;
        if (urlPath === "/reports/payroll-status?current=true")
          return overrides.payrollStatusCurrent ?? mockState.payrollStatus;
        if (urlPath === "/time-entries/all?status=submitted")
          return overrides.submitted ?? [submittedEntry];
        if (urlPath === "/absences/all?status=pending_review") return [];
        if (urlPath === "/reopen-requests/pending") return [];
        if (urlPath === "/users")
          return [
            {
              id: 3,
              first_name: "Eva",
              last_name: "E",
              role: "employee",
              tracks_time: true,
              active: true,
            },
          ];
        if (urlPath === "/time-entries/batch-approve" && opts.method === "POST")
          return { approved: 1 };
        if (urlPath.startsWith("/reports/month?")) return mockState.monthReport;
        if (urlPath.startsWith("/reports/overtime?")) return [];
        if (urlPath.startsWith("/reports/flextime?")) return [];
        return [];
      };
    }

    function payrollCalls(suffix = "") {
      return api.mock.calls.filter(
        ([p]) => p === `/reports/payroll-status${suffix}`,
      ).length;
    }

    function queueCalls() {
      return api.mock.calls.filter(
        ([p]) => p === "/time-entries/all?status=submitted",
      ).length;
    }

    beforeEach(() => {
      currentUser.set({
        id: 1,
        role: "team_lead",
        tracks_time: true,
        weekly_hours: 40,
        start_date: "2026-01-01",
        permissions: {
          can_approve: true,
          can_view_dashboard: true,
          can_view_team_reports: true,
        },
      });
      api.mockImplementation(approverApi());
    });

    it("shows the card with the month's progress", async () => {
      component = mount(Dashboard, { target });
      await settle();
      await settle();

      await waitForText(target, "1 of 2 done");
      expect(target.querySelector(".payroll-card")).toBeTruthy();
      const overviewCards = target.querySelector(".dashboard-overview-grid");
      expect(overviewCards.children[0].classList.contains("slider-card")).toBe(
        true,
      );
      expect(overviewCards.children[1].classList.contains("payroll-card")).toBe(
        true,
      );
    });

    it("refreshes together with the approval queues", async () => {
      component = mount(Dashboard, { target });
      await settle();
      await settle();
      await waitForText(target, "1 of 2 done");

      const before = payrollCalls();
      expect(before).toBe(queueCalls());

      // Approving re-runs the dashboard load; the card has to come along.
      const approveAll = [...target.querySelectorAll("button")].find(
        (button) => button.textContent.trim() === "Approve All",
      );
      expect(approveAll, "Approve All button not found").toBeTruthy();
      approveAll.click();
      await settle();
      await settle();

      expect(payrollCalls()).toBeGreaterThan(before);
      expect(payrollCalls()).toBe(queueCalls());
    });

    it("peeking at the current month sticks across later dashboard refreshes", async () => {
      api.mockImplementation(
        approverApi({
          payrollStatus: { ...mockState.payrollStatus, sent: true },
          payrollStatusCurrent: {
            ...mockState.payrollStatus,
            period: "2026-08",
            period_label: "August 2026",
            sent: false,
            ready: 0,
            awaiting_approval: 1,
            not_submitted: 1,
          },
        }),
      );

      component = mount(Dashboard, { target });
      await settle();
      await settle();
      await waitForText(target, "July 2026 sent");
      expect(payrollCalls("?current=true")).toBe(0);

      const label = `Show ${fmtMonthName(appTodayDate())}`;
      const peekButton = [...target.querySelectorAll("button")].find(
        (button) => button.textContent.trim() === label,
      );
      expect(peekButton, "peek button not found").toBeTruthy();
      peekButton.click();
      await settle();
      await settle();

      // The tile switched to the current month's own donut, not the sent one.
      await waitForText(target, "0 of 2 done");
      expect(payrollCalls("?current=true")).toBe(1);

      // Approving re-runs the dashboard load; the peek must not reset.
      const approveAll = [...target.querySelectorAll("button")].find(
        (button) => button.textContent.trim() === "Approve All",
      );
      expect(approveAll, "Approve All button not found").toBeTruthy();
      approveAll.click();
      await settle();
      await settle();

      expect(payrollCalls("?current=true")).toBeGreaterThan(1);
      // The bare endpoint was only ever hit once, by the very first load —
      // every refresh since the peek keeps asking for the current month.
      expect(payrollCalls()).toBe(1);
    });

    it("re-reads the status when the detail list is opened", async () => {
      // The overrides object is read on every call, so changing it here is
      // what "somebody approved something in the meantime" looks like.
      const overrides = { payrollStatus: { ...mockState.payrollStatus } };
      api.mockImplementation(approverApi(overrides));

      component = mount(Dashboard, { target });
      await settle();
      await settle();
      await waitForText(target, "1 of 2 done");

      const before = payrollCalls();
      overrides.payrollStatus = {
        ...mockState.payrollStatus,
        ready: 2,
        not_submitted: 0,
      };

      target.querySelector(".payroll-card-button").click();
      await settle();
      await settle();

      expect(payrollCalls()).toBe(before + 1);
      await waitForText(target, "2 of 2 done");
    });

    it("is not requested at all for someone who cannot approve", async () => {
      currentUser.set({
        id: 3,
        role: "employee",
        tracks_time: true,
        weekly_hours: 40,
        start_date: "2026-01-01",
        permissions: { can_approve: false },
      });
      component = mount(Dashboard, { target });
      await settle();
      await settle();

      expect(payrollCalls()).toBe(0);
      expect(target.querySelector(".payroll-card")).toBeNull();
    });
  });
});
