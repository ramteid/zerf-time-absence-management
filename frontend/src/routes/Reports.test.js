// Reports page: an Employee/Team scope switch sharing one toolbar (person +
// period). Everything loads automatically — there is no "Show" button — so
// these tests mount the page and wait for content to appear rather than
// clicking a trigger first.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Reports from "./Reports.svelte";
import { api } from "../api.js";
import { currentUser, absenceCategories, path, toasts } from "../stores.js";
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
      if (path === "/reports/users") {
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
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
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
  let originalScrollIntoView;
  let scrollIntoView;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    scrollIntoView = vi.fn();
    HTMLElement.prototype.scrollIntoView = scrollIntoView;
    history.replaceState({}, "", "/reports");

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
    HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
    history.replaceState({}, "", "/reports");
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
      if (path === "/reports/users") return mockState.users;
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

  it("keeps the CSV download identity from when the export was started", async () => {
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
    mockState.rangeReport = monthReportFixture({ user_id: 7 });
    path.set("/reports?user=7&from=2030-01-02&to=2030-01-10");
    component = mount(Reports, { target });
    await waitFor(
      () => target.querySelector("#reports-user-select")?.value === "7",
    );
    await waitFor(() => target.querySelector(".stat-cards"));

    const delayedExport = deferred();
    api.mockClear();
    api.mockImplementation(async (requestPath) => {
      if (
        requestPath.startsWith("/reports/range?") &&
        requestPath.includes("user_id=7") &&
        requestPath.includes("from=2030-01-02") &&
        requestPath.includes("to=2030-01-10")
      ) {
        return await delayedExport.promise;
      }
      return await defaultApiImpl(requestPath);
    });
    const originalCreateObjectURL = URL.createObjectURL;
    const originalRevokeObjectURL = URL.revokeObjectURL;
    const downloads = [];
    const downloadClick = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(function () {
        downloads.push(this.download);
      });
    URL.createObjectURL = vi.fn(() => "blob:mock");
    URL.revokeObjectURL = vi.fn();

    try {
      const csvButton = [...target.querySelectorAll("button")].find((button) =>
        button.textContent.includes("Export CSV"),
      );
      csvButton.click();
      await waitFor(() =>
        api.mock.calls.some(
          ([requestPath]) =>
            requestPath.startsWith("/reports/range?") &&
            requestPath.includes("user_id=7"),
        ),
      );

      path.set("/reports?user=8&from=2030-01-05&to=2030-01-12");
      await waitFor(
        () => target.querySelector("#reports-user-select")?.value === "8",
      );
      delayedExport.resolve(monthReportFixture({ user_id: 7 }));
      await waitFor(() => downloads.length === 1);

      expect(downloads).toEqual([
        "stundennachweis-Ada-Lead-2030-01-02_2030-01-10.csv",
      ]);
      expect(
        api.mock.calls.some(
          ([requestPath]) =>
            requestPath.startsWith("/reports/range?") &&
            requestPath.includes("user_id=7") &&
            requestPath.includes("from=2030-01-02") &&
            requestPath.includes("to=2030-01-10"),
        ),
      ).toBe(true);
    } finally {
      await settle();
      URL.createObjectURL = originalCreateObjectURL;
      URL.revokeObjectURL = originalRevokeObjectURL;
      downloadClick.mockRestore();
    }
  }, 20000);

  it("keeps the PDF download identity from when the export was started", async () => {
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
    mockState.rangeReport = monthReportFixture({ user_id: 7 });
    path.set("/reports?user=7&from=2030-01-02&to=2030-01-10");
    component = mount(Reports, { target });
    await waitFor(
      () => target.querySelector("#reports-user-select")?.value === "7",
    );
    await waitFor(() => target.querySelector(".stat-cards"));

    const delayedExport = deferred();
    api.mockClear();
    api.mockImplementation(async (requestPath) => {
      if (
        requestPath.startsWith("/reports/pdf?") &&
        requestPath.includes("user_id=7") &&
        requestPath.includes("from=2030-01-02") &&
        requestPath.includes("to=2030-01-10")
      ) {
        return await delayedExport.promise;
      }
      return await defaultApiImpl(requestPath);
    });
    const originalCreateObjectURL = URL.createObjectURL;
    const originalRevokeObjectURL = URL.revokeObjectURL;
    const downloads = [];
    const downloadClick = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(function () {
        downloads.push(this.download);
      });
    URL.createObjectURL = vi.fn(() => "blob:mock");
    URL.revokeObjectURL = vi.fn();

    try {
      const pdfButton = [...target.querySelectorAll("button")].find((button) =>
        button.textContent.includes("Export PDF"),
      );
      pdfButton.click();
      await waitFor(() =>
        api.mock.calls.some(
          ([requestPath]) =>
            requestPath.startsWith("/reports/pdf?") &&
            requestPath.includes("user_id=7"),
        ),
      );

      path.set("/reports?user=8&from=2030-01-05&to=2030-01-12");
      await waitFor(
        () => target.querySelector("#reports-user-select")?.value === "8",
      );
      delayedExport.resolve({
        blob: async () => new Blob(["A"], { type: "application/pdf" }),
      });
      await waitFor(() => downloads.length === 1);

      expect(downloads).toEqual([
        "stundennachweis-Ada-Lead-2030-01-02_2030-01-10.pdf",
      ]);
      expect(
        api.mock.calls.some(
          ([requestPath]) =>
            requestPath.startsWith("/reports/pdf?") &&
            requestPath.includes("user_id=7") &&
            requestPath.includes("from=2030-01-02") &&
            requestPath.includes("to=2030-01-10"),
        ),
      ).toBe(true);
    } finally {
      await settle();
      URL.createObjectURL = originalCreateObjectURL;
      URL.revokeObjectURL = originalRevokeObjectURL;
      downloadClick.mockRestore();
    }
  }, 20000);

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

  it("blocks a team report until its failed roster load succeeds", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    const failedUsers = deferred();
    const retriedUsers = deferred();
    mockState.usersQueue = [failedUsers.promise, retriedUsers.promise];
    mockState.teamAbsences = [
      {
        id: 17,
        user_id: 8,
        kind: "vacation",
        start_date: "2030-01-07",
        end_date: "2030-01-11",
        status: "approved",
      },
    ];
    component = mount(Reports, { target });
    api.mockClear();

    const teamTabButton = [...target.querySelectorAll(".tab-link")].find(
      (button) => button.textContent.includes("Team report"),
    );
    teamTabButton.click();
    await waitFor(() => target.querySelector(".loading-state"));
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/team?") ||
          requestPath.startsWith("/reports/team-categories?") ||
          requestPath.startsWith("/absences/all?"),
      ),
    ).toBe(false);

    failedUsers.reject(new Error("Error"));
    const retryButton = await waitFor(() =>
      target.querySelector(".zf-card-empty button"),
    );
    const teamPdfButton = [...target.querySelectorAll("button")].find(
      (button) => button.textContent.includes("Export team PDF"),
    );
    expect(teamPdfButton.disabled).toBe(true);
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/team?") ||
          requestPath.startsWith("/reports/team-categories?") ||
          requestPath.startsWith("/absences/all?"),
      ),
    ).toBe(false);

    retryButton.click();
    await settle();
    retriedUsers.resolve([
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      {
        id: 8,
        first_name: "Ben",
        last_name: "Employee",
        role: "employee",
        workdays_per_week: 3,
      },
    ]);

    const absenceRow = await waitFor(() =>
      [...target.querySelectorAll("tbody tr")].find((row) =>
        row.textContent.includes("Ben Employee"),
      ),
    );
    expect(absenceRow.querySelectorAll("td")[4].textContent.trim()).toBe("3");
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

  it("shows an unavailable linked employee after an empty user list finishes loading", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    const delayedUsers = deferred();
    mockState.usersQueue = [delayedUsers.promise];
    path.set("/reports?user=99&from=2030-01-06&to=2030-01-12");
    component = mount(Reports, { target });
    await settle();

    expect(target.textContent).not.toContain("User not found or inactive.");
    expect(target.querySelector(".loading-state")).not.toBeNull();
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?") ||
          requestPath.startsWith("/reports/flextime?") ||
          requestPath.startsWith("/leave-balances/") ||
          requestPath.startsWith("/absences/all?"),
      ),
    ).toBe(false);

    delayedUsers.resolve([]);
    await waitFor(() =>
      target.textContent.includes("User not found or inactive."),
    );
    expect(target.querySelector(".loading-state")).toBeNull();

    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?") ||
          requestPath.startsWith("/reports/flextime?") ||
          requestPath.startsWith("/leave-balances/") ||
          requestPath.startsWith("/absences/all?"),
      ),
    ).toBe(false);
  }, 20000);

  it("blocks a foreign deep link in a self-only view before it fetches the own report", async () => {
    currentUser.set({
      id: 7,
      role: "employee",
      first_name: "Ada",
      last_name: "Employee",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: false },
    });
    mockState.rangeReport = monthReportFixture({ user_id: 7 });
    path.set("/reports?user=8&from=2030-01-06&to=2030-01-12");
    component = mount(Reports, { target });

    expect(target.textContent).toContain("User not found or inactive.");
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?"),
      ),
    ).toBe(false);

    await settle();

    expect(target.textContent).toContain("User not found or inactive.");
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?") ||
          requestPath.startsWith("/reports/flextime?") ||
          requestPath.startsWith("/leave-balances/") ||
          requestPath.startsWith("/absences?year="),
      ),
    ).toBe(false);
  }, 20000);

  it("ignores a roster failure after the reports page unmounts", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    const delayedUsers = deferred();
    mockState.usersQueue = [delayedUsers.promise];
    let visibleToasts = [];
    toasts.set([]);
    const unsubscribe = toasts.subscribe((value) => {
      visibleToasts = value;
    });
    try {
      component = mount(Reports, { target });
      await settle();

      unmount(component);
      component = null;
      delayedUsers.reject(new Error("Error"));
      await settle();

      expect(visibleToasts).toEqual([]);
    } finally {
      unsubscribe();
      toasts.set([]);
    }
  }, 20000);

  it("does not substitute another employee for an unavailable link and accepts manual selection", async () => {
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
    path.set("/reports?user=99&from=2030-01-06&to=2030-01-12");
    component = mount(Reports, { target });

    await waitFor(() =>
      target.textContent.includes("User not found or inactive."),
    );
    const unavailableOption = target.querySelector(
      "#reports-user-select option:checked",
    );
    expect(unavailableOption.disabled).toBe(true);
    expect(unavailableOption.textContent).toBe("User not found or inactive.");
    const exportCsvButton = [...target.querySelectorAll("button")].find(
      (button) => button.textContent.includes("Export CSV"),
    );
    expect(exportCsvButton.disabled).toBe(true);
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?"),
      ),
    ).toBe(false);

    await selectUser(target, 8);
    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=8"),
      ),
    );

    expect(target.textContent).not.toContain("User not found or inactive.");
    expect(exportCsvButton.disabled).toBe(false);
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=7"),
      ),
    ).toBe(false);
  }, 20000);

  it("reapplies an unavailable deep link after a hash-only navigation", async () => {
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
    const linkedPath = "/reports?user=99&from=2030-01-06&to=2030-01-12";
    history.replaceState({}, "", `${linkedPath}#report-entries`);
    path.set(linkedPath);
    component = mount(Reports, { target });

    await waitFor(() =>
      target.textContent.includes("User not found or inactive."),
    );
    await selectUser(target, 8);
    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=8"),
      ),
    );
    api.mockClear();

    history.pushState({}, "", `${linkedPath}#report-absences`);
    window.dispatchEvent(new Event("hashchange"));

    await waitFor(() =>
      target.textContent.includes("User not found or inactive."),
    );
    expect(target.querySelector("#reports-user-select").value).toBe("99");
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?"),
      ),
    ).toBe(false);
  }, 20000);

  it("retries a failed linked-user load without changing its target", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    const failedUsers = deferred();
    const retriedUsers = deferred();
    mockState.usersQueue = [failedUsers.promise, retriedUsers.promise];
    mockState.rangeReport = monthReportFixture({ user_id: 8 });
    path.set("/reports?user=8&from=2030-01-06&to=2030-01-12");
    component = mount(Reports, { target });

    failedUsers.reject(new Error("Error"));
    const retryButton = await waitFor(() =>
      [...target.querySelectorAll("button")].find(
        (button) => button.textContent.includes("Retry") && !button.disabled,
      ),
    );
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?"),
      ),
    ).toBe(false);

    retryButton.click();
    await settle();
    retriedUsers.resolve([
      { id: 7, first_name: "Ada", last_name: "Lead", role: "team_lead" },
      { id: 8, first_name: "Ben", last_name: "Employee", role: "employee" },
    ]);

    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=8"),
      ),
    );

    expect(target.textContent).not.toContain("Retry");
    expect(target.querySelector("#reports-user-select").value).toBe("8");
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=7"),
      ),
    ).toBe(false);
  }, 20000);

  it("keeps a lead's own report usable when roster loading fails", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    const failedUsers = deferred();
    mockState.usersQueue = [failedUsers.promise];
    mockState.monthReport = monthReportFixture({ user_id: 7 });
    component = mount(Reports, { target });

    failedUsers.reject(new Error("Error"));
    await waitFor(() => target.querySelector("#reports-user-select")?.disabled);
    await waitFor(() => target.querySelector(".stat-cards"));

    const retryButton = [...target.querySelectorAll("button")].find((button) =>
      button.textContent.includes("Retry"),
    );
    const exportCsvButton = [...target.querySelectorAll("button")].find(
      (button) => button.textContent.includes("Export CSV"),
    );
    expect(retryButton).toBeTruthy();
    expect(target.querySelector("#reports-user-select").value).toBe("7");
    expect(exportCsvButton.disabled).toBe(false);
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") &&
          requestPath.includes("user_id=7"),
      ),
    ).toBe(true);
  }, 20000);

  it("restores the normal employee fallback after leaving an unavailable link", async () => {
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
    mockState.rangeReport = monthReportFixture({ user_id: 7 });
    path.set("/reports?user=99&from=2030-01-06&to=2030-01-12");
    component = mount(Reports, { target });

    await waitFor(() =>
      target.textContent.includes("User not found or inactive."),
    );
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/month?") ||
          requestPath.startsWith("/reports/range?"),
      ),
    ).toBe(false);
    api.mockClear();

    path.set("/reports");
    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=7"),
      ),
    );

    expect(target.querySelector("#reports-user-select").value).toBe("7");
    expect(target.textContent).not.toContain("User not found or inactive.");
  }, 20000);

  it("waits for a linked employee's metadata before loading their report", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: { can_view_team_reports: true },
    });
    const delayedUsers = deferred();
    mockState.usersQueue = [delayedUsers.promise];
    mockState.rangeReport = monthReportFixture({ user_id: 8 });
    mockState.teamAbsences = [
      {
        id: 55,
        user_id: 8,
        kind: "vacation",
        start_date: "2030-01-07",
        end_date: "2030-01-11",
        status: "approved",
        comment: "Three-day employee absence",
      },
    ];

    path.set("/reports?user=8&from=2030-01-07&to=2030-01-11");
    component = mount(Reports, { target });
    await settle();

    // Loading before /reports/users resolves would fall back to five workdays
    // and no flextime account. It must make no user-specific request yet.
    expect(
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.includes("user_id=8") ||
          requestPath.startsWith("/absences/all?"),
      ),
    ).toBe(false);

    delayedUsers.resolve([
      {
        id: 7,
        first_name: "Ada",
        last_name: "Lead",
        role: "team_lead",
        workdays_per_week: 5,
      },
      {
        id: 8,
        first_name: "Ben",
        last_name: "Employee",
        role: "employee",
        workdays_per_week: 3,
      },
    ]);

    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=8"),
      ),
    );
    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/flextime?") &&
          requestPath.includes("user_id=8"),
      ),
    );
    await waitFor(() =>
      target.textContent.includes("Three-day employee absence"),
    );

    const absenceRow = target.querySelector("#report-absences tbody tr");
    expect(absenceRow.querySelectorAll("td")[3].textContent.trim()).toBe("3");
    expect(target.textContent).toContain("Flextime balance");
  }, 20000);

  it("focuses the current deep-link section only after history navigation loads its report", async () => {
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
      {
        id: 7,
        first_name: "Ada",
        last_name: "Lead",
        role: "team_lead",
        workdays_per_week: 5,
      },
      {
        id: 8,
        first_name: "Ben",
        last_name: "Employee",
        role: "employee",
        workdays_per_week: 5,
      },
    ];
    component = mount(Reports, { target });
    await waitFor(() => target.querySelector("#report-entries"));
    api.mockClear();

    const delayedRange = deferred();
    mockState.teamAbsences = [
      {
        id: 56,
        user_id: 8,
        kind: "sick",
        start_date: "2030-01-07",
        end_date: "2030-01-07",
        status: "approved",
        comment: "Current navigation absence",
      },
    ];
    api.mockImplementation(async (requestPath) => {
      if (
        requestPath.startsWith("/reports/range?") &&
        requestPath.includes("user_id=8")
      ) {
        return await delayedRange.promise;
      }
      return await defaultApiImpl(requestPath);
    });

    // A browser history traversal can update the query and fragment without
    // a separate hashchange event. The existing report must not consume that
    // fragment before the newly selected data is ready.
    history.pushState(
      {},
      "",
      "/reports?user=8&from=2030-01-07&to=2030-01-11#report-absences",
    );
    window.dispatchEvent(new PopStateEvent("popstate"));
    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/reports/range?") &&
          requestPath.includes("user_id=8"),
      ),
    );

    expect(scrollIntoView).not.toHaveBeenCalled();
    delayedRange.resolve(monthReportFixture({ user_id: 8 }));
    await waitFor(() =>
      target.textContent.includes("Current navigation absence"),
    );
    await settle();

    const section = target.querySelector("#report-absences");
    expect(document.activeElement).toBe(section);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start" });
  }, 20000);

  it("keeps a valid far-future absence deep link visible in the date fields", async () => {
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

    // Absences may start more than a year in the future. The range itself is
    // still only two days, so it is a valid report deep link.
    path.set("/reports?user=8&from=2032-07-01&to=2032-07-02");
    component = mount(Reports, { target });

    await waitFor(() =>
      api.mock.calls.some(
        ([p]) =>
          p.startsWith("/absences/all?") &&
          p.includes("from=2032-07-01") &&
          p.includes("to=2032-07-02"),
      ),
    );
    await waitFor(
      () =>
        target.querySelector("#reports-period-from")?.value === "2032-07-01",
    );

    expect(target.querySelector("#reports-period-to").value).toBe("2032-07-02");
  }, 20000);

  it("keeps a calendar absence deep link before a later start date visible", async () => {
    // An administrator can correct a user's start date after an absence was
    // recorded. The calendar still exposes that historical absence, so its
    // report link must not be clamped to the corrected start date and lose the
    // absence comment it is meant to reveal.
    currentUser.set({
      id: 7,
      role: "team_lead",
      first_name: "Ada",
      last_name: "Lead",
      weekly_hours: 40,
      start_date: "2025-01-01",
      permissions: { can_view_team_reports: true },
    });
    mockState.users = [
      {
        id: 7,
        first_name: "Ada",
        last_name: "Lead",
        role: "team_lead",
        start_date: "2025-01-01",
      },
      {
        id: 8,
        first_name: "Ben",
        last_name: "Employee",
        role: "employee",
        start_date: "2024-01-01",
      },
    ];
    mockState.rangeReport = monthReportFixture({ user_id: 8 });
    mockState.teamAbsences = [
      {
        id: 55,
        user_id: 8,
        kind: "sick",
        start_date: "2023-07-10",
        end_date: "2023-07-11",
        status: "approved",
        comment: "Historical absence comment",
      },
    ];

    path.set("/reports?user=8&from=2023-07-10&to=2023-07-11");
    component = mount(Reports, { target });

    await waitFor(
      () => target.querySelector("#reports-user-select")?.value === "8",
    );
    await waitFor(
      () =>
        target.querySelector("#reports-period-from")?.value === "2023-07-10",
    );
    await waitFor(() =>
      api.mock.calls.some(
        ([requestPath]) =>
          requestPath.startsWith("/absences/all?") &&
          requestPath.includes("from=2023-07-10") &&
          requestPath.includes("to=2023-07-11"),
      ),
    );
    await waitFor(() =>
      target.textContent.includes("Historical absence comment"),
    );

    expect(target.querySelector("#reports-period-to").value).toBe("2023-07-11");

    // The exception is deliberately restricted to the linked employee. A
    // manual change to someone else must restore that person's normal lower
    // date bound instead of leaving the historical range open globally.
    await selectUser(target, 7);
    await waitFor(
      () =>
        target.querySelector("#reports-period-from")?.value === "2025-01-01",
    );
  }, 20000);

  it("ignores a deep link with a calendar-invalid date", async () => {
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

    path.set("/reports?user=8&from=2032-02-30&to=2032-03-01");
    component = mount(Reports, { target });

    await waitFor(
      () => target.querySelector("#reports-user-select")?.value === "7",
    );
    expect(api.mock.calls.some(([p]) => p.startsWith("/reports/range?"))).toBe(
      false,
    );
  }, 20000);

  it("ignores a malformed ?user/from/to deep link (multi-century range) instead of querying it", async () => {
    // Regression test: applyDeepLink used to trust from/to straight from the
    // URL with no validation. A stray or malformed link could set an
    // absurdly long range, which the absence-loading code would then expand
    // into one request per calendar year — flooding the API. It must be
    // rejected up front and the page must fall back to its normal default
    // (month view for the current user) instead.
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

    path.set("/reports?user=8&from=1926-01-01&to=2030-01-12");
    component = mount(Reports, { target });

    await waitFor(
      () => target.querySelector("#reports-user-select")?.value === "7",
    );
    expect(api.mock.calls.some(([p]) => p.startsWith("/reports/range?"))).toBe(
      false,
    );
  }, 20000);
});
