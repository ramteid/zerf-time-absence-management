import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Calendar from "./Calendar.svelte";
import { api } from "../api.js";
import { categories, currentUser, path, settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const DEFAULT_USER = {
  id: 2,
  first_name: "Tina",
  last_name: "Team",
  role: "employee",
  active: true,
  tracks_time: true,
};

const DEFAULT_TIME_ENTRIES = [
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

const mockState = vi.hoisted(() => ({
  failUsers: false,
  holidays: [],
  absences: [],
  timeEntries: [],
  // Returned by /time-entries (the viewer's own entries). Employees only ever
  // hit this endpoint; leads hit it in addition to /time-entries/all.
  ownTimeEntries: [],
  users: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath.startsWith("/absences/calendar?")) return mockState.absences;
    if (urlPath.startsWith("/holidays?")) return mockState.holidays;
    if (urlPath.startsWith("/time-entries/all?")) return mockState.timeEntries;
    if (urlPath.startsWith("/time-entries?")) return mockState.ownTimeEntries;
    if (urlPath === "/categories") {
      return [{ id: 7, name: "Project", color: "#2f7d32" }];
    }
    if (urlPath === "/users") {
      if (mockState.failUsers) throw new Error("users failed");
      return mockState.users;
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
  throw new Error(
    `Path did not become ${expectedPath}; latest path was ${currentPath}`,
  );
}

// Open a day cell's popup and return its rendered groups: one entry per
// category, each with its colour swatch, its title and its rows.
async function openDay(target, dateString) {
  const dayButton = target.querySelector(`.cal-day[data-date="${dateString}"]`);
  if (!dayButton) throw new Error(`No day cell rendered for ${dateString}`);
  dayButton.click();
  await settle();
  const dialog = document.querySelector(".cal-popup");
  if (!dialog) throw new Error(`Clicking ${dateString} did not open a popup`);
  return Array.from(dialog.querySelectorAll(".cal-popup-group")).map(
    (group) => ({
      label: group.querySelector(".cal-popup-group-head span:last-child")
        .textContent,
      rows: Array.from(group.querySelectorAll(".cal-popup-row")).map((row) => ({
        primary: row.querySelector(".cal-popup-primary").textContent,
        secondary:
          row.querySelector(".cal-popup-secondary")?.textContent ?? null,
      })),
    }),
  );
}

function closePopup() {
  const dialog = document.querySelector("dialog");
  const closeButton = Array.from(dialog.querySelectorAll("button")).find(
    (button) => button.textContent.trim() === "Close",
  );
  closeButton.click();
}

// Open the category filter menu and return its options as
// `{ label, visible }` pairs, in the order the menu lists them.
async function openFilterMenu(target) {
  const trigger = target.querySelector(".cal-filter-trigger");
  if (!trigger) throw new Error("No category filter button rendered");
  if (trigger.getAttribute("aria-expanded") !== "true") {
    trigger.click();
    await settle();
  }
  return filterOptions(target);
}

function filterOptions(target) {
  return Array.from(target.querySelectorAll(".cal-filter-option")).map(
    (option) => ({
      label: option.querySelector(".cal-filter-label").textContent,
      visible: option.getAttribute("aria-checked") === "true",
    }),
  );
}

// Click one category in the open filter menu, by its label.
async function clickFilterOption(target, label) {
  const option = Array.from(target.querySelectorAll(".cal-filter-option")).find(
    (el) => el.querySelector(".cal-filter-label").textContent === label,
  );
  if (!option) throw new Error(`No filter option labelled ${label}`);
  option.click();
  await settle();
}

async function clickFilterAction(target, label) {
  const button = Array.from(
    target.querySelectorAll(".cal-filter-actions .zf-btn"),
  ).find((el) => el.textContent.trim() === label);
  if (!button) throw new Error(`No filter action labelled ${label}`);
  button.click();
  await settle();
}

// The chips rendered inside one day cell, as `{ title, count }` pairs.
function dayChips(target, dateString) {
  const dayButton = target.querySelector(`.cal-day[data-date="${dateString}"]`);
  if (!dayButton) throw new Error(`No day cell rendered for ${dateString}`);
  return Array.from(dayButton.querySelectorAll(".cal-event")).map((chip) => ({
    title: chip.querySelector(".cal-event-title").textContent,
    count: chip.querySelector(".cal-event-count")?.textContent ?? null,
  }));
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
    mockState.absences = [];
    mockState.timeEntries = DEFAULT_TIME_ENTRIES;
    mockState.ownTimeEntries = [];
    mockState.users = [DEFAULT_USER];
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
    // The grid shows the category; the times are one click away in the popup.
    await waitForText(target, "Project");
    const groups = await openDay(target, "2026-05-04");
    expect(groups).toEqual([
      {
        label: "Project",
        rows: [{ primary: "09:00 - 11:00 (2:00)", secondary: null }],
      },
    ]);
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

    const previousButton = target.querySelector(
      '[aria-label="Previous month"]',
    );
    const nextButton = target.querySelector('[aria-label="Next month"]');

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

  it("calculates repeated navigation from the latest path state", async () => {
    component = mount(Calendar, { target });
    await settle();

    path.set("/calendar?year=2026&month=11");
    history.replaceState({}, "", "/calendar?year=2026&month=11");
    await settle();

    const nextButton = target.querySelector('[aria-label="Next month"]');

    nextButton.click();
    await waitForPath("/calendar?year=2026&month=12");
    await waitForText(target, "December 2026");

    nextButton.click();
    await waitForPath("/calendar?year=2027&month=1");
    await waitForText(target, "January 2027");
  });

  it("renders two different people's overlapping same-category absences on one day without crashing", async () => {
    // Regression test for a Svelte `each_key_duplicate` crash: the calendar's
    // event key used to be derived only from the absence/category kind
    // (`absence:vacation`), not the record itself. Two different employees
    // with overlapping absences of the same kind on the same day (e.g.
    // overlapping vacations during the summer holiday period) then produced
    // two events with an identical key inside the day cell's keyed
    // `{#each ev.key}` block. Svelte throws on that duplicate key, and
    // because the throw happens inside Svelte's own render effect (not
    // synchronously inside `mount()`), it surfaces as an unhandled
    // exception/rejection that aborts the whole component's render — the
    // grid stays up but every event and the legend disappear, even though
    // the fetched data was correct. This test fails on the old behaviour
    // (via the captured uncaught-exception/rejection below) and only
    // passes once each event key is unique per record.
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Alice Approver",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-10",
        end_date: "2026-05-12",
        comment: null,
        status: "approved",
      },
      {
        id: 102,
        user_id: 3,
        name: "Bob Report",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-10",
        end_date: "2026-05-14",
        comment: null,
        status: "approved",
      },
    ];

    const capturedErrors = [];
    const onUncaught = (err) => capturedErrors.push(err);
    process.on("uncaughtException", onUncaught);
    process.on("unhandledRejection", onUncaught);

    let groups;
    try {
      component = mount(Calendar, { target });
      await settle();
      await waitForText(target, "Vacation");
      // The popup's rows are keyed per record too, so open it inside the
      // capture window as well.
      groups = await openDay(target, "2026-05-11");
    } finally {
      process.off("uncaughtException", onUncaught);
      process.off("unhandledRejection", onUncaught);
    }

    if (capturedErrors.length > 0) {
      throw capturedErrors[0];
    }

    // The day grid shows one chip per category — two people on vacation is
    // still a single "Vacation" chip, carrying the number of people — while
    // the popup lists both of them, so neither absence is lost.
    expect(dayChips(target, "2026-05-11")).toEqual([
      { title: "Vacation", count: "2" },
    ]);
    expect(groups).toEqual([
      {
        label: "Vacation",
        rows: [
          { primary: "Alice Approver", secondary: null },
          { primary: "Bob Report", secondary: null },
        ],
      },
    ]);
  });

  it("shows only the picked category on the first click, then toggles category by category", async () => {
    mockState.holidays = [
      {
        id: 1,
        holiday_date: "2026-05-01",
        name: "May Day",
        year: 2026,
        is_auto: true,
      },
    ];
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Tina Team",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: null,
        status: "approved",
      },
    ];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Project");

    // The menu lists every category of the month, in the same holiday →
    // absence → work order the day cells and the popup use, all visible.
    expect(await openFilterMenu(target)).toEqual([
      { label: "Holiday", visible: true },
      { label: "Vacation", visible: true },
      { label: "Project", visible: true },
    ]);

    // First click out of the unfiltered state focuses that one category
    // instead of hiding it — one click, not one click per other category.
    await clickFilterOption(target, "Vacation");
    expect(filterOptions(target)).toEqual([
      { label: "Holiday", visible: false },
      { label: "Vacation", visible: true },
      { label: "Project", visible: false },
    ]);
    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Vacation",
    ]);

    // With a filter active, further clicks plainly toggle one category.
    await clickFilterOption(target, "Project");
    expect(filterOptions(target)).toEqual([
      { label: "Holiday", visible: false },
      { label: "Vacation", visible: true },
      { label: "Project", visible: true },
    ]);
    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Vacation",
      "Project",
    ]);

    await clickFilterOption(target, "Vacation");
    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Project",
    ]);
  });

  it("hides every category at once and brings them all back", async () => {
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Tina Team",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: null,
        status: "approved",
      },
    ];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Project");
    await openFilterMenu(target);

    await clickFilterAction(target, "Hide all");
    expect(filterOptions(target).every((option) => !option.visible)).toBe(true);
    expect(target.querySelectorAll(".cal-event")).toHaveLength(0);
    // Nothing is left to click through to, so no day cell stays clickable.
    expect(target.querySelectorAll(".cal-day.has-events")).toHaveLength(0);

    await clickFilterAction(target, "Show all");
    expect(filterOptions(target).every((option) => option.visible)).toBe(true);
    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Vacation",
      "Project",
    ]);
  });

  it("drops the filter for a category the next month does not contain", async () => {
    // A filter is only meaningful against categories that are on screen: after
    // navigating away from the month that had the absence, the menu must not
    // still claim something is filtered out.
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Tina Team",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: null,
        status: "approved",
      },
    ];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Vacation");
    await openFilterMenu(target);

    // Focus the absence, so the work category is the hidden one.
    await clickFilterOption(target, "Vacation");
    expect(target.querySelector(".cal-filter-count").textContent).toBe("1/2");

    // June has neither the absence nor the time entry.
    mockState.absences = [];
    mockState.timeEntries = [];
    target.querySelector('[aria-label="Next month"]').click();
    await waitForPath("/calendar?year=2026&month=6");
    await settle();

    // No categories at all — the filter button has nothing to offer and goes.
    expect(target.querySelector(".cal-filter-trigger")).toBe(null);

    // Back in May everything is shown again rather than still filtered.
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Tina Team",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: null,
        status: "approved",
      },
    ];
    mockState.timeEntries = DEFAULT_TIME_ENTRIES;
    target.querySelector('[aria-label="Previous month"]').click();
    await waitForPath("/calendar?year=2026&month=5");
    await settle();
    await waitForText(target, "Vacation");

    expect(target.querySelector(".cal-filter-count")).toBe(null);
    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Vacation",
      "Project",
    ]);
  });

  it("shows one chip per time category, titled by the category and not by the employee", async () => {
    // Three entries from two people in the same category used to render three
    // chips, each starting with the employee's name. The day cell is about
    // what was worked on, so it shows the category once, with how many
    // records it covers.
    mockState.users = [
      DEFAULT_USER,
      {
        id: 3,
        first_name: "Ben",
        last_name: "Busy",
        role: "employee",
        active: true,
        tracks_time: true,
      },
    ];
    mockState.timeEntries = [
      {
        id: 11,
        user_id: 2,
        entry_date: "2026-05-04",
        start_time: "09:00:00",
        end_time: "11:00:00",
        category_id: 7,
        status: "approved",
      },
      {
        id: 12,
        user_id: 3,
        entry_date: "2026-05-04",
        start_time: "13:00:00",
        end_time: "17:00:00",
        category_id: 7,
        status: "approved",
      },
      {
        id: 13,
        user_id: 3,
        entry_date: "2026-05-04",
        start_time: "08:00:00",
        end_time: "09:00:00",
        category_id: 7,
        status: "approved",
      },
    ];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Project");

    expect(dayChips(target, "2026-05-04")).toEqual([
      { title: "Project", count: "3" },
    ]);

    // The popup lists every record, sorted by employee and then by time.
    const groups = await openDay(target, "2026-05-04");
    expect(groups).toEqual([
      {
        label: "Project",
        rows: [
          { primary: "Ben Busy", secondary: "08:00 - 09:00 (1:00)" },
          { primary: "Ben Busy", secondary: "13:00 - 17:00 (4:00)" },
          { primary: "Tina Team", secondary: "09:00 - 11:00 (2:00)" },
        ],
      },
    ]);
  });

  it("opens the same grouped popup for a day holding a holiday, an absence and working time", async () => {
    mockState.holidays = [
      {
        id: 1,
        holiday_date: "2026-05-04",
        name: "May Day",
        year: 2026,
        is_auto: true,
      },
    ];
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Tina Team",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: "Long weekend",
        status: "approved",
      },
    ];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Project");

    // Chips and popup groups follow the same order everywhere: holiday first,
    // then absences, then work categories.
    expect(dayChips(target, "2026-05-04")).toEqual([
      { title: "May Day", count: null },
      { title: "Vacation", count: null },
      { title: "Project", count: null },
    ]);

    const groups = await openDay(target, "2026-05-04");
    expect(groups).toEqual([
      { label: "Holiday", rows: [{ primary: "May Day", secondary: null }] },
      {
        label: "Vacation",
        rows: [{ primary: "Tina Team", secondary: "Long weekend" }],
      },
      {
        label: "Project",
        rows: [{ primary: "Tina Team", secondary: "09:00 - 11:00 (2:00)" }],
      },
    ]);

    // Every row sits in its group's row container, which carries the single
    // fixed indent — no row is indented by its own content width.
    const popup = document.querySelector(".cal-popup");
    const rows = Array.from(popup.querySelectorAll(".cal-popup-row"));
    expect(rows).toHaveLength(3);
    for (const row of rows) {
      expect(row.parentElement.classList.contains("cal-popup-rows")).toBe(true);
      expect(row.querySelectorAll(".cal-popup-primary")).toHaveLength(1);
    }
  });

  it("names the viewer's own entries in the popup even without a team lookup", async () => {
    // An employee never loads /users, so their own name has to come from the
    // session — otherwise their rows would be the only nameless ones.
    currentUser.set({
      id: 2,
      first_name: "Tina",
      last_name: "Team",
      role: "employee",
      permissions: { can_approve: false },
      tracks_time: true,
    });
    // Employees see only their own entries, and get no /users lookup at all.
    mockState.timeEntries = [];
    mockState.ownTimeEntries = DEFAULT_TIME_ENTRIES;
    mockState.users = [];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Project");

    const groups = await openDay(target, "2026-05-04");
    expect(groups).toEqual([
      {
        label: "Project",
        rows: [{ primary: "Tina Team", secondary: "09:00 - 11:00 (2:00)" }],
      },
    ]);
  });

  it("hides a filtered-out category from both the day cells and the popup", async () => {
    mockState.absences = [
      {
        id: 101,
        user_id: 2,
        name: "Tina Team",
        kind: "vacation",
        category_name: "Vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: null,
        status: "approved",
      },
    ];

    component = mount(Calendar, { target });
    await settle();
    await waitForText(target, "Vacation");

    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Vacation",
      "Project",
    ]);

    await openFilterMenu(target);
    // Focusing "Project" leaves the absence hidden.
    await clickFilterOption(target, "Project");

    expect(dayChips(target, "2026-05-04").map((chip) => chip.title)).toEqual([
      "Project",
    ]);
    const groups = await openDay(target, "2026-05-04");
    expect(groups.map((group) => group.label)).toEqual(["Project"]);
    closePopup();
    await settle();
  });
});
