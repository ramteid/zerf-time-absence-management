import { afterEach, beforeEach, describe, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Calendar from "./Calendar.svelte";
import { api } from "../api.js";
import { categories, currentUser, path, settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  failUsers: false,
  holidays: [],
  absences: [],
  timeEntries: [
    {
      id: 11,
      user_id: 2,
      entry_date: "2026-05-04",
      start_time: "09:00:00",
      end_time: "11:00:00",
      category_id: 7,
      status: "approved",
    },
  ],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath.startsWith("/absences/calendar?")) return mockState.absences;
    if (urlPath.startsWith("/holidays?")) return mockState.holidays;
    if (urlPath.startsWith("/time-entries/all?")) return mockState.timeEntries;
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
  throw new Error(
    `Path did not become ${expectedPath}; latest path was ${currentPath}`,
  );
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

    try {
      component = mount(Calendar, { target });
      await settle();
    } finally {
      process.off("uncaughtException", onUncaught);
      process.off("unhandledRejection", onUncaught);
    }

    if (capturedErrors.length > 0) {
      throw capturedErrors[0];
    }

    // The day grid shows the category label (e.g. "Vacation"), not the
    // person's name — the name only appears in the click-through popup — so
    // assert on the label, and specifically that BOTH overlapping absences
    // render as distinct events rather than being silently collapsed into
    // one (the crash isn't the only possible failure mode of a duplicate key).
    await waitForText(target, "Vacation");
    const vacationEvents = Array.from(
      target.querySelectorAll(".cal-event"),
    ).filter((el) => el.textContent.trim() === "Vacation");
    if (vacationEvents.length < 2) {
      throw new Error(
        `expected at least 2 separate "Vacation" events to render (one per overlapping absence), found ${vacationEvents.length}`,
      );
    }
  });

  it("filters time and absence categories independently by clicking legend items", async () => {
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

    // Both the time entry and absence should be visible initially
    await waitForText(target, "Project");
    await waitForText(target, "Vacation");

    // The legend should have both items
    const legendButtons = target.querySelectorAll(".cal-legend-item");
    const projectButton = Array.from(legendButtons).find((btn) =>
      btn.textContent.includes("Project"),
    );
    const vacationButton = Array.from(legendButtons).find((btn) =>
      btn.textContent.includes("Vacation"),
    );

    if (!projectButton || !vacationButton) {
      throw new Error("Could not find Project or Vacation filter buttons");
    }

    if (projectButton.classList.contains("inactive")) {
      throw new Error("Project filter should be active initially");
    }

    // Click the Project filter to hide it
    projectButton.click();
    await settle();

    // Check that the Project button is now inactive
    const updatedProjectButton = Array.from(
      target.querySelectorAll(".cal-legend-item"),
    ).find((btn) => btn.textContent.includes("Project"));

    if (!updatedProjectButton.classList.contains("inactive")) {
      throw new Error("Project filter should be inactive after clicking");
    }

    // Vacation should still be visible (active)
    const updatedVacationButton = Array.from(
      target.querySelectorAll(".cal-legend-item"),
    ).find((btn) => btn.textContent.includes("Vacation"));

    if (updatedVacationButton.classList.contains("inactive")) {
      throw new Error("Vacation filter should remain active");
    }

    // Re-enable the Project filter
    updatedProjectButton.click();
    await settle();

    const finalProjectButton = Array.from(
      target.querySelectorAll(".cal-legend-item"),
    ).find((btn) => btn.textContent.includes("Project"));

    if (finalProjectButton.classList.contains("inactive")) {
      throw new Error("Project filter should be active again after re-clicking");
    }
  });
});
