// Tests for the Employee tab's report body: one person's balance, leave
// balance, absences, category breakdown, entries and flextime chart for the
// shared toolbar period. It absorbs what used to be three separate cards
// (Employee report, Category breakdown, Absences), so these tests cover all
// three areas plus the month/custom-range and own/other-user branches that
// only exist here now.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import PersonReport from "./PersonReport.svelte";
import { currentUser, settings, absenceCategories } from "../../stores.js";
import { setLanguage, setAbsenceCategoryCache } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getMonthReport: vi.fn(),
  getRangeReport: vi.fn(),
  getLeaveBalance: vi.fn(),
  getFlextimeReport: vi.fn(),
  getAbsenceReport: vi.fn(),
  getUserAbsencesByYear: vi.fn(),
  getHolidaysByYear: vi.fn(),
}));

import {
  getMonthReport,
  getRangeReport,
  getLeaveBalance,
  getFlextimeReport,
  getAbsenceReport,
  getUserAbsencesByYear,
  getHolidaysByYear,
} from "../../lib/api/reportsApi.js";

// Freeze "today" so month/range future-vs-past branching is deterministic.
vi.mock("../../format.js", async () => {
  const actual = await vi.importActual("../../format.js");
  return { ...actual, appTodayDate: vi.fn(() => new Date(2026, 5, 15)) }; // 2026-06-15
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`Text not found: "${text}"`);
}

