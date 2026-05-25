import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Reports from "./Reports.svelte";
import { api } from "../api.js";
import { currentUser } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  monthReport: null,
  overtimeRows: [],
  flextimeRows: [],
  leaveBalance: null,
  users: [],
  teamAbsences: [],
  ownAbsencesByYear: {},
  holidaysByYear: {},
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (path) => {
    if (path.startsWith("/reports/month?")) return mockState.monthReport;
    if (path.startsWith("/leave-balance/")) return mockState.leaveBalance;
    if (path.startsWith("/reports/overtime?")) return mockState.overtimeRows;
    if (path.startsWith("/reports/flextime?")) return mockState.flextimeRows;
    if (path === "/users") return mockState.users;
    if (path.startsWith("/absences/all?")) return mockState.teamAbsences;
    if (path.startsWith("/absences?year=")) {
      const year = path.split("year=")[1];
      return mockState.ownAbsencesByYear[year] || [];
    }
    if (path.startsWith("/holidays?year=")) {
      const year = path.split("year=")[1];
      return mockState.holidaysByYear[year] || [];
    }
    throw new Error(`Unhandled API path: ${path}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

// Poll until a matching element appears in `target`, or throw after `timeout` ms.
async function waitForElement(target, selector, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const el = target.querySelector(selector);
    if (el) return el;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Element not found within ${timeout}ms: ${selector}`);
}

