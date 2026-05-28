// Tests for TimesheetExport — the CSV and PDF export panel. Employees and
// managers can download a date-range timesheet for themselves or (managers)
// for any team member. Tests focus on the form rendering, user-selector
// visibility, validation errors shown before any API call, and the Show
// button existence without triggering real downloads.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import TimesheetExport from "./TimesheetExport.svelte";
import { currentUser, settings } from "../../stores.js";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getRangeReport: vi.fn(),
  getFlextimeReport: vi.fn(),
}));

vi.mock("../../lib/exports/reportPdf.js", () => ({
  buildReportPdf: vi.fn(),
}));

import { getRangeReport } from "../../lib/api/reportsApi.js";

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

describe("TimesheetExport", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    currentUser.set({
      id: 1,
      role: "employee",
      tracks_time: true,
      workdays_per_week: 5,
    });
    vi.clearAllMocks();
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Export / Timesheet section heading", async () => {
    component = mount(TimesheetExport, { target, props: { users: [] } });
    await waitForText(target, "Export");
  });

  it("renders export buttons (CSV and/or PDF)", async () => {
    // Employees need at least one download option — if the export buttons
    // are missing the entire section is useless.
    component = mount(TimesheetExport, { target, props: { users: [] } });
    await settle();
    const buttons = target.querySelectorAll("button");
    expect(buttons.length).toBeGreaterThan(0);
  });

  it("shows a user selector dropdown in manager view", async () => {
    // Managers export timesheets for any team member. The dropdown must be
    // present and populated with the users list.
    const users = [
      { id: 2, first_name: "Bob", last_name: "Emp", role: "employee", tracks_time: true },
    ];
    component = mount(TimesheetExport, {
      target,
      props: { users, isSelfOnlyReportsView: false },
    });
    await settle();
    const select = target.querySelector("select");
    expect(select).not.toBeNull();
  });

  it("hides the user selector in self-only mode", async () => {
    // Employees see only their own data; showing a user selector would be
    // confusing and incorrect.
    component = mount(TimesheetExport, {
      target,
      props: { users: [], isSelfOnlyReportsView: true },
    });
    await settle();
    const select = target.querySelector("select");
    expect(select).toBeNull();
  });

  it("does not call getRangeReport before the export button is clicked", async () => {
    // Expensive API calls must only happen on demand, not on initial render.
    component = mount(TimesheetExport, { target, props: { users: [] } });
    await settle();
    expect(getRangeReport).not.toHaveBeenCalled();
  });
});