function monthReportFixture(overrides = {}) {
  return {
    user_id: 1,
    month: "2026-06",
    days: [
      {
        date: "2026-06-01",
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
    category_totals: { Development: 480 },
    weeks_all_submitted: true,
    current_week_status: "approved",
    ...overrides,
  };
}

const users = [
  {
    id: 1,
    first_name: "Alice",
    last_name: "Employee",
    role: "employee",
    workdays_per_week: 5,
    start_date: "2020-01-01",
  },
  {
    id: 2,
    first_name: "Ann",
    last_name: "Assistant",
    role: "assistant",
    workdays_per_week: 5,
    start_date: "2020-01-01",
  },
];

describe("PersonReport", () => {
  let target;
  let component;
  let originalResizeObserver;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    // FlextimeChart measures its container via bind:clientWidth, which
    // compiles to a ResizeObserver — jsdom doesn't implement one.
    originalResizeObserver = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    currentUser.set({ id: 1, role: "employee", start_date: "2020-01-01" });
    const cats = [
      { id: 1, slug: "vacation", name: "Vacation", cost_type: "vacation" },
      { id: 2, slug: "sick", name: "Sick", cost_type: "none" },
      {
        id: 7,
        slug: "flextime_reduction",
        name: "Flextime Reduction",
        cost_type: "flextime",
      },
    ];
    absenceCategories.set(cats);
    setAbsenceCategoryCache(cats);
    vi.clearAllMocks();
    getMonthReport.mockResolvedValue(monthReportFixture());
    getRangeReport.mockResolvedValue(monthReportFixture());
    getLeaveBalance.mockResolvedValue(null);
    getFlextimeReport.mockResolvedValue([]);
    getAbsenceReport.mockResolvedValue([]);
    getUserAbsencesByYear.mockResolvedValue([]);
    getHolidaysByYear.mockResolvedValue([]);
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    globalThis.ResizeObserver = originalResizeObserver;
    target.remove();
  });

  it("shows an empty state and fetches nothing when no user is selected", async () => {
    component = mount(PersonReport, {
      target,
      props: { userId: null, users, periodMode: "month", month: "2026-06" },
    });
    await settle();

    expect(target.textContent).toContain("No data.");
    expect(getMonthReport).not.toHaveBeenCalled();
  });

  it("labels the balance 'My Balance' for the logged-in user's own report", async () => {
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "My Balance");
  });

  it("labels the balance 'Balance' (not 'My Balance') for another employee", async () => {
    currentUser.set({ id: 99, role: "team_lead", start_date: "2020-01-01" });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Balance");
    expect(target.textContent).not.toContain("My Balance");
  });

  it("hides the Submissions card, the target subtext, and flextime for an assistant", async () => {
    getMonthReport.mockResolvedValue(
      monthReportFixture({ target_min: 0, full_month_target_min: 0 }),
    );
    component = mount(PersonReport, {
      target,
      props: { userId: 2, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Logged");

    expect(target.textContent).not.toContain("Submissions");
    expect(target.textContent).not.toContain("Flextime balance");
    const loggedCard = [...target.querySelectorAll(".stat-card")].find((c) =>
      c.textContent.includes("Logged"),
    );
    expect(loggedCard.querySelector(".stat-card-sub")).toBeNull();
    // Assistants have no flextime account, so the chart/overtime fetch is
    // skipped entirely rather than requested and discarded.
    expect(getFlextimeReport).not.toHaveBeenCalled();
  });

  it("renders leave balance cards, including Planned/Requested only when > 0", async () => {
    getLeaveBalance.mockResolvedValue({
      annual_entitlement: 30,
      already_taken: 5,
      approved_upcoming: 2,
      requested: 0,
      available: 23,
    });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Entitlement");

    expect(target.textContent).toContain("Taken");
    expect(target.textContent).toContain("Planned");
    expect(target.textContent).not.toContain("Requested");
    expect(target.textContent).toContain("Remaining");
  });

  it("hides absence stat cards entirely when every absence has 0 effective days", async () => {
    // A Saturday-only absence counts as 0 workdays; the summary must not show
    // a "Sick: 0" card.
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 1,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-06", // Saturday
        end_date: "2026-06-06",
        status: "approved",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "My Balance");
    await settle();

    const sickCard = [...target.querySelectorAll(".stat-card")].find((c) =>
      c.textContent.includes("Sick"),
    );
    expect(sickCard).toBeUndefined();
  });

  it("shows the category breakdown with a percentage column", async () => {
    getMonthReport.mockResolvedValue(
      monthReportFixture({
        category_totals: { Development: 300, Meetings: 100 },
      }),
    );
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Category breakdown");

    expect(target.textContent).toContain("Development");
    expect(target.textContent).toContain("75%"); // 300 / (300+100)
    expect(target.textContent).toContain("25%");
  });

  it("renders the entries table with a status chip", async () => {
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Development");
    expect(target.querySelector(".zf-chip-approved")).not.toBeNull();
  });

  it("renders the entry comment truncated with a tooltip in the entries table", async () => {
    getMonthReport.mockResolvedValue(
      monthReportFixture({
        days: [
          {
            date: "2026-06-01",
            weekday: "Monday",
            entries: [
              {
                start_time: "08:00",
                end_time: "16:00",
                category: "Development",
                minutes: 480,
                status: "approved",
                comment: "Investigated the flaky login test",
              },
            ],
            actual_min: 480,
            target_min: 480,
            absence: null,
            holiday: null,
          },
        ],
      }),
    );
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Investigated the flaky login test");
    const commentCell = target.querySelector(".text-truncate-tooltip");
    expect(commentCell).not.toBeNull();
    expect(commentCell.getAttribute("title")).toBe(
      "Investigated the flaky login test",
    );
  });

  it("shows a dash for an entry without a comment", async () => {
    // monthReportFixture uses an empty comment; the table must render "-".
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Development");
    expect(target.querySelector(".text-truncate-tooltip")).toBeNull();
  });

  it("fetches own absences via getUserAbsencesByYear, not the team endpoint", async () => {
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await settle();

    expect(getUserAbsencesByYear).toHaveBeenCalledWith(2026);
    expect(getAbsenceReport).not.toHaveBeenCalled();
  });

  it("fetches another employee's absences via the team endpoint, filtered to that user", async () => {
    currentUser.set({ id: 99, role: "team_lead", start_date: "2020-01-01" });
    getAbsenceReport.mockResolvedValue([
      {
        id: 5,
        user_id: 1,
        kind: "vacation",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
      },
      {
        id: 6,
        user_id: 42, // a different employee — must be filtered out
        kind: "vacation",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Vacation");

    expect(getAbsenceReport).toHaveBeenCalled();
    expect(getUserAbsencesByYear).not.toHaveBeenCalled();
    // Only user 1's row should have made it into the rendered table.
    const rows = target.querySelectorAll("table.zf-table tbody tr");
    const absenceRows = [...rows].filter((r) =>
      r.textContent.includes("Vacation"),
    );
    expect(absenceRows.length).toBe(1);
  });

  it("renders the flextime chart section when the user has a flextime account and data", async () => {
    getFlextimeReport.mockResolvedValue([
      {
        date: "2026-06-01",
        actual_min: 480,
        target_min: 480,
        diff_min: 0,
        cumulative_min: 60,
        absence: null,
        holiday: null,
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Flextime balance");
    expect(target.querySelector("svg")).not.toBeNull();
  });

  it("shows a future-period note and skips time-based fetches for a fully-future custom range", async () => {
    component = mount(PersonReport, {
      target,
      props: {
        userId: 1,
        users,
        periodMode: "range",
        from: "2026-07-01",
        to: "2026-07-31",
      },
    });
    await settle();

    expect(target.textContent).toContain(
      "This period is entirely in the future",
    );
    expect(getRangeReport).not.toHaveBeenCalled();
    expect(getFlextimeReport).not.toHaveBeenCalled();
    // Absences still look forward even though hours/flextime don't.
    expect(getUserAbsencesByYear).toHaveBeenCalled();
  });

  it("omits the leave balance card for a custom range spanning more than one year", async () => {
    component = mount(PersonReport, {
      target,
      props: {
        userId: 1,
        users,
        periodMode: "range",
        from: "2025-12-01",
        to: "2026-01-31",
      },
    });
    await waitForText(target, "Logged");

    expect(getLeaveBalance).not.toHaveBeenCalled();
    expect(target.textContent).not.toContain("Entitlement");
  });

  it("shows a loading state while the request is in flight, not stale content", async () => {
    let resolveMonth;
    getMonthReport.mockImplementation(
      () => new Promise((resolve) => (resolveMonth = resolve)),
    );
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await settle();

    expect(target.textContent).toContain("Loading");
    expect(target.textContent).not.toContain("My Balance");

    resolveMonth(monthReportFixture());
    await waitForText(target, "My Balance");
  });

  // The multi-user version of this race guard (switching the employee picker
  // mid-request) is exercised end-to-end in Reports.test.js, since Svelte 5's
  // `mount()` doesn't expose a way to push new props into a live instance
  // from a unit test the way the real page does.
});
