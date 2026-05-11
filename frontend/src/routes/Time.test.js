import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Time from "./Time.svelte";
import { currentUser, path, settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  entries: [],
  absences: [],
  holidays: [],
  reopens: [],
  categories: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath.startsWith("/time-entries")) return mockState.entries;
    if (urlPath.startsWith("/reopen-requests")) return mockState.reopens;
    if (urlPath.startsWith("/categories")) return mockState.categories;
    if (urlPath.startsWith("/absences")) return mockState.absences;
    if (urlPath.startsWith("/holidays")) return mockState.holidays;
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

// Returns a Monday date string for a past week (to avoid future-day disabling).
function pastMonday() {
  const now = new Date();
  const day = now.getDay();
  const diff = day === 0 ? 6 : day - 1;
  const mon = new Date(now);
  mon.setDate(mon.getDate() - diff - 7); // last week's Monday
  return mon.toISOString().slice(0, 10);
}

describe("Time", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    currentUser.set({
      id: 1,
      role: "employee",
      weekly_hours: 40,
      workdays_per_week: 5,
      start_date: "2020-01-01",
    });
    settings.set({ time_format: "24h" });
    setLanguage("en");
    mockState.entries = [];
    mockState.absences = [];
    mockState.holidays = [];
    mockState.reopens = [];
    mockState.categories = [];
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("cancelled absences do not block time entry creation", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.absences = [
      {
        id: 10,
        user_id: 1,
        kind: "vacation",
        start_date: monday,
        end_date: monday,
        status: "cancelled",
        comment: null,
      },
    ];

    component = mount(Time, { target });
    await settle();

    // The add-entry buttons should not be disabled for the Monday that only
    // has a cancelled absence.
    const addButtons = target.querySelectorAll("button[style*='dashed']");
    const mondayButton = addButtons[0];
    expect(mondayButton).toBeDefined();
    expect(mondayButton.disabled).toBe(false);
  });

  it("approved absences block time entry creation", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.absences = [
      {
        id: 11,
        user_id: 1,
        kind: "vacation",
        start_date: monday,
        end_date: monday,
        status: "approved",
        comment: null,
      },
    ];

    component = mount(Time, { target });
    await settle();

    const addButtons = target.querySelectorAll("button[style*='dashed']");
    const mondayButton = addButtons[0];
    expect(mondayButton).toBeDefined();
    expect(mondayButton.disabled).toBe(true);
  });

  it("approved flextime reduction absences still block time entry creation", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.absences = [
      {
        id: 15,
        user_id: 1,
        kind: "flextime_reduction",
        start_date: monday,
        end_date: monday,
        status: "approved",
        comment: null,
      },
    ];

    component = mount(Time, { target });
    await settle();

    const addButtons = target.querySelectorAll("button[style*='dashed']");
    const mondayButton = addButtons[0];
    expect(mondayButton).toBeDefined();
    expect(mondayButton.disabled).toBe(true);
  });

  it("rejected absences do not block time entry creation", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.absences = [
      {
        id: 12,
        user_id: 1,
        kind: "vacation",
        start_date: monday,
        end_date: monday,
        status: "rejected",
        comment: null,
      },
    ];

    component = mount(Time, { target });
    await settle();

    const addButtons = target.querySelectorAll("button[style*='dashed']");
    const mondayButton = addButtons[0];
    expect(mondayButton).toBeDefined();
    expect(mondayButton.disabled).toBe(false);
  });

  it("requested absences do not reduce weekly target", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    // Keep one approved entry so the summary strip is rendered.
    mockState.entries = [
      {
        id: 100,
        user_id: 1,
        entry_date: monday,
        start_time: "08:00",
        end_time: "12:00",
        category_id: 1,
        status: "approved",
      },
    ];

    // Important regression case: requested absences must not remove target time.
    mockState.absences = [
      {
        id: 13,
        user_id: 1,
        kind: "vacation",
        start_date: monday,
        end_date: monday,
        status: "requested",
        comment: null,
      },
    ];

    component = mount(Time, { target });
    await settle();

    expect(target.textContent).toContain("of 40.0h target");
  });

  it("flextime reduction absences keep the weekly target", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.entries = [
      {
        id: 101,
        user_id: 1,
        entry_date: monday,
        start_time: "08:00",
        end_time: "12:00",
        category_id: 1,
        status: "approved",
      },
    ];
    mockState.absences = [
      {
        id: 14,
        user_id: 1,
        kind: "flextime_reduction",
        start_date: monday,
        end_date: monday,
        status: "approved",
        comment: null,
      },
    ];
    mockState.categories = [{ id: 1, name: "Core Duties", counts_as_work: true }];

    component = mount(Time, { target });
    await settle();

    expect(target.textContent).toContain("of 40.0h target");
  });

  it("flextime reduction entries do not add credited weekly hours", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.categories = [
      { id: 1, name: "Core Duties", counts_as_work: true },
      { id: 2, name: "Flextime Reduction", counts_as_work: false },
    ];
    mockState.entries = [
      {
        id: 102,
        user_id: 1,
        entry_date: monday,
        start_time: "08:00",
        end_time: "12:00",
        category_id: 1,
        status: "approved",
      },
      {
        id: 103,
        user_id: 1,
        entry_date: monday,
        start_time: "13:00",
        end_time: "17:00",
        category_id: 2,
        status: "approved",
      },
    ];

    component = mount(Time, { target });
    await settle();

    expect(target.textContent).toContain("of 40.0h target");
    expect(target.textContent).toContain("Logged: 4.0h");
  });

  it("uses entry counts_as_work when category lookup is unavailable", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.categories = [{ id: 1, name: "Core Duties", counts_as_work: true }];
    mockState.entries = [
      {
        id: 104,
        user_id: 1,
        entry_date: monday,
        start_time: "08:00",
        end_time: "12:00",
        category_id: 999,
        counts_as_work: false,
        status: "approved",
      },
    ];

    component = mount(Time, { target });
    await settle();

    expect(target.textContent).toContain("of 40.0h target");
    expect(target.textContent).toContain("Logged: 0.0h");
  });
});
