import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Dashboard from "./Dashboard.svelte";
import { api } from "../api.js";
import { categories, currentUser, path } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  monthReport: null,
  overtimeRows: [],
  flextimeRows: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath.startsWith("/reports/month?")) return mockState.monthReport;
    if (urlPath.startsWith("/reports/overtime?")) return mockState.overtimeRows;
    if (urlPath.startsWith("/reports/flextime?")) return mockState.flextimeRows;
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Text not found within ${timeout}ms: ${text}`);
}

describe("Dashboard", () => {
  let target;
  let component;
  let originalResizeObserver;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    originalResizeObserver = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    path.set("/dashboard");
    currentUser.set({
      id: 1,
      role: "employee",
      weekly_hours: 40,
      start_date: "2026-05-01",
      permissions: {
        can_approve: false,
      },
    });
    categories.set([
      { id: 1, name: "Core Duties", counts_as_work: true },
      { id: 2, name: "Flextime Reduction", counts_as_work: false },
    ]);
    setLanguage("en");
    mockState.monthReport = null;
    mockState.overtimeRows = [{ month: "2026-05", cumulative_min: 0, diff_min: 0 }];
    mockState.flextimeRows = [];
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    globalThis.ResizeObserver = originalResizeObserver;
    target.remove();
  });

  it("marks the current month as submitted when only flextime reduction is submitted", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 480,
          submitted_min: 0,
          absence: null,
          entries: [
            {
              id: 10,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Flextime Reduction",
              status: "approved",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("marks the current month as submitted when the entry counts as work", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 480,
          submitted_min: 480,
          absence: null,
          entries: [
            {
              id: 11,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Core Duties",
              status: "approved",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("counts submitted entries even when category lookup is unavailable", async () => {
    categories.set([{ id: 1, name: "Core Duties", counts_as_work: true }]);
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 0,
          submitted_min: 0,
          absence: null,
          entries: [
            {
              id: 12,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Archived Flextime Reduction",
              counts_as_work: false,
              status: "approved",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("ignores current-week draft entries when elapsed weeks are submitted", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [
        {
          date: "2026-05-11",
          target_min: 480,
          actual_min: 480,
          submitted_min: 480,
          absence: null,
          entries: [
            {
              id: 13,
              entry_date: "2026-05-11",
              start_time: "08:00",
              end_time: "16:00",
              category: "Core Duties",
              status: "approved",
            },
            {
              id: 14,
              entry_date: "2026-05-11",
              start_time: "16:00",
              end_time: "17:00",
              category: "Flextime Reduction",
              counts_as_work: false,
              status: "draft",
            },
          ],
        },
      ],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "All submitted", 15000);
    expect(target.textContent).toContain("All submitted");
  });

  it("marks missing when the backend reports elapsed weeks missing", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [],
      weeks_all_submitted: false,
    };

    component = mount(Dashboard, { target });
    await settle();

    await waitForText(target, "Weeks missing", 15000);
    expect(target.textContent).toContain("Weeks missing");
  });

  it("requests overtime with a concrete year", async () => {
    mockState.monthReport = {
      month: "2026-05",
      days: [],
      weeks_all_submitted: true,
    };

    component = mount(Dashboard, { target });
    await settle();

    const overtimeCall = api.mock.calls.find(([pathValue]) =>
      String(pathValue).startsWith("/reports/overtime?year="),
    );
    expect(overtimeCall).toBeTruthy();
    expect(overtimeCall[0]).toMatch(/^\/reports\/overtime\?year=\d{4}$/);
  });
});
