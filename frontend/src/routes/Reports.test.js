// Reports page: an Employee/Team scope switch sharing one toolbar (person +
// period). Everything loads automatically — there is no "Show" button — so
// these tests mount the page and wait for content to appear rather than
// clicking a trigger first.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Reports from "./Reports.svelte";
import { api } from "../api.js";
import { currentUser, absenceCategories, path } from "../stores.js";
import { setLanguage, setAbsenceCategoryCache } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  monthReport: null,
  rangeReport: null,
  flextimeRows: [],
  leaveBalances: [],
  users: [],
  usersQueue: [],
  teamReport: { leave_account_categories: [], rows: [] },
  teamCategoryReport: [],
  teamAbsences: [],
  ownAbsencesByYear: {},
  holidaysByYear: {},
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

// Freeze "today" so month/range defaults are deterministic across runs.
vi.mock("../format.js", async () => {
  const actual = await vi.importActual("../format.js");
  return { ...actual, appTodayDate: vi.fn(() => new Date(2030, 0, 15)) };
});

// A named, reusable default so tests that need to override the mock (e.g. to
// stall specific requests with a deferred promise) can restore it afterwards
// instead of leaking a custom implementation into later tests.
const defaultApiImpl = vi.hoisted(
  () =>
    async function defaultApiImpl(path) {
      if (path.startsWith("/reports/month?")) return mockState.monthReport;
      if (path.startsWith("/reports/range?")) return mockState.rangeReport;
      if (path.startsWith("/leave-balances/")) return mockState.leaveBalances;
      if (path.startsWith("/reports/flextime?")) return mockState.flextimeRows;
      if (path.startsWith("/reports/team-categories?"))
        return mockState.teamCategoryReport;
      if (path.startsWith("/reports/team?")) return mockState.teamReport;
      if (path.startsWith("/reports/pdf?")) {
        return {
          blob: async () => new Blob(["pdf"], { type: "application/pdf" }),
        };
      }
      if (path === "/users") {
        if (mockState.usersQueue.length > 0)
          return await mockState.usersQueue.shift();
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
    },
);

vi.mock("../api.js", () => ({
  api: vi.fn(defaultApiImpl),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function deferred() {
  let resolve;
  const promise = new Promise((res) => (resolve = res));
  return { promise, resolve };
}

async function waitFor(check, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const result = check();
    if (result) return result;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error("Condition not met within timeout");
}

// The #reports-user-select element renders immediately, but its <option>s
// only appear once the async user list has loaded — wait for the specific
// option, not just the element, before driving a selection change.
async function selectUser(target, userId, timeout = 15000) {
  const select = await waitFor(() => {
    const el = target.querySelector("#reports-user-select");
    return el && [...el.options].some((o) => o.value === String(userId))
      ? el
      : null;
  }, timeout);
  select.value = String(userId);
  select.dispatchEvent(new Event("change"));
  return select;
}

function monthReportFixture(overrides = {}) {
  return {
    user_id: 1,
    month: "2030-01",
    days: [
      {
        date: "2030-01-04",
        weekday: "Friday",
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
    category_totals: { Development: 480 },
    weeks_all_submitted: true,
    ...overrides,
  };
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
      permissions: { can_view_team_reports: false },
    });
    setLanguage("en");
    const cats = [
      { id: 1, slug: "vacation", name: "Vacation", cost_type: "vacation" },
      { id: 2, slug: "sick", name: "Sick", cost_type: "none" },
    ];
    absenceCategories.set(cats);
    setAbsenceCategoryCache(cats);

    mockState.monthReport = monthReportFixture();
    mockState.rangeReport = null;
    mockState.leaveBalances = [];
    mockState.flextimeRows = [];
    mockState.users = [];
    mockState.usersQueue = [];
    mockState.teamReport = { leave_account_categories: [], rows: [] };
    mockState.teamCategoryReport = [];
    mockState.teamAbsences = [];
    mockState.ownAbsencesByYear = {};
    mockState.holidaysByYear = {};
    api.mockClear();
    // Restore the default routing in case a previous test overrode it via
    // api.mockImplementation() (e.g. the race-guard test below).
    api.mockImplementation(defaultApiImpl);
    // Reset the routing path so deep-link tests don't leak query params into
    // unrelated tests.
    path.set("/reports");
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
    path.set("/reports");
  });

  it("loads the employee's own report automatically, without a Show button", async () => {
    component = mount(Reports, { target });
    await waitFor(() => target.querySelector(".stat-cards"));

    expect(target.textContent).toContain("My Balance");
    expect(
      [...target.querySelectorAll("button")].some(
        (b) => b.textContent.trim() === "Show",
      ),
    ).toBe(false);
  }, 20000);

  it("hides the tab bar and employee picker for a user without team-report access", async () => {
    component = mount(Reports, { target });
    await settle();

    expect(target.querySelector(".tab-link")).toBeNull();
    expect(target.querySelector("#reports-user-select")).toBeNull();
    expect(target.textContent).toContain("Your hours overview");
  }, 20000);

  it("shows Employee/Team tabs and the subtitle for a team lead", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", role: "employee" },
    ];

    component = mount(Reports, { target });
    await waitFor(() => target.querySelectorAll(".tab-link").length === 2);

    expect(target.textContent).toContain("Team hours overview");
  }, 20000);

  it("switching the employee picker fetches the newly selected user's report", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", role: "employee" },
    ];

    component = mount(Reports, { target });
    await waitFor(() => target.querySelector(".stat-cards"));
    expect(target.textContent).toContain("My Balance");

    api.mockClear();
    await selectUser(target, 8);

    await waitFor(() =>
      api.mock.calls.some(([path]) => path.includes("user_id=8")),
    );
    await waitFor(() => target.textContent.includes("Balance"));
    expect(target.textContent).not.toContain("My Balance");
  }, 20000);

  it("keeps the selected employee and period when switching to the Team tab and back", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", role: "employee" },
    ];

    component = mount(Reports, { target });
    await selectUser(target, 8);
    await settle();

    const teamTabBtn = [...target.querySelectorAll(".tab-link")].find((b) =>
      b.textContent.includes("Team report"),
    );
    teamTabBtn.click();
    await waitFor(() =>
      api.mock.calls.some(([p]) => p.startsWith("/reports/team?")),
    );

    const employeeTabBtn = [...target.querySelectorAll(".tab-link")].find((b) =>
      b.textContent.includes("Employee report"),
    );
    employeeTabBtn.click();
    await settle();

    expect(target.querySelector("#reports-user-select").value).toBe("8");
  }, 20000);

  it("pure-admin defaults to the first employee, never fetching a report without user_id", async () => {
    currentUser.set({
      id: 99,
      role: "admin",
      tracks_time: false,
      first_name: "Pure",
      last_name: "Admin",
      weekly_hours: 0,
      start_date: "2024-01-01",
      permissions: { can_view_team_reports: true, can_approve: true },
    });
    mockState.users = [
      {
        id: 1,
        first_name: "Alice",
        last_name: "Employee",
        role: "employee",
        start_date: "2023-01-01",
      },
    ];

    component = mount(Reports, { target });
    await waitFor(() => target.querySelector(".stat-cards"));

    const monthCalls = api.mock.calls
      .map(([path]) => path)
      .filter((p) => p.startsWith("/reports/month?"));
    expect(monthCalls.length).toBeGreaterThan(0);
    expect(monthCalls.every((p) => p.includes("user_id=1"))).toBe(true);
  }, 20000);

  it("only commits the most recently selected employee's data when switching quickly (race guard)", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", role: "employee" },
      { id: 9, first_name: "Cara", last_name: "Employee", role: "employee" },
    ];

    const slow = deferred();
    const fast = deferred();
    api.mockImplementation(async (path) => {
      if (path.startsWith("/reports/month?") && path.includes("user_id=8")) {
        return slow.promise;
      }
      if (path.startsWith("/reports/month?") && path.includes("user_id=9")) {
        return fast.promise;
      }
      if (path === "/users") return mockState.users;
      if (path.startsWith("/reports/month?")) return monthReportFixture();
      if (path.startsWith("/reports/flextime?")) return [];
      if (path.startsWith("/leave-balances/")) return [];
      if (path.startsWith("/absences?year=")) return [];
      if (path.startsWith("/absences/all?")) return [];
      if (path.startsWith("/holidays?year=")) return [];
      return null;
    });

    component = mount(Reports, { target });
    const select = await selectUser(target, 8);
    await settle();
    select.value = "9";
    select.dispatchEvent(new Event("change"));
    await settle();

    // Resolve the stale (user 8) request AFTER the newer (user 9) one.
    fast.resolve(
      monthReportFixture({
        user_id: 9,
        category_totals: { FreshCategoryMarker: 60 },
      }),
    );
    await settle();
    slow.resolve(
      monthReportFixture({
        user_id: 8,
        category_totals: { StaleCategoryMarker: 999 },
      }),
    );
    await settle();
    await settle();

    expect(target.textContent).not.toContain("StaleCategoryMarker");
    expect(target.textContent).toContain("FreshCategoryMarker");
  }, 20000);

  it("exports CSV for the currently displayed employee and period", async () => {
    mockState.rangeReport = { days: [] };
    mockState.flextimeRows = [];
    const originalCreateObjectURL = URL.createObjectURL;
    URL.createObjectURL = vi.fn(() => "blob:mock");
    URL.revokeObjectURL = vi.fn();

    component = mount(Reports, { target });
    await waitFor(() => target.querySelector(".stat-cards"));

    const csvBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Export CSV"),
    );
    csvBtn.click();
    await waitFor(() =>
      api.mock.calls.some(([p]) => p.startsWith("/reports/range?")),
    );

    URL.createObjectURL = originalCreateObjectURL;
  }, 15000);

  it("Team tab offers a combined PDF export instead of CSV", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
    ];

    component = mount(Reports, { target });
    await waitFor(() => target.querySelectorAll(".tab-link").length === 2);
    const teamTabBtn = [...target.querySelectorAll(".tab-link")].find((b) =>
      b.textContent.includes("Team report"),
    );
    teamTabBtn.click();
    await settle();

    expect(target.textContent).toContain("Export team PDF");
    expect(
      [...target.querySelectorAll("button")].some((b) =>
        b.textContent.includes("Export CSV"),
      ),
    ).toBe(false);
  }, 20000);

  it("applies a ?user/from/to deep link: employee tab, that user, custom range", async () => {
    // Simulates arriving from a pending approval's "View in report" button.
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", role: "employee" },
    ];
    mockState.rangeReport = monthReportFixture({ user_id: 8 });

    path.set("/reports?user=8&from=2030-01-06&to=2030-01-12");
    component = mount(Reports, { target });

    // The range endpoint is queried for the linked user and dates, proving the
    // deep link forced periodMode="range" and selected user 8.
    await waitFor(() =>
      api.mock.calls.some(
        ([p]) =>
          p.startsWith("/reports/range?") &&
          p.includes("user_id=8") &&
          p.includes("from=2030-01-06") &&
          p.includes("to=2030-01-12"),
      ),
    );

    // The Employee tab stays selected and the picker reflects the linked user.
    await waitFor(
      () => target.querySelector("#reports-user-select")?.value === "8",
    );
    expect(target.querySelector("#reports-user-select").value).toBe("8");
  }, 20000);
});
