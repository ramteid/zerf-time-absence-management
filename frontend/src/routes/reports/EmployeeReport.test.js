// Tests for the EmployeeReport section component. It shows a month report
// for a selected employee: logged hours, flextime balance, and a list of
// entries + absences. Tests verify the Show button trigger, the user selector
// in manager view (hidden in self-only mode), and that results render.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import EmployeeReport from "./EmployeeReport.svelte";
import { currentUser, earliestStartDate, settings } from "../../stores.js";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getMonthReport: vi.fn(),
  getLeaveBalance: vi.fn(),
  getOvertimeReport: vi.fn(),
  getFlextimeReport: vi.fn(),
}));

import {
  getFlextimeReport,
  getLeaveBalance,
  getMonthReport,
  getOvertimeReport,
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

const monthReportEmpty = {
  submitted_min: 0,
  full_month_target_min: 0,
  weeks_all_submitted: false,
  weeks_all_approved: false,
  current_week_status: "draft",
  entries: [],
  absences: [],
  category_totals: {},
};

describe("EmployeeReport", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    earliestStartDate.set(null);
    currentUser.set({
      id: 1,
      role: "employee",
      tracks_time: true,
      workdays_per_week: 5,
      weekly_hours: 40,
    });
    vi.clearAllMocks();
    // Default: empty but successful API responses
    getMonthReport.mockResolvedValue(monthReportEmpty);
    getLeaveBalance.mockResolvedValue(null);
    getOvertimeReport.mockResolvedValue([]);
    getFlextimeReport.mockResolvedValue([]);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Employee report section heading", async () => {
    component = mount(EmployeeReport, { target, props: { users: [] } });
    await waitForText(target, "Employee report");
  });

  it("renders a Show button to trigger loading the report", async () => {
    // The report is loaded on demand to avoid fetching when the tab isn't active.
    component = mount(EmployeeReport, { target, props: { users: [] } });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    expect(showBtn).not.toBeNull();
  });

  it("shows a user selector dropdown in manager view", async () => {
    // Managers need to select which employee's report to view; the dropdown
    // is hidden in self-only mode since there is only one possible user.
    const users = [
      { id: 2, first_name: "Bob", last_name: "Emp", role: "employee", tracks_time: true },
    ];
    component = mount(EmployeeReport, {
      target,
      props: { users, isSelfOnlyReportsView: false },
    });
    await settle();
    const select = target.querySelector("select");
    expect(select).not.toBeNull();
  });

  it("hides the user selector in self-only mode", async () => {
    // Self-only mode is for employees — there is no choice to make since the
    // report is always for the current user.
    component = mount(EmployeeReport, {
      target,
      props: { users: [], isSelfOnlyReportsView: true },
    });
    await settle();
    const select = target.querySelector("select");
    expect(select).toBeNull();
  });

  it("calls getMonthReport when Show is clicked", async () => {
    const users = [
      { id: 1, first_name: "Alice", last_name: "Emp", role: "employee", tracks_time: true },
    ];
    component = mount(EmployeeReport, {
      target,
      props: { users, isSelfOnlyReportsView: true },
    });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();
    expect(getMonthReport).toHaveBeenCalledWith(
      expect.objectContaining({ userId: 1 })
    );
  });

  it("renders stat cards after loading a report", async () => {
    // The stat cards (Logged, Submissions) must appear so the user can see
    // their summary at a glance without scrolling to the detail table.
    const users = [
      { id: 1, first_name: "Alice", last_name: "Emp", role: "employee", tracks_time: true, weekly_hours: 40, workdays_per_week: 5 },
    ];
    getMonthReport.mockResolvedValue({
      ...monthReportEmpty,
      submitted_min: 2400,
      full_month_target_min: 9600,
    });
    component = mount(EmployeeReport, {
      target,
      props: { users, isSelfOnlyReportsView: true },
    });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await waitForText(target, "Logged");
  });
});
