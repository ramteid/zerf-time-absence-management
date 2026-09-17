import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Time from "./Time.svelte";
import { currentUser, path, settings, absenceCategories } from "../stores.js";
import { setLanguage, setAbsenceCategoryCache } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  entries: [],
  absences: [],
  holidays: [],
  reopens: [],
  categories: [],
  // What the server says each day of the shown week asks for. The page no
  // longer works this out: which weekdays a contract works, and what one of
  // them is worth, are decided once, on the server.
  rangeReport: null,
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
    if (urlPath.startsWith("/reports/range")) return mockState.rangeReport;
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

// Five working days of eight hours each, in the shape the range report returns.
function weekOfEightHourDays(monday) {
  return {
    days: Array.from({ length: 5 }, (_, index) => {
      const date = new Date(`${monday}T12:00:00`);
      date.setDate(date.getDate() + index);
      return {
        date: date.toISOString().slice(0, 10),
        target_min: 480,
        full_target_min: 480,
      };
    }),
  };
}

// Returns a Monday date string for a past week (to avoid future-day disabling).
function pastMonday() {
  const now = new Date();
  const day = now.getDay();
  const diff = day === 0 ? 6 : day - 1;
  const mon = new Date(now);
  mon.setDate(mon.getDate() - diff - 7); // last week's Monday
  const year = mon.getFullYear();
  const month = String(mon.getMonth() + 1).padStart(2, "0");
  const dayOfMonth = String(mon.getDate()).padStart(2, "0");
  return `${year}-${month}-${dayOfMonth}`;
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
    // A plain five-day, eight-hour week unless a test says otherwise. The
    // server decides these numbers now, so a test that is not about them just
    // needs them present.
    mockState.rangeReport = weekOfEightHourDays(pastMonday());
    setLanguage("en");
    // Seed the absenceCategories store so absenceRemovesTarget / absenceBlocksEntry
    // (which read cost_type / auto_approve_past from the store) behave
    // correctly. Without this they fall back to "removes target" / "blocks entry"
    // for every kind, which breaks every flextime-reduction-related assertion.
    const cats = [
      {
        id: 1,
        slug: "vacation",
        name: "Vacation",
        cost_type: "vacation",
        auto_approve_past: false,
      },
      {
        id: 2,
        slug: "sick",
        name: "Sick",
        cost_type: "none",
        auto_approve_past: true,
      },
      {
        id: 3,
        slug: "flextime_reduction",
        name: "Flextime Reduction",
        cost_type: "flextime",
        auto_approve_past: false,
      },
    ];
    absenceCategories.set(cats);
    setAbsenceCategoryCache(cats);
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

  it("shows daily total hours with two decimal places", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.categories = [
      {
        id: 1,
        name: "Core Duties",
        counts_as_work: true,
        color: "#336699",
      },
    ];
    mockState.entries = [
      {
        id: 1,
        user_id: 1,
        entry_date: monday,
        start_time: "08:00",
        end_time: "15:30",
        category_id: 1,
        status: "draft",
      },
    ];

    component = mount(Time, { target });
    await settle();

    expect(
      target.querySelector(".day-card .day-total").textContent.trim(),
    ).toBe("7.50h");
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
    const addButtons = target.querySelectorAll("button.add-entry-btn");
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

    const addButtons = target.querySelectorAll("button.add-entry-btn");
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

    const addButtons = target.querySelectorAll("button.add-entry-btn");
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

    const addButtons = target.querySelectorAll("button.add-entry-btn");
    const mondayButton = addButtons[0];
    expect(mondayButton).toBeDefined();
    expect(mondayButton.disabled).toBe(false);
  });

  it("marks a day done against the target the server gave it", async () => {
    // A day card marks itself done once the day's booked hours reach what that
    // day asks for, and what it asks for comes from the server. The targets are
    // released separately here so the assertion cannot pass on a card that was
    // rendered before they existed.
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);
    mockState.entries = [
      {
        id: 102,
        user_id: 1,
        entry_date: monday,
        start_time: "08:00",
        end_time: "16:00",
        category_id: 1,
        status: "approved",
      },
    ];
    let releaseTargets;
    mockState.rangeReport = new Promise((resolve) => {
      releaseTargets = resolve;
    });

    component = mount(Time, { target });
    await settle();
    // Nothing to compare against yet, so nothing is marked done.
    expect(target.querySelector(".day-total.target-met")).toBeNull();

    releaseTargets(weekOfEightHourDays(monday));
    await settle();

    expect(target.querySelector(".day-total.target-met")).not.toBeNull();
  });

  it("shows the weekly target the server reported for this week", async () => {
    // The page adds up what each day asks for and prints the total. Which
    // weekdays a contract works, what one of them is worth, and whether an
    // absence removes it are all decided on the server, so there is nothing
    // left here to get wrong a second time.
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);
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
    mockState.rangeReport = weekOfEightHourDays(monday);

    component = mount(Time, { target });
    await settle();

    expect(target.textContent).toContain("of 40.00h target");
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

    expect(target.textContent).toContain("of 40.00h target");
  });

  it("uses entry counts_as_work when category lookup is unavailable", async () => {
    const monday = pastMonday();
    path.set(`/time?week=${monday}`);

    mockState.categories = [
      { id: 1, name: "Core Duties", counts_as_work: true },
    ];
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

    expect(target.textContent).toContain("of 40.00h target");
  });
});
