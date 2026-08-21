// Tests for the Employee tab's report body: one person's balance, leave
// balance, absences, category breakdown, entries and flextime chart for the
// shared toolbar period. It absorbs what used to be three separate cards
// (Employee report, Category breakdown, Absences), so these tests cover all
// three areas plus the month/custom-range and own/other-user branches that
// only exist here now.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import { get } from "svelte/store";
import PersonReport from "./PersonReport.svelte";
import {
  currentUser,
  settings,
  absenceCategories,
  go,
  toasts,
} from "../../stores.js";
import { setLanguage, setAbsenceCategoryCache } from "../../i18n.js";
import { MASKED_ABSENCE_COLOR } from "../../colors.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getMonthReport: vi.fn(),
  getRangeReport: vi.fn(),
  getLeaveBalances: vi.fn(),
  getFlextimeReport: vi.fn(),
  getAbsenceReport: vi.fn(),
  getUserAbsencesByYear: vi.fn(),
  getHolidaysByYear: vi.fn(),
}));

import {
  getMonthReport,
  getRangeReport,
  getLeaveBalances,
  getFlextimeReport,
  getAbsenceReport,
  getUserAbsencesByYear,
  getHolidaysByYear,
} from "../../lib/api/reportsApi.js";

// Freeze "today" so month/range future-vs-past branching is deterministic.
vi.mock("../../format.js", async () => {
  const actual = await vi.importActual("../../format.js");
  // Both spellings of "today" must be frozen to the same day: the report reads
  // appTodayDate, while the flextime chart reads appTodayIsoDate to decide
  // which days are already in the past (only those get an absence band).
  // Leaving the latter on the wall clock would make the chart's behaviour
  // depend on when the suite runs.
  return {
    ...actual,
    appTodayDate: vi.fn(() => new Date(2026, 5, 15)), // 2026-06-15
    appTodayIsoDate: vi.fn(() => "2026-06-15"),
  };
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function deferred() {
  let resolve;
  const promise = new Promise((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`Text not found: "${text}"`);
}

async function waitFor(check, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (check()) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("Condition not met within timeout");
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

function leaveBalanceFixture(overrides = {}) {
  return {
    category_id: 1,
    category_name: "Vacation",
    color: "#3b82f6",
    active: true,
    annual_entitlement: 30,
    already_taken: 0,
    approved_upcoming: 0,
    requested: 0,
    available: 30,
    carryover_days: 0,
    carryover_remaining: 0,
    carryover_expiry: null,
    carryover_expired: false,
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
    weekly_hours: 40,
    start_date: "2020-01-01",
  },
  {
    id: 2,
    first_name: "Ann",
    last_name: "Assistant",
    role: "assistant",
    workdays_per_week: 5,
    weekly_hours: 0,
    start_date: "2020-01-01",
  },
];

describe("PersonReport", () => {
  let target;
  let component;
  let originalResizeObserver;
  let originalScrollIntoView;
  let scrollIntoView;

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
    originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    scrollIntoView = vi.fn();
    HTMLElement.prototype.scrollIntoView = scrollIntoView;
    history.replaceState({}, "", "/reports");
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    currentUser.set({ id: 1, role: "employee", start_date: "2020-01-01" });
    // `color` mirrors what GET /absence-categories actually returns — the
    // flextime chart colours its absence bands from exactly this field, so a
    // fixture without it would silently exercise the unknown-category fallback
    // instead of the real path.
    const cats = [
      {
        id: 1,
        slug: "vacation",
        name: "Vacation",
        cost_type: "vacation",
        color: "#0017c7",
      },
      {
        id: 2,
        slug: "sick",
        name: "Sick",
        cost_type: "none",
        color: "#ef4444",
      },
      {
        id: 7,
        slug: "flextime_reduction",
        name: "Flextime Reduction",
        cost_type: "flextime",
        color: "#008f8c",
      },
    ];
    absenceCategories.set(cats);
    setAbsenceCategoryCache(cats);
    vi.clearAllMocks();
    getMonthReport.mockResolvedValue(monthReportFixture());
    getRangeReport.mockResolvedValue(monthReportFixture());
    getLeaveBalances.mockResolvedValue([]);
    getFlextimeReport.mockResolvedValue({ days: [], balanceAsOf: null });
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
    HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
    history.replaceState({}, "", "/reports");
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

  it("renders every leave account for an assistant, including a zero account", async () => {
    getMonthReport.mockResolvedValue(
      monthReportFixture({ target_min: 0, full_month_target_min: 0 }),
    );
    getLeaveBalances.mockResolvedValue([
      leaveBalanceFixture({
        annual_entitlement: 0,
        already_taken: 0,
        approved_upcoming: 0,
        requested: 0,
        available: 0,
      }),
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 2, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Logged");
    await settle();

    expect(target.textContent).toContain("Vacation");
    expect(target.textContent).toContain("Entitlement");
  });

  it("shows leave-account values for an assistant with a real entitlement", async () => {
    getMonthReport.mockResolvedValue(
      monthReportFixture({ target_min: 0, full_month_target_min: 0 }),
    );
    getLeaveBalances.mockResolvedValue([
      leaveBalanceFixture({
        annual_entitlement: 10,
        already_taken: 2,
        approved_upcoming: 0,
        requested: 0,
        available: 8,
      }),
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 2, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Entitlement");

    expect(target.textContent).toContain("Vacation");
    expect(target.textContent).toContain("Available");
  });

  it("renders one leave-account card per category", async () => {
    getLeaveBalances.mockResolvedValue([
      leaveBalanceFixture({
        annual_entitlement: 30,
        already_taken: 5,
        approved_upcoming: 2,
        requested: 0,
        available: 23,
      }),
      leaveBalanceFixture({
        category_id: 8,
        category_name: "Education leave",
        annual_entitlement: 5,
        already_taken: 1,
        approved_upcoming: 1,
        requested: 1,
        available: 2,
      }),
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Entitlement");

    expect(
      target.querySelectorAll("[data-testid^='leave-account-card-']"),
    ).toHaveLength(2);
    expect(target.textContent).toContain("Education leave");
    expect(target.textContent).toContain("Taken");
    expect(target.textContent).toContain("Approved planned");
    expect(target.textContent).toContain("Requested");
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

  it("scrolls and focuses the linked entries section after loading the report", async () => {
    history.replaceState({}, "", "/reports#report-entries");
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Development");
    await settle();

    const section = target.querySelector("#report-entries");
    expect(section).not.toBeNull();
    expect(document.activeElement).toBe(section);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start" });
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

  it("renders an absence comment collapsed with a title tooltip until clicked", async () => {
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 5,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
        comment: "Medical certificate was submitted electronically.",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(
      target,
      "Medical certificate was submitted electronically.",
    );

    const comment = target.querySelector(".report-absence-comment");
    expect(comment).not.toBeNull();
    expect(comment.tagName).toBe("BUTTON");
    expect(comment.textContent.trim()).toBe(
      "Medical certificate was submitted electronically.",
    );
    expect(comment.getAttribute("title")).toBe(
      "Medical certificate was submitted electronically.",
    );
    expect(comment.classList).not.toContain("text-truncate-tooltip");
    // Collapsed by default: the row must not blow up before the user asks
    // for the full comment.
    expect(comment.classList).not.toContain("report-absence-comment-expanded");
    expect(comment.getAttribute("aria-expanded")).toBe("false");
  });

  it("expands an absence comment on click and collapses it again on a second click", async () => {
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 5,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
        comment: "Medical certificate was submitted electronically.",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(
      target,
      "Medical certificate was submitted electronically.",
    );

    const comment = target.querySelector(".report-absence-comment");
    comment.click();
    await settle();
    expect(comment.classList).toContain("report-absence-comment-expanded");
    expect(comment.getAttribute("aria-expanded")).toBe("true");

    comment.click();
    await settle();
    expect(comment.classList).not.toContain("report-absence-comment-expanded");
    expect(comment.getAttribute("aria-expanded")).toBe("false");
  });

  it("scrolls and focuses the linked absences section after loading it", async () => {
    history.replaceState({}, "", "/reports#report-absences");
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 5,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
        comment: "Medical certificate was submitted electronically.",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(
      target,
      "Medical certificate was submitted electronically.",
    );
    await settle();

    const section = target.querySelector("#report-absences");
    expect(section).not.toBeNull();
    expect(document.activeElement).toBe(section);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start" });
  });

  it("follows a report fragment selected after the data has loaded", async () => {
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 5,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
        comment: "Medical certificate was submitted electronically.",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(
      target,
      "Medical certificate was submitted electronically.",
    );
    expect(scrollIntoView).not.toHaveBeenCalled();

    history.replaceState({}, "", "/reports#report-absences");
    window.dispatchEvent(new Event("hashchange"));
    await settle();

    const section = target.querySelector("#report-absences");
    expect(document.activeElement).toBe(section);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start" });
  });

  it("does not focus a queued section after the fragment becomes invalid", async () => {
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 6,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
        comment: "A real target for the queued focus.",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "A real target for the queued focus.");

    history.replaceState({}, "", "/reports#report-absences");
    window.dispatchEvent(new Event("hashchange"));
    // Let the reactive focus effect queue its tick callback, then replace the
    // fragment before that callback is allowed to act.
    await Promise.resolve();
    history.replaceState({}, "", "/reports#unknown-section");
    window.dispatchEvent(new Event("hashchange"));
    await settle();

    expect(scrollIntoView).not.toHaveBeenCalled();
    expect(document.activeElement).not.toBe(
      target.querySelector("#report-absences"),
    );
  });

  it("follows a fragment set through SPA navigation with unchanged path and query", async () => {
    getUserAbsencesByYear.mockResolvedValue([
      {
        id: 5,
        user_id: 1,
        kind: "sick",
        start_date: "2026-06-10",
        end_date: "2026-06-10",
        status: "approved",
        comment: "Medical certificate was submitted electronically.",
      },
    ]);
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(
      target,
      "Medical certificate was submitted electronically.",
    );
    expect(scrollIntoView).not.toHaveBeenCalled();

    // `pushState` does not fire `hashchange` natively. The SPA helper must
    // still notify the already-mounted report that its target fragment moved.
    go("/reports#report-absences");
    await settle();

    const section = target.querySelector("#report-absences");
    expect(document.activeElement).toBe(section);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start" });
  });

  it("does not scroll or change focus for an unknown report fragment", async () => {
    history.replaceState({}, "", "/reports#unknown-section");
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Development");
    await settle();

    expect(scrollIntoView).not.toHaveBeenCalled();
    expect(document.activeElement).not.toBe(
      target.querySelector("#report-entries"),
    );
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
    getFlextimeReport.mockResolvedValue({
      days: [
        {
          date: "2026-06-01",
          actual_min: 480,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 60,
          absence: null,
          holiday: null,
        },
      ],
      balanceAsOf: "2026-06-07",
    });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Flextime balance");
    expect(target.querySelector("svg")).not.toBeNull();
  });

  // Regression: an employee who was absent during the reported period made the
  // flextime chart throw while colouring the absence band, which aborted the
  // whole report render — the person could be picked from the dropdown but
  // their report stayed blank. Colleagues without absences were unaffected,
  // which is what made it look like only certain employees were broken.
  it("renders the report for an employee who was absent during the period", async () => {
    getFlextimeReport.mockResolvedValue({
      days: [
        {
          date: "2026-06-01",
          actual_min: 480,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 60,
          absence: null,
          holiday: null,
        },
        {
          date: "2026-06-02",
          actual_min: 0,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 60,
          absence: "vacation",
          holiday: null,
        },
        {
          date: "2026-06-03",
          actual_min: 0,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 60,
          absence: "sick",
          holiday: null,
        },
      ],
      balanceAsOf: "2026-06-07",
    });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });

    await waitForText(target, "Flextime balance");
    expect(target.querySelector("svg")).not.toBeNull();
    // Assert the absence bands specifically. A plain `svg rect` count would
    // always pass — the chart's three clip-path rects exist regardless of
    // whether a single band was ever drawn.
    const bands = [
      ...target.querySelectorAll('rect[data-testid="flextime-band"]'),
    ].map((rect) => rect.getAttribute("fill"));
    expect(bands).toContain("#0017c7"); // vacation
    expect(bands).toContain("#ef4444"); // sick
  });

  it("renders the report when the chart covers an absence category the client does not know", async () => {
    // A category deleted after the absence was booked still reaches the chart
    // as a slug with no matching colour. It must fall back, not throw.
    getFlextimeReport.mockResolvedValue({
      days: [
        {
          date: "2026-06-01",
          actual_min: 0,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 0,
          absence: "category_removed_since",
          holiday: null,
        },
      ],
      balanceAsOf: "2026-06-07",
    });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });

    await waitForText(target, "Flextime balance");
    const bands = [
      ...target.querySelectorAll('rect[data-testid="flextime-band"]'),
    ].map((rect) => rect.getAttribute("fill"));
    expect(bands).toContain(MASKED_ABSENCE_COLOR);
  });

  it("labels the flextime balance with the date it is stated as of", async () => {
    // The balance stops at the last fully approved week, so the stat card and
    // the chart both name that date instead of implying "as of the period end".
    getFlextimeReport.mockResolvedValue({
      days: [
        {
          date: "2026-06-01",
          actual_min: 480,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 60,
          absence: null,
          holiday: null,
        },
      ],
      balanceAsOf: "2026-06-07",
    });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Flextime balance");

    expect(target.textContent).toContain("As of");
    // Quick-range buttons let the chart look beyond the reported month.
    expect(
      [...target.querySelectorAll("button")].some((b) =>
        b.textContent.includes("Last 90 days"),
      ),
    ).toBe(true);
  });

  it("reloads only the chart when a quick range is picked", async () => {
    getFlextimeReport.mockResolvedValue({
      days: [
        {
          date: "2026-06-01",
          actual_min: 480,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 60,
          absence: null,
          holiday: null,
        },
      ],
      balanceAsOf: "2026-06-07",
    });
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Flextime balance");
    getMonthReport.mockClear();
    getFlextimeReport.mockClear();

    const rangeButton = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Last 90 days"),
    );
    rangeButton.click();
    await settle();

    expect(getFlextimeReport).toHaveBeenCalledTimes(1);
    // The month report is untouched: the chart range is independent of the
    // period the rest of the page reports on.
    expect(getMonthReport).not.toHaveBeenCalled();
  });

  it("discards a stale chart response after the report context changes", async () => {
    // Supplying no `users` forces the component to use currentUser as its
    // metadata source. Updating the same user's workdays therefore creates a
    // new report key without needing a test-only wrapper component.
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users: [], periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "Flextime balance");
    getFlextimeReport.mockClear();

    const staleChart = deferred();
    const freshChart = {
      days: [
        {
          date: "2026-06-14",
          actual_min: 480,
          target_min: 480,
          diff_min: 0,
          cumulative_min: 120,
          absence: null,
          holiday: null,
        },
      ],
      balanceAsOf: "2026-06-14",
    };
    getFlextimeReport.mockImplementation(() => {
      if (getFlextimeReport.mock.calls.length === 1) return staleChart.promise;
      return Promise.resolve(freshChart);
    });

    const rangeButton = [...target.querySelectorAll("button")].find((button) =>
      button.textContent.includes("Last 90 days"),
    );
    rangeButton.click();
    await Promise.resolve();

    // The changed workday metadata starts a new report and invalidates the
    // outstanding manual chart request for the old report context.
    currentUser.set({
      id: 1,
      role: "employee",
      start_date: "2020-01-01",
      workdays_per_week: 4,
    });
    await waitFor(() => getFlextimeReport.mock.calls.length >= 2);
    await waitFor(() => target.querySelector(".chart-as-of")?.textContent);

    staleChart.resolve({ days: [], balanceAsOf: "2025-01-01" });
    await settle();

    const chartAsOf = target.querySelector(".chart-as-of").textContent;
    expect(chartAsOf).toContain("2026");
    expect(chartAsOf).not.toContain("2025");
  });

  it("reloads the report when weekly-hours metadata changes", async () => {
    // Supplying no `users` makes currentUser the metadata source, so this
    // isolates a weekly-hours change from every other report-key field.
    const originalUser = {
      id: 1,
      role: "employee",
      start_date: "2020-01-01",
      workdays_per_week: 5,
      weekly_hours: 40,
      overtime_start_balance_min: 0,
      tracks_time: true,
      active: true,
    };
    currentUser.set(originalUser);
    getMonthReport.mockResolvedValueOnce(
      monthReportFixture({ category_totals: { OriginalTargetMarker: 480 } }),
    );
    getMonthReport.mockResolvedValueOnce(
      monthReportFixture({ category_totals: { UpdatedTargetMarker: 384 } }),
    );
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users: [], periodMode: "month", month: "2026-06" },
    });
    await waitForText(target, "OriginalTargetMarker");

    currentUser.set({ ...originalUser, weekly_hours: 32 });
    await waitFor(() => getMonthReport.mock.calls.length === 2);
    await waitForText(target, "UpdatedTargetMarker");

    expect(target.textContent).not.toContain("OriginalTargetMarker");
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

    expect(getLeaveBalances).not.toHaveBeenCalled();
    expect(target.textContent).not.toContain("Entitlement");
  });

  it("caps an absurdly long custom range instead of firing one absence/holiday request per year", async () => {
    // Regression test: an unvalidated custom range (e.g. from a stray
    // deep-link value) used to expand into one getUserAbsencesByYear +
    // getHolidaysByYear call per calendar year in the range via
    // Promise.all — a multi-century span would flood the API with
    // thousands of requests. It must now be rejected up front.
    component = mount(PersonReport, {
      target,
      props: {
        userId: 1,
        users,
        periodMode: "range",
        from: "1926-01-01",
        to: "2026-06-15",
      },
    });
    await settle();

    expect(getUserAbsencesByYear).not.toHaveBeenCalled();
    expect(getHolidaysByYear).not.toHaveBeenCalled();
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

  it("does not surface a stale report failure after the component unmounts", async () => {
    let rejectMonth;
    getMonthReport.mockImplementation(
      () =>
        new Promise((_, reject) => {
          rejectMonth = reject;
        }),
    );
    component = mount(PersonReport, {
      target,
      props: { userId: 1, users, periodMode: "month", month: "2026-06" },
    });
    await waitFor(() => typeof rejectMonth === "function");

    toasts.set([]);
    unmount(component);
    component = null;

    rejectMonth(new Error("Late report failure"));
    await settle();

    expect(get(toasts)).toEqual([]);
  });

  // The multi-user version of this race guard (switching the employee picker
  // mid-request) is exercised end-to-end in Reports.test.js, since Svelte 5's
  // `mount()` doesn't expose a way to push new props into a live instance
  // from a unit test the way the real page does.
});
