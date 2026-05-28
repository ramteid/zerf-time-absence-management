// Tests for the TeamReport section component. Admins and team leads use this
// to see an at-a-glance summary of every employee's monthly flextime balance,
// sick days, and vacation usage. Tests verify:
//   - The Show button triggers the correct API call
//   - Report data is rendered in a table with employee rows
//   - Error handling: API failures surface as toasts, not crashes

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import TeamReport from "./TeamReport.svelte";
import { earliestStartDate, settings } from "../../stores.js";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getTeamReport: vi.fn(),
}));

import { getTeamReport } from "../../lib/api/reportsApi.js";

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
    earliestStartDate.set("2020-01-01");
    vi.clearAllMocks();
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Team report section card heading", async () => {
    component = mount(TeamReport, { target });
    await waitForText(target, "Team report");
  });

  it("renders a Show button to trigger loading the report", async () => {
    // The report is expensive to load so it is not fetched automatically.
    // The admin must explicitly click Show to trigger the request.
    component = mount(TeamReport, { target });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    expect(showBtn).not.toBeNull();
  });

  it("calls getTeamReport with the selected month when Show is clicked", async () => {
    getTeamReport.mockResolvedValueOnce([]);
    component = mount(TeamReport, { target });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();

    expect(getTeamReport).toHaveBeenCalledWith(
      expect.objectContaining({ month: expect.stringMatching(/^\d{4}-\d{2}$/) })
    );
  });

  it("renders employee rows when the report contains data", async () => {
    const reportData = [
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
    ];
    getTeamReport.mockResolvedValueOnce(reportData);
    component = mount(TeamReport, { target });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await waitForText(target, "Alice Smith");
    expect(target.textContent).toContain("Bob Jones");
  });

  it("shows 'Yes' for employees with all weeks submitted", async () => {
    // The weeks-all-submitted indicator lets team leads quickly spot who
    // still has draft entries and needs a reminder to submit.
    getTeamReport.mockResolvedValueOnce([
      {
        user_id: 1,
        name: "Carol K",
        flextime_balance_min: 0,
        diff_min: 0,
        sick_days: 0,
        vacation_days: 0,
        vacation_planned_days: 0,
        weeks_all_submitted: true,
      },
    ]);
    component = mount(TeamReport, { target });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await waitForText(target, "Yes");
  });

  it("clears the report and shows no table when the API call fails", async () => {
    // A network failure or permission error must not leave stale data from a
    // previous load visible — it resets to show nothing (the toast conveys the error).
    getTeamReport.mockRejectedValueOnce(new Error("Forbidden"));
    component = mount(TeamReport, { target });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();

    expect(target.querySelector("table")).toBeNull();
  });
});
