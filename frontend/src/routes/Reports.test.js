import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Reports from "./Reports.svelte";
import { currentUser } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  monthReport: null,
  overtimeRows: [],
  flextimeRows: [],
  leaveBalance: null,
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (path) => {
    if (path.startsWith("/reports/month?")) return mockState.monthReport;
    if (path.startsWith("/leave-balance/")) return mockState.leaveBalance;
    if (path.startsWith("/reports/overtime?")) return mockState.overtimeRows;
    if (path.startsWith("/reports/flextime?")) return mockState.flextimeRows;
    throw new Error(`Unhandled API path: ${path}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

// Poll until a matching element appears in `target`, or throw after `timeout` ms.
async function waitForElement(target, selector, timeout = 10000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const el = target.querySelector(selector);
    if (el) return el;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Element not found within ${timeout}ms: ${selector}`);
}

describe("Reports", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);

    currentUser.set({
      id: 1,
      role: "employee",
      weekly_hours: 40,
      start_date: "2020-01-01",
      permissions: {
        can_view_team_reports: false,
      },
    });
    setLanguage("en");

    mockState.monthReport = {
      user_id: 1,
      month: "2026-05",
      days: [
        {
          date: "2026-05-04",
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
      category_totals: {
        Development: 480,
      },
      weeks_all_submitted: true,
    };
    mockState.leaveBalance = null;
    mockState.overtimeRows = [{ month: "2026-05", cumulative_min: 120, diff_min: 120 }];
    mockState.flextimeRows = [];
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  // loadReport() makes 4 parallel async API calls; Svelte needs additional
  // microtask cycles to propagate the reactive update — use waitFor to poll.
  it("shows help text when clicking Logged and Submission status info buttons", async () => {
    component = mount(Reports, { target });
    await settle();

    const showButton = target.querySelector("button.kz-btn.kz-btn-primary");
    expect(showButton).not.toBeNull();
    showButton.click();

    const loggedHelp =
      "Submitted and approved hours including the current day for the current month.";
    const approvalsHelp =
      "Whether all required weeks in the selected month have been submitted.";

    // Poll until the stat cards appear — loadReport() is async and Svelte needs
    // several microtask cycles to re-render after Promise.all resolves.
    const loggedInfoButton = await waitForElement(
      target,
      `button[title='${loggedHelp}']`,
      10000,
    );
    loggedInfoButton.click();
    await settle();

    expect(target.textContent).toContain(loggedHelp);

    const approvalsInfoButton = await waitForElement(
      target,
      `button[title='${approvalsHelp}']`,
      5000,
    );
    approvalsInfoButton.click();
    await settle();

    expect(target.textContent).toContain(approvalsHelp);
  }, 20000);
});
