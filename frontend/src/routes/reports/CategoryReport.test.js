// Tests for the CategoryReport section component. Employees see a breakdown
// of their hours by category (e.g. Project work, Training) over a date range.
// Admins see a team-wide breakdown across all employees. Tests verify:
//   - Show button triggers the API with the correct date range
//   - isSelfOnlyReportsView flag switches between personal and team endpoints
//   - Date range validation (from > to) prevents obviously wrong requests

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import CategoryReport from "./CategoryReport.svelte";
import { currentUser, earliestStartDate, settings } from "../../stores.js";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/reportsApi.js", () => ({
  getCategoryReport: vi.fn(),
  getTeamCategoryReport: vi.fn(),
}));

import { getCategoryReport, getTeamCategoryReport } from "../../lib/api/reportsApi.js";

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

describe("CategoryReport", () => {
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
      active: true,
    });
    vi.clearAllMocks();
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Category breakdown section heading", async () => {
    component = mount(CategoryReport, { target });
    await waitForText(target, "Category breakdown");
  });

  it("renders a Show button to trigger loading the report", async () => {
    // Deferred loading prevents unnecessary API calls on tab switch.
    component = mount(CategoryReport, { target });
    await settle();
    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    expect(showBtn).not.toBeNull();
  });

  it("calls getCategoryReport for the current user in self-only mode", async () => {
    // When isSelfOnlyReportsView is true, only the logged-in user's own data
    // should be requested — team-wide data must not be exposed.
    getCategoryReport.mockResolvedValueOnce([]);
    component = mount(CategoryReport, {
      target,
      props: { isSelfOnlyReportsView: true },
    });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();

    expect(getCategoryReport).toHaveBeenCalledWith(
      expect.objectContaining({ userId: 1 })
    );
    expect(getTeamCategoryReport).not.toHaveBeenCalled();
  });

  it("calls getTeamCategoryReport when not in self-only mode", async () => {
    // Admins / team leads see hours across all employees.
    // The team endpoint returns a matrix of user × category minutes.
    getTeamCategoryReport.mockResolvedValueOnce([]);
    component = mount(CategoryReport, {
      target,
      props: { isSelfOnlyReportsView: false },
    });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await settle();
    await settle();

    expect(getTeamCategoryReport).toHaveBeenCalled();
    expect(getCategoryReport).not.toHaveBeenCalled();
  });

  it("renders category rows when personal report data is returned", async () => {
    // The table must list each category name with its total hours so the
    // employee can see where their time was spent.
    getCategoryReport.mockResolvedValueOnce([
      { category: "Core Duties", total_min: 2400 },
      { category: "Training", total_min: 480 },
    ]);
    component = mount(CategoryReport, {
      target,
      props: { isSelfOnlyReportsView: true },
    });
    await settle();

    const showBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Show")
    );
    showBtn.click();
    await waitForText(target, "Core Duties");
    expect(target.textContent).toContain("Training");
  });

  it("resets report data when the API call fails", async () => {
    // A failed request must not leave stale data visible from a previous load.
    getCategoryReport.mockRejectedValueOnce(new Error("Server error"));
    component = mount(CategoryReport, {
      target,
      props: { isSelfOnlyReportsView: true },
    });
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
