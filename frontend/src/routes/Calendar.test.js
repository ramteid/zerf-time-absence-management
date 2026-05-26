import { afterEach, beforeEach, describe, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Calendar from "./Calendar.svelte";
import { api } from "../api.js";
import { categories, currentUser, path, settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  failUsers: false,
  holidays: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath.startsWith("/absences/calendar?")) return [];
    if (urlPath.startsWith("/holidays?")) return mockState.holidays;
    if (urlPath.startsWith("/time-entries/all?")) {
      return [
        {
          id: 11,
          user_id: 2,
          entry_date: "2026-05-04",
          start_time: "09:00:00",
          end_time: "11:00:00",
          category_id: 7,
          status: "approved",
        },
      ];
    }
    if (urlPath === "/categories") {
      return [{ id: 7, name: "Project", color: "#2f7d32" }];
    }
    if (urlPath === "/users") {
      if (mockState.failUsers) throw new Error("users failed");
      return [
        {
          id: 2,
          first_name: "Tina",
          last_name: "Team",
          role: "employee",
          active: true,
          tracks_time: true,
        },
      ];
    }
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

async function waitForText(target, text, timeout = 10000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent.includes(text)) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`Text not found within ${timeout}ms: ${text}`);
}

async function waitForPath(expectedPath, timeout = 10000) {
  const deadline = Date.now() + timeout;
  let currentPath = "";
  const unsubscribe = path.subscribe((value) => {
    currentPath = value;
  });
  try {
    while (Date.now() < deadline) {
      if (currentPath === expectedPath) return;
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  } finally {
    unsubscribe();
  }
  throw new Error(`Path did not become ${expectedPath}; latest path was ${currentPath}`);
}

describe("Calendar", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    currentUser.set({
      id: 1,
      role: "admin",
      permissions: { can_approve: true },
      tracks_time: true,
    });
    history.replaceState({}, "", "/calendar?year=2026&month=5");
    path.set("/calendar?year=2026&month=5");
    settings.set({ timezone: "UTC" });
    categories.set([]);
    setLanguage("en");
    mockState.failUsers = false;
    mockState.holidays = [];
    api.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("keeps admin team time entries visible when loading users fails", async () => {
    mockState.failUsers = true;

    component = mount(Calendar, { target });
    await settle();

    await waitForText(target, "Team Calendar");
    await waitForText(target, "09:00 - 11:00");
  });

  it("renders all loaded holidays in the visible month", async () => {
    mockState.holidays = [
      {
        id: 1,
        holiday_date: "2026-05-01",
        name: "Tag der Arbeit",
        year: 2026,
        is_auto: true,
      },
      {
        id: 2,
        holiday_date: "2026-05-25",
        name: "Pfingstmontag",
        year: 2026,
        is_auto: true,
      },
    ];

    component = mount(Calendar, { target });
    await settle();

    await waitForText(target, "Tag der Arbeit");
    await waitForText(target, "Pfingstmontag");
  });

  it("allows repeated month navigation clicks without reloading the page", async () => {
    component = mount(Calendar, { target });
    await settle();

    const buttons = target.querySelectorAll(".calendar-top-actions button");
    const previousButton = buttons[0];
    const nextButton = buttons[1];

    previousButton.click();
    await waitForPath("/calendar?year=2026&month=4");
    await waitForText(target, "April 2026");

    previousButton.click();
    await waitForPath("/calendar?year=2026&month=3");
    await waitForText(target, "March 2026");

    nextButton.click();
    await waitForPath("/calendar?year=2026&month=4");
    await waitForText(target, "April 2026");
  });
});
