// Tests for the Team tab: a per-person month table, a category matrix, and
// team absences, all driven by the shared toolbar period (no per-section
// "Show" button — everything loads automatically when the period changes).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import TeamReport from "./TeamReport.svelte";
import { currentUser, settings } from "../../stores.js";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getTeamReport: vi.fn(),
  getTeamCategoryReport: vi.fn(),
  getAbsenceReport: vi.fn(),
  getUserAbsencesByYear: vi.fn(),
  getHolidaysByYear: vi.fn(),
}));

import {
  getTeamReport,
  getTeamCategoryReport,
  getAbsenceReport,
  getUserAbsencesByYear,
  getHolidaysByYear,
} from "../../lib/api/reportsApi.js";

const users = [
  { id: 1, first_name: "Alice", last_name: "Smith", workdays_per_week: 5 },
  { id: 2, first_name: "Bob", last_name: "Jones", workdays_per_week: 5 },
];

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

describe("TeamReport", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    currentUser.set({
      id: 7,
      role: "team_lead",
      tracks_time: false, // keep the "own absences" merge branch inert by default
      permissions: { can_view_team_reports: true },
    });
    vi.clearAllMocks();
    getTeamReport.mockResolvedValue([]);
    getTeamCategoryReport.mockResolvedValue([]);
    getAbsenceReport.mockResolvedValue([]);
    getUserAbsencesByYear.mockResolvedValue([]);
    getHolidaysByYear.mockResolvedValue([]);
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("loads the month table automatically for the given month, without a Show button", async () => {
    component = mount(TeamReport, {
      target,
      props: { users, periodMode: "month", month: "2026-05", from: "", to: "" },
    });
    await settle();

    expect(getTeamReport).toHaveBeenCalledWith({ month: "2026-05" });
    expect(
      [...target.querySelectorAll("button")].some((b) =>
        b.textContent.includes("Show"),
      ),
    ).toBe(false);
  });

  it("renders employee rows once the month table resolves", async () => {
    getTeamReport.mockResolvedValueOnce([
      {
        user_id: 1,
        name: "Alice Smith",
        flextime_balance_min: 120,
        diff_min: 30,
        sick_days: 0,
        vacation_days: 5,
        vacation_planned_days: 0,
        weeks_all_submitted: true,
      },
      {
        user_id: 2,
        name: "Bob Jones",
        flextime_balance_min: -60,
        diff_min: -60,
        sick_days: 1,
        vacation_days: 0,
        vacation_planned_days: 0,
        weeks_all_submitted: false,
      },
    ]);
    component = mount(TeamReport, {
      target,
      props: { users, periodMode: "month", month: "2026-05", from: "", to: "" },
    });
    await waitForText(target, "Alice Smith");
    expect(target.textContent).toContain("Bob Jones");
    expect(target.textContent).toContain("Yes"); // Alice: all weeks submitted
  });

  it("clears the table and shows nothing when the API call fails", async () => {
    getTeamReport.mockRejectedValueOnce(new Error("Forbidden"));
    component = mount(TeamReport, {
      target,
      props: { users, periodMode: "month", month: "2026-05", from: "", to: "" },
    });
    await settle();
    await settle();

    expect(target.querySelector("table")).toBeNull();
  });

  it("hides the month table and shows a hint when period mode is a custom range", async () => {
    component = mount(TeamReport, {
      target,
      props: {
        users,
        periodMode: "range",
        month: "2026-05",
        from: "2026-04-01",
        to: "2026-06-30",
      },
    });
    await settle();

    expect(getTeamReport).not.toHaveBeenCalled();
    expect(target.textContent).toContain(
      "The team overview table is only available in month view.",
    );
  });

  it("loads the category matrix for the period bounds", async () => {
    component = mount(TeamReport, {
      target,
      props: { users, periodMode: "month", month: "2026-05", from: "", to: "" },
    });
    await settle();

    expect(getTeamCategoryReport).toHaveBeenCalledWith({
      from: "2026-05-01",
      to: "2026-05-31",
    });
  });

  it("re-fetches the category matrix when switching to a custom range", async () => {
    component = mount(TeamReport, {
      target,
      props: { users, periodMode: "month", month: "2026-05", from: "", to: "" },
    });
    await settle();
    getTeamCategoryReport.mockClear();

    // Re-mount with range props to simulate the toolbar switching mode.
    unmount(component);
    component = mount(TeamReport, {
      target,
      props: {
        users,
        periodMode: "range",
        month: "2026-05",
        from: "2026-04-01",
        to: "2026-04-30",
      },
    });
    await settle();

    expect(getTeamCategoryReport).toHaveBeenCalledWith({
      from: "2026-04-01",
      to: "2026-04-30",
    });
  });

  it("merges team absences with the lead's own absences when the lead tracks time", async () => {
    currentUser.set({
      id: 7,
      role: "team_lead",
      tracks_time: true,
      permissions: { can_view_team_reports: true },
    });
    getAbsenceReport.mockResolvedValueOnce([
      {
        id: 101,
        user_id: 1,
        kind: "vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-04",
        status: "approved",
      },
    ]);
    getUserAbsencesByYear.mockResolvedValueOnce([
      {
        id: 202,
        user_id: 7,
        kind: "sick",
        start_date: "2026-05-05",
        end_date: "2026-05-05",
        status: "approved",
      },
    ]);
    component = mount(TeamReport, {
      target,
      props: {
        users: [...users, { id: 7, first_name: "Ada", last_name: "Lead" }],
        periodMode: "month",
        month: "2026-05",
        from: "",
        to: "",
      },
    });
    await waitForText(target, "Alice Smith");
    expect(target.textContent).toContain("Ada Lead");
  });

  it("does not fetch the lead's own absences when they don't track time", async () => {
    component = mount(TeamReport, {
      target,
      props: { users, periodMode: "month", month: "2026-05", from: "", to: "" },
    });
    await settle();

    expect(getUserAbsencesByYear).not.toHaveBeenCalled();
  });
});