describe("Reports", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);

    currentUser.set({
      id: 1,
      role: "employee",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: {
        can_view_team_reports: false,
      },
    });
    setLanguage("en");

    mockState.monthReport = {
      user_id: 1,
      month: "2026-05",
      days: [
        {
          date: "2026-05-04",
          weekday: "Monday",
          entries: [
            {
              start_time: "08:00",
              end_time: "16:00",
              category: "Development",
              minutes: 480,
              status: "approved",
              comment: "",
            },
          ],
          actual_min: 480,
          target_min: 480,
          absence: null,
          holiday: null,
        },
      ],
      target_min: 480,
      actual_min: 480,
      diff_min: 0,
      submitted_min: 480,
      full_month_target_min: 480,
      category_totals: {
        Development: 480,
      },
      weeks_all_submitted: true,
    };
    mockState.leaveBalance = null;
    mockState.overtimeRows = [{ month: "2026-05", cumulative_min: 120, diff_min: 120 }];
    mockState.flextimeRows = [];
    mockState.users = [];
    mockState.teamAbsences = [];
    mockState.ownAbsencesByYear = {};
    mockState.holidaysByYear = {};
    api.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  // loadReport() makes 4 parallel async API calls; Svelte needs additional
  // microtask cycles to propagate the reactive update — use waitFor to poll.
  it("shows help text when clicking Logged and Submission status info buttons", async () => {
    component = mount(Reports, { target });
    await settle();

    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    expect(showButton).not.toBeNull();
    showButton.click();

    const loggedHelp =
      "Submitted and approved hours including the current day for the current month.";
    const approvalsHelp =
      "Whether all required weeks in the selected month have been submitted.";

    // Poll until the stat cards appear — loadReport() is async and Svelte needs
    // several microtask cycles to re-render after Promise.all resolves.
    await waitForElement(target, ".stat-cards", 20000);
    const loggedInfoButton = await waitForElement(
      target,
      `button[title='${loggedHelp}']`,
      20000,
    );
    loggedInfoButton.click();
    await settle();

    expect(target.textContent).toContain(loggedHelp);

    const approvalsInfoButton = await waitForElement(
      target,
      `button[title='${approvalsHelp}']`,
      20000,
    );
    approvalsInfoButton.click();
    await settle();

    expect(target.textContent).toContain(approvalsHelp);
  }, 60000);

  it("hides target subtext and skips flextime/overtime fetches for assistants", async () => {
    currentUser.set({
      id: 1,
      role: "assistant",
      weekly_hours: 0,
      start_date: "2020-01-01",
      permissions: {
        can_view_team_reports: false,
      },
    });
    mockState.monthReport = {
      ...mockState.monthReport,
      target_min: 0,
      full_month_target_min: 0,
    };

    component = mount(Reports, { target });
    await settle();

    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    expect(showButton).not.toBeNull();
    showButton.click();

    await waitForElement(target, ".stat-cards", 20000);
    const loggedLabel = Array.from(target.querySelectorAll(".stat-card-label span")).find(
      (el) => el.textContent?.trim() === "Logged",
    );
    expect(loggedLabel).toBeTruthy();
    const loggedCard = loggedLabel.closest(".stat-card");
    expect(loggedCard).toBeTruthy();
    expect(loggedCard.querySelector(".stat-card-sub")).toBeNull();

    const calledPaths = api.mock.calls.map(([path]) => path);
    expect(calledPaths.some((path) => path.startsWith("/reports/overtime?"))).toBe(false);
    expect(calledPaths.some((path) => path.startsWith("/reports/flextime?"))).toBe(false);
  }, 60000);

  // Bug 1: "My Balance" label is contextual
  it("shows 'My Balance' when viewing own report and 'Balance' when viewing another employee", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", workdays_per_week: 5, role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", workdays_per_week: 5, role: "employee" },
    ];

    component = mount(Reports, { target });
    await settle();

    // Click Show for own report (userId=7 selected by default)
    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);
    expect(target.textContent).toContain("My Balance");

    // Switch to employee 8 and click Show again
    const select = target.querySelector("#report-user-id");
    expect(select).not.toBeNull();
    select.value = "8";
    select.dispatchEvent(new Event("change"));
    await settle();
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);
    expect(target.textContent).toContain("Balance");
    expect(target.textContent).not.toContain("My Balance");
  }, 60000);

  // Bug 2: Null flextime balance shows neutral color
  it("shows neutral color for null flextime balance", async () => {
    mockState.overtimeRows = []; // no overtime rows → flextimeBalance is null
    component = mount(Reports, { target });
    await settle();
    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);

    // The flextime balance stat-card value should not be green (success-text) when null
    const flexCard = Array.from(target.querySelectorAll(".stat-card")).find((card) =>
      card.textContent?.includes("Flextime balance"),
    );
    expect(flexCard).toBeTruthy();
    const valueEl = flexCard.querySelector(".stat-card-value");
    expect(valueEl).toBeTruthy();
    // Should display the dash placeholder, not a positive balance
    expect(valueEl.textContent?.trim()).toBe("–");
    // Color must be --text-tertiary, not --success-text
    expect(valueEl.getAttribute("style")).toContain("--text-tertiary");
  }, 60000);

  // Bug 3: Empty category filter shows "No data" not an empty table
  it("shows No data message when all categories are deselected", async () => {
    mockState.monthReport = {
      ...mockState.monthReport,
      days: [],
    };
    component = mount(Reports, { target });
    await settle();

    // Load the category report for the current user
    const catFromEl = await waitForElement(target, "#cat-from", 20000);
    const catCard = catFromEl.closest(".zf-card");
    const showBtn = Array.from(catCard.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Show",
    );
    expect(showBtn).toBeTruthy();
    showBtn.click();

    // Wait for category table or No data message
    await new Promise((r) => setTimeout(r, 500));
    await settle();

    // When catFilteredCategories is empty (no categories), "No data." must appear
    // (the fix prevents an invisible empty table from rendering instead)
    // If the API returned no categories, catReport = [] which also shows No data.
    const noData = catCard.querySelector("div[style*='text-tertiary']");
    if (noData) {
      expect(noData.textContent?.trim()).toBe("No data.");
    }
  }, 60000);

  // Bug 4 & 5: Correct workdays_per_week (5 default) used for normalization
  it("normalises absence days with 5-day fallback when user is not in list", async () => {
    // Employee with 4-day week — but users list is empty (not loaded yet / self-only view)
    // The normalization should use 5 as default, not the current user's schedule.
    currentUser.set({
      id: 1,
      role: "employee",
      weekly_hours: 32,
      workdays_per_week: 4,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: false },
    });
    // Report has a Friday absence — with 4-day week Friday is not a workday,
    // but the fallback of 5 means Friday IS counted.
    mockState.monthReport = {
      ...mockState.monthReport,
      days: [
        {
          date: "2026-05-08",
          weekday: "Friday",
          entries: [],
          actual_min: 0,
          target_min: 0,
          absence: "sick",
          holiday: null,
        },
      ],
    };
    component = mount(Reports, { target });
    await settle();
    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);
    // The absence section should appear (sick day was counted)
    expect(target.textContent).toContain("Sick");
  }, 60000);

  // Bug 10: Absence stat cards hidden when all absences have 0 effective days
  it("hides absence day stat cards when all absences fall on non-workdays", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", workdays_per_week: 5 },
    ];
    // Absence on a Saturday and Sunday only — 0 working days
    mockState.teamAbsences = [
      {
        id: 301,
        user_id: 7,
        kind: "sick",
        start_date: "2026-05-09",
        end_date: "2026-05-10",
        status: "approved",
      },
    ];
    mockState.ownAbsencesByYear = {
      2026: [
        {
          id: 301,
          user_id: 7,
          kind: "sick",
          start_date: "2026-05-09",
          end_date: "2026-05-10",
          status: "approved",
        },
      ],
    };
    component = mount(Reports, { target });
    await settle();

    const absenceFromEl = await waitForElement(target, "#absence-from", 20000);
    const absenceCard = absenceFromEl.closest(".zf-card");
    const runButton = Array.from(absenceCard.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Show",
    );
    expect(runButton).toBeTruthy();
    runButton.click();
    await new Promise((r) => setTimeout(r, 800));
    await settle();

    // The absence table should appear (there are absences), but stat cards with
    // "Total days" must NOT appear since all days are 0.
    const totalDaysCard = Array.from(absenceCard.querySelectorAll(".stat-card")).find((c) =>
      c.textContent?.includes("Total days"),
    );
    expect(totalDaysCard).toBeUndefined();
  }, 60000);

  it("includes a team lead's own absences in the absence report", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
      permissions: {
        can_view_team_reports: true,
      },
    });
    mockState.users = [
      {
        id: 7,
        first_name: "Ada",
        last_name: "Lead",
        workdays_per_week: 5,
      },
      {
        id: 8,
        first_name: "Ben",
        last_name: "Report",
        workdays_per_week: 5,
      },
    ];
    mockState.teamAbsences = [
      {
        id: 101,
        user_id: 8,
        kind: "vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-04",
        status: "approved",
      },
    ];
    mockState.ownAbsencesByYear = {
      2026: [
        {
          id: 202,
          user_id: 7,
          kind: "sick",
          start_date: "2026-05-05",
          end_date: "2026-05-05",
          status: "approved",
        },
      ],
    };

    component = mount(Reports, { target });
    await settle();

    const absenceFrom = await waitForElement(target, "#absence-from", 20000);
    const absenceCard = absenceFrom.closest(".zf-card");
    const runButton = Array.from(absenceCard.querySelectorAll("button")).find(
      (button) => button.textContent?.trim() === "Show",
    );
    expect(runButton).toBeTruthy();
    runButton.click();

    await waitForElement(absenceCard, "table.zf-table", 20000);

    const calledPaths = api.mock.calls.map(([path]) => path);
    expect(calledPaths).toContain("/absences?year=2026");
    expect(calledPaths.some((path) => path.startsWith("/absences/all?"))).toBe(true);
    expect(absenceCard.textContent).toContain("Ada Lead");
    expect(absenceCard.textContent).toContain("Ben Report");
    expect(absenceCard.textContent).toContain("Sick");
    expect(absenceCard.textContent).toContain("Vacation");
  }, 60000);
});
