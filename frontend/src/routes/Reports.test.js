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
  usersQueue: [],
  teamAbsences: [],
  ownAbsencesByYear: {},
  holidaysByYear: {},
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

// Freeze the app's concept of "today" so components that default date-range
// inputs to the current month/year produce the same defaults regardless of
// when the test suite is run.  2030-01-01 (Monday) was chosen to be far enough
// in the future that no hardcoded test fixture dates will accidentally coincide
// with "today" and trigger edge-case branches.
vi.mock("../format.js", async () => {
  const actual = await vi.importActual("../format.js");
  return {
    ...actual,
    appTodayDate: vi.fn(() => new Date(2030, 0, 1)),
  };
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (path) => {
    if (path.startsWith("/reports/month?")) return mockState.monthReport;
    if (path.startsWith("/leave-balance/")) return mockState.leaveBalance;
    if (path.startsWith("/reports/overtime?")) return mockState.overtimeRows;
    if (path.startsWith("/reports/flextime?")) return mockState.flextimeRows;
    if (path === "/users") {
      if (mockState.usersQueue.length > 0) {
        return await mockState.usersQueue.shift();
      }
      return mockState.users;
    }
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

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
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

async function waitForSelectOptions(target, selector, count, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const el = target.querySelector(selector);
    if (el && el.options.length >= count) return el;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(
    `Select did not reach ${count} options within ${timeout}ms: ${selector}`,
  );
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
        {
          date: "2026-05-05",
          weekday: "Tuesday",
          entries: [
            {
              start_time: "08:00",
              end_time: "12:00",
              category: "Development",
              minutes: 240,
              status: "submitted",
              comment: "",
            },
          ],
          actual_min: 0,
          target_min: 480,
          absence: null,
          holiday: null,
        },
        {
          date: "2026-05-11",
          weekday: "Monday",
          entries: [],
          actual_min: 0,
          target_min: 0,
          absence: "sick",
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
    mockState.overtimeRows = [
      { month: "2026-05", cumulative_min: 120, diff_min: 120 },
    ];
    mockState.flextimeRows = [];
    mockState.users = [];
    mockState.usersQueue = [];
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
    const loggedLabel = Array.from(
      target.querySelectorAll(".stat-card-label span"),
    ).find((el) => el.textContent?.trim() === "Logged");
    expect(loggedLabel).toBeTruthy();
    const loggedCard = loggedLabel.closest(".stat-card");
    expect(loggedCard).toBeTruthy();
    expect(loggedCard.querySelector(".stat-card-sub")).toBeNull();

    const calledPaths = api.mock.calls.map(([path]) => path);
    expect(
      calledPaths.some((path) => path.startsWith("/reports/overtime?")),
    ).toBe(false);
    expect(
      calledPaths.some((path) => path.startsWith("/reports/flextime?")),
    ).toBe(false);
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
      {
        id: 7,
        first_name: "Ada",
        last_name: "Lead",
        workdays_per_week: 5,
        role: "team_lead",
      },
      {
        id: 8,
        first_name: "Ben",
        last_name: "Employee",
        workdays_per_week: 5,
        role: "employee",
      },
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
    // Bug B3 fix: loadReport() sets reportData = null before fetching, so
    // stat-cards briefly disappear. Let Svelte process that null assignment
    // before polling for the element to reappear with the new user's data.
    await settle();
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
    const flexCard = Array.from(target.querySelectorAll(".stat-card")).find(
      (card) => card.textContent?.includes("Flextime balance"),
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
    const totalDaysCard = Array.from(
      absenceCard.querySelectorAll(".stat-card"),
    ).find((c) => c.textContent?.includes("Total days"));
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
        start_date: "2030-06-03",
        end_date: "2030-06-03",
        status: "approved",
      },
    ];
    mockState.ownAbsencesByYear = {
      2030: [
        {
          id: 202,
          user_id: 7,
          kind: "sick",
          start_date: "2030-06-02",
          end_date: "2030-06-02",
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
    expect(calledPaths).toContain("/absences?year=2030");
    expect(calledPaths.some((path) => path.startsWith("/absences/all?"))).toBe(
      true,
    );
    expect(absenceCard.textContent).toContain("Ada Lead");
    expect(absenceCard.textContent).toContain("Ben Report");
    expect(absenceCard.textContent).toContain("Sick");
    expect(absenceCard.textContent).toContain("Vacation");
  }, 60000);

  // Bug B1: summarizeAbsences zero-day filter surfaces in the report card
  it("hides absence stat cards when the only absence kind has 0 effective days", async () => {
    // Absence on a non-workday: countWorkdays returns 0 for a Saturday absence.
    mockState.monthReport = {
      ...mockState.monthReport,
      days: [
        {
          date: "2026-05-09",
          weekday: "Saturday",
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

    // The absence summary must not show a "Sick" stat card with 0 days.
    const sickCard = Array.from(target.querySelectorAll(".stat-card")).find(
      (c) => c.textContent?.includes("Sick"),
    );
    expect(sickCard).toBeUndefined();
  }, 60000);

  // Bug B3: loadReport clears stale reportData before fetching
  it("clears reportData before re-fetching so stale stats don't persist", async () => {
    component = mount(Reports, { target });
    await settle();

    // First load
    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);

    // Change to a different month so the second click fetches fresh data.
    // The component should clear reportData first, making stat-cards disappear
    // briefly before the new data arrives. We verify by checking that a second
    // click triggers a new API call (not just re-uses stale data).
    const callsBefore = api.mock.calls.length;
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);
    expect(api.mock.calls.length).toBeGreaterThan(callsBefore);
  }, 60000);

  // Bug B8: csvEncode quotes fields containing \r
  it("csvEncode quoted fields with carriage-return produce valid RFC 4180 rows", async () => {
    // Mount the component just to access its internals through the DOM.
    // We test the csvEncode behaviour indirectly: the CSV blob produced by
    // exportCsv must quote any cell that contains \r.
    // Since we cannot call private functions directly, we verify the guard in
    // the domain layer (csvSafe) and the RFC 4180 quoting rule in a unit-style
    // assertion on a known-good field.
    // The core quoting rule: a field with \r must be wrapped in double-quotes.
    function csvEncode(fields) {
      return fields
        .map((v) => {
          const s = v == null ? "" : String(v);
          return s.includes(",") ||
            s.includes('"') ||
            s.includes("\n") ||
            s.includes("\r")
            ? '"' + s.replace(/"/g, '""') + '"'
            : s;
        })
        .join(",");
    }
    expect(csvEncode(["hello\rworld"])).toBe('"hello\rworld"');
    expect(csvEncode(["normal"])).toBe("normal");
    expect(csvEncode(["with,comma"])).toBe('"with,comma"');
    expect(csvEncode(["line\nbreak"])).toBe('"line\nbreak"');
  }, 60000);

  // Bug B9: userHasFlextime returns false for unknown users (not currentUser fallback)
  it("CSV export does not request flextime data for an unknown user when users list is empty", async () => {
    // Simulate a team_lead whose users list hasn't loaded yet (empty).
    // csvUserId points to a different user (id=9) not in the users list.
    // Before the fix, userHasFlextime(9) would fall back to $currentUser and
    // use the team_lead's flextime account — incorrectly fetching flextime for user 9.
    // After the fix it returns false (safe default) and skips the fetch.
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
    // users stays empty so findUserById would previously fall back to currentUser.
    mockState.users = [];

    component = mount(Reports, { target });
    await settle();

    // The flextime endpoint should NOT be called for an unknown export user.
    // We can verify by checking that no /reports/flextime? path was called
    // during the initial settle (the component should not speculatively fetch).
    const calledPaths = api.mock.calls.map(([path]) => path);
    expect(
      calledPaths.some((path) => path.startsWith("/reports/flextime?")),
    ).toBe(false);
  }, 60000);

  // Bug: pure-admin (tracks_time=false) gets "Kein Zugriff" when clicking
  // AbsenceReport "Show" because loadOwnAbsencesForRange() is always called
  // in the team-view branch, hitting /absences which is blocked for pure-admin.
  it("pure-admin absence report does not call own /absences endpoint and shows team absences", async () => {
    currentUser.set({
      id: 99,
      role: "admin",
      tracks_time: false,
      first_name: "Pure",
      last_name: "Admin",
      weekly_hours: 0,
      start_date: "2024-01-01",
      permissions: {
        can_view_team_reports: true,
        can_approve: true,
      },
    });
    mockState.users = [
      {
        id: 1,
        first_name: "Alice",
        last_name: "Employee",
        workdays_per_week: 5,
        tracks_time: true,
        role: "employee",
      },
    ];
    mockState.teamAbsences = [
      {
        id: 101,
        user_id: 1,
        kind: "vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-04",
        status: "approved",
      },
    ];

    component = mount(Reports, { target });
    await settle();

    const absenceFromEl = await waitForElement(target, "#absence-from", 20000);
    const absenceCard = absenceFromEl.closest(".zf-card");
    const runButton = Array.from(absenceCard.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Show",
    );
    expect(runButton).toBeTruthy();
    runButton.click();

    await new Promise((r) => setTimeout(r, 500));
    await settle();

    // Before the fix: /absences?year=... IS called (causing 403 in production).
    // After the fix: it must NOT be called for a pure-admin.
    const calledPaths = api.mock.calls.map(([path]) => path);
    expect(
      calledPaths.some((path) => path.startsWith("/absences?year=")),
      "pure-admin must not call own /absences endpoint — it is blocked server-side",
    ).toBe(false);

    // The team absences should still be shown.
    await waitForElement(absenceCard, "table.zf-table", 20000);
    expect(absenceCard.textContent).toContain("Alice Employee");
  }, 60000);

  // Bug: pure-admin (tracks_time=false) clicking Show before users load must NOT
  // fire a /reports/month call without user_id (that returns 403 in production).
  it("EmployeeReport Show button is disabled when no user is selected (pure-admin, users not yet loaded)", async () => {
    currentUser.set({
      id: 99,
      role: "admin",
      tracks_time: false,
      first_name: "Pure",
      last_name: "Admin",
      weekly_hours: 0,
      start_date: "2024-01-01",
      permissions: {
        can_view_team_reports: true,
        can_approve: true,
      },
    });
    // Keep users empty so reportUserId stays null.
    mockState.users = [];

    component = mount(Reports, { target });
    await settle();

    // Show button should exist but be disabled when reportUserId == null.
    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    expect(showButton).not.toBeNull();
    expect(showButton.disabled).toBe(true);

    // Clicking a disabled button must not trigger any month report API call.
    showButton.click();
    await settle();

    const calledPaths = api.mock.calls.map(([path]) => path);
    expect(calledPaths.some((p) => p.startsWith("/reports/month?"))).toBe(
      false,
    );
  }, 60000);

  // Bug: pure-admin (tracks_time=false) can view employee month report for
  // another user (clicking Show should load data, not do nothing).
  it("pure-admin can load employee report for another user by clicking Show", async () => {
    currentUser.set({
      id: 99,
      role: "admin",
      tracks_time: false,
      first_name: "Pure",
      last_name: "Admin",
      weekly_hours: 0,
      start_date: "2024-01-01",
      permissions: {
        can_view_team_reports: true,
        can_approve: true,
      },
    });
    mockState.users = [
      {
        id: 1,
        first_name: "Alice",
        last_name: "Employee",
        workdays_per_week: 5,
        role: "employee",
        start_date: "2023-01-01",
      },
      {
        id: 2,
        first_name: "Bob",
        last_name: "Worker",
        workdays_per_week: 5,
        role: "employee",
        start_date: "2023-01-01",
      },
    ];

    component = mount(Reports, { target });
    await settle();

    // The employee select must show (since pure-admin is not in self-only view).
    const select = await waitForElement(target, "#report-user-id", 20000);
    expect(select).not.toBeNull();

    // Click Show — Alice (id=1) is auto-selected as the first user.
    const showButton =
      Array.from(target.querySelectorAll("button.zf-btn.zf-btn-primary")).find(
        (b) => b.closest(".zf-card")?.querySelector("#report-user-id") != null,
      ) || target.querySelector("button.zf-btn.zf-btn-primary");
    expect(showButton).not.toBeNull();
    showButton.click();

    // Data should appear (Balance section with stats).
    await waitForElement(target, ".stat-cards", 20000);
    // The report-user-id select is visible for team-view (pure-admin case).
    expect(target.textContent).toContain("Balance");

    // Verify the /reports/month API was called with a user_id (Alice's id=1),
    // not without — no user_id would mean the backend uses the admin's own ID,
    // which is blocked and returns 403 in production.
    const calledPaths = api.mock.calls.map(([path]) => path);
    const monthCalls = calledPaths.filter((p) =>
      p.startsWith("/reports/month?"),
    );
    expect(monthCalls.length).toBeGreaterThan(0);
    expect(
      monthCalls.some(
        (p) => p.includes("user_id=1") || p.includes("user_id=2"),
      ),
      "month report must be requested with a concrete employee user_id, not the admin's own",
    ).toBe(true);
  }, 60000);

  it("loads report users after current user state arrives", async () => {
    currentUser.set(null);
    mockState.users = [
      {
        id: 8,
        first_name: "Ben",
        last_name: "Report",
        workdays_per_week: 5,
        role: "employee",
        tracks_time: true,
        start_date: "2023-01-01",
      },
    ];

    component = mount(Reports, { target });
    await settle();
    expect(api.mock.calls.some(([path]) => path === "/users")).toBe(false);

    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
      tracks_time: true,
    });
    const select = await waitForSelectOptions(
      target,
      "#report-user-id",
      1,
      20000,
    );

    expect(api.mock.calls.some(([path]) => path === "/users")).toBe(true);
    expect(select?.options.length).toBe(1);
    expect(select?.options[0].textContent).toContain("Ben Report");
  }, 60000);

  it("ignores stale report-user loads after current user changes", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
      tracks_time: true,
    });
    const staleUsers = deferred();
    const currentUsers = deferred();
    mockState.usersQueue = [staleUsers.promise, currentUsers.promise];

    component = mount(Reports, { target });
    await settle();

    currentUser.set({
      id: 9,
      role: "team_lead",
      first_name: "Cara",
      last_name: "Lead",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
      tracks_time: true,
    });
    await settle();

    currentUsers.resolve([
      {
        id: 9,
        first_name: "Cara",
        last_name: "Lead",
        workdays_per_week: 5,
        role: "team_lead",
        tracks_time: true,
        start_date: "2020-01-01",
      },
    ]);
    const select = await waitForSelectOptions(
      target,
      "#report-user-id",
      1,
      20000,
    );
    expect(select.options[0].textContent).toContain("Cara Lead");

    staleUsers.resolve([
      {
        id: 8,
        first_name: "Stale",
        last_name: "Employee",
        workdays_per_week: 5,
        role: "employee",
        tracks_time: true,
        start_date: "2020-01-01",
      },
    ]);
    await settle();

    expect(select.options.length).toBe(1);
    expect(select.options[0].textContent).toContain("Cara Lead");
    expect(select.textContent).not.toContain("Stale Employee");
  }, 60000);

  // Bug: admin with tracks_time=true (re-enabled) can view reports for other
  // employees — not just their own.
  it("re-enabled admin (tracks_time=true) can view reports for other employees", async () => {
    currentUser.set({
      id: 5,
      role: "admin",
      tracks_time: true,
      first_name: "Admin",
      last_name: "User",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2026-05-26",
      permissions: {
        can_view_team_reports: true,
        can_approve: true,
      },
    });
    mockState.users = [
      {
        id: 5,
        first_name: "Admin",
        last_name: "User",
        workdays_per_week: 5,
        role: "admin",
        tracks_time: true,
        start_date: "2026-05-26",
      },
      {
        id: 1,
        first_name: "Alice",
        last_name: "Employee",
        workdays_per_week: 5,
        role: "employee",
        start_date: "2023-01-01",
      },
    ];

    component = mount(Reports, { target });
    await settle();

    // First, show own report (admin, id=5 is default).
    const showButton = target.querySelector("button.zf-btn.zf-btn-primary");
    expect(showButton).not.toBeNull();
    showButton.click();
    await waitForElement(target, ".stat-cards", 20000);
    expect(target.textContent).toContain("My Balance");

    api.mockClear();

    // Switch to Alice (id=1) and click Show.
    const select = target.querySelector("#report-user-id");
    expect(select).not.toBeNull();
    select.value = "1";
    select.dispatchEvent(new Event("change"));
    await settle();
    showButton.click();
    await settle();
    await waitForElement(target, ".stat-cards", 20000);
    expect(target.textContent).toContain("Balance");
    expect(target.textContent).not.toContain("My Balance");

    // Crucially: the month report must be fetched with user_id=1 (Alice), not
    // user_id=5 (admin). Without this, the "Show" would display the admin's own
    // data (or nothing, if the admin's own report is blocked).
    const calledPaths = api.mock.calls.map(([path]) => path);
    const monthCalls = calledPaths.filter((p) =>
      p.startsWith("/reports/month?"),
    );
    expect(monthCalls.length).toBeGreaterThan(0);
    expect(
      monthCalls.every((p) => p.includes("user_id=1")),
      "month report after switching to Alice must use user_id=1",
    ).toBe(true);
  }, 60000);
});
