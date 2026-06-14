// Tests for the AbsenceReport section component. It shows approved absences
// for a date range: a table with employee, type, dates, and days. Managers
// (isLeadView) see an Employee column; employees in self-only mode don't.
// Tests verify the Show button trigger, API routing, and rendering of rows.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceReport from "./AbsenceReport.svelte";
import { currentUser, earliestStartDate, settings, absenceCategories } from "../../stores.js";
import { setLanguage, setAbsenceCategoryCache } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getAbsenceReport: vi.fn(),
  getHolidaysByYear: vi.fn(),
  getUserAbsencesByYear: vi.fn(),
}));

import {
  getAbsenceReport,
  getHolidaysByYear,
  getUserAbsencesByYear,
} from "../../lib/api/reportsApi.js";

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

describe("AbsenceReport", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    earliestStartDate.set(null);
    currentUser.set({ id: 1, role: "employee", tracks_time: true });
    const cats = [
      { id: 1, slug: "vacation", name: "Vacation", cost_type: "vacation", auto_approve_past: false },
      { id: 2, slug: "sick", name: "Sick", cost_type: "none", auto_approve_past: true },
      { id: 7, slug: "flextime_reduction", name: "Flextime Reduction", cost_type: "flextime", auto_approve_past: false },
    ];
    absenceCategories.set(cats);
    setAbsenceCategoryCache(cats);
    vi.clearAllMocks();
    // Default: no absences or holidays
    getAbsenceReport.mockResolvedValue([]);
    getHolidaysByYear.mockResolvedValue([]);
    getUserAbsencesByYear.mockResolvedValue([]);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Absences section heading", async () => {
    component = mount(AbsenceReport, { target, props: { users: [] } });
    await waitForText(target, "Absences");
  });

  it("renders a Show button to trigger loading the report", async () => {
    // Deferred loading prevents unnecessary API calls on initial render.
    component = mount(AbsenceReport, { target, props: { users: [] } });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    expect(showBtn).not.toBeNull();
  });

  it("calls getAbsenceReport in manager (lead) view", async () => {
    // Managers see all team absences via the /absences/all endpoint.
    // Pure-admin managers (tracks_time=false) only call this endpoint;
    // time-tracking managers also call getUserAbsencesByYear for their own row.
    currentUser.set({ id: 1, role: "admin", tracks_time: false });
    component = mount(AbsenceReport, {
      target,
      props: { users: [], isSelfOnlyReportsView: false },
    });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();
    expect(getAbsenceReport).toHaveBeenCalled();
    expect(getUserAbsencesByYear).not.toHaveBeenCalled();
  });

  it("calls getUserAbsencesByYear in self-only mode", async () => {
    // Employees can only access their own absence data; the team endpoint
    // would return a 403 for them.
    component = mount(AbsenceReport, {
      target,
      props: { users: [], isSelfOnlyReportsView: true },
    });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();
    expect(getUserAbsencesByYear).toHaveBeenCalled();
    expect(getAbsenceReport).not.toHaveBeenCalled();
  });

  it("renders 'No data.' when the report returns no absences", async () => {
    getAbsenceReport.mockResolvedValue([]);
    component = mount(AbsenceReport, {
      target,
      props: { users: [], isSelfOnlyReportsView: false },
    });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await waitForText(target, "No data.");
  });

  it("renders absence rows when the team report returns data", async () => {
    // An approved vacation row must appear in the table so managers can
    // verify who is away and for how long.
    getAbsenceReport.mockResolvedValue([
      {
        id: 1,
        user_id: 2,
        kind: "vacation",
        start_date: "2026-07-01",
        end_date: "2026-07-05",
        status: "approved",
        days: 5,
      },
    ]);
    component = mount(AbsenceReport, {
      target,
      props: {
        users: [{ id: 2, first_name: "Bob", last_name: "Emp", workdays_per_week: 5 }],
        isSelfOnlyReportsView: false,
      },
    });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await waitForText(target, "Vacation");
  });
});
