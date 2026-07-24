import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminHolidays from "./AdminHolidays.svelte";
import { settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const apiMock = vi.hoisted(() => vi.fn());

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
}));

vi.mock("../confirm.js", () => ({
  confirmDialog: vi.fn().mockResolvedValue(true),
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
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`Text not found: "${text}"`);
}

describe("AdminHolidays", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    apiMock.mockReset();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("renders Holidays heading", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "Holidays");
  });

  it("renders holiday list", async () => {
    apiMock.mockResolvedValue([
      {
        id: 1,
        holiday_date: "2026-01-01",
        name: "New Year's Day",
        is_auto: false,
      },
      {
        id: 2,
        holiday_date: "2026-12-25",
        name: "Christmas Day",
        is_auto: true,
      },
    ]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "New Year's Day");
    await waitForText(target, "Christmas Day");
  });

  it("shows API badge for auto-imported holidays", async () => {
    apiMock.mockResolvedValue([
      {
        id: 2,
        holiday_date: "2026-12-25",
        name: "Christmas Day",
        is_auto: true,
      },
    ]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "API");
  });

  it("shows 'No holidays' when list is empty", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "No holidays");
  });

  it("navigates to previous year when prev button is clicked", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "Holidays");

    const prevBtn = [...target.querySelectorAll("button")].find((b) =>
      b.querySelector("svg"),
    );
    const currentYear = new Date().getFullYear();
    prevBtn.click();
    await settle();

    expect(target.textContent).toContain(String(currentYear - 1));
  });

  it("shows validation toast when adding holiday without date or name", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "Holidays");

    // Click Add without filling date/name
    const addBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Add"),
    );
    expect(addBtn).not.toBeNull();
    addBtn.click();
    await settle();

    // The API should NOT have been called for POST
    const postCall = apiMock.mock.calls.find(
      ([, opts]) => opts?.method === "POST",
    );
    expect(postCall).toBeUndefined();
  });

  it("shows a Recurring badge for holidays marked as recurring", async () => {
    apiMock.mockResolvedValue([
      {
        id: 3,
        holiday_date: "2026-12-24",
        name: "Heiligabend",
        is_auto: false,
        recurring: true,
        recurrence_end_year: null,
      },
    ]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "Recurring");
  });

  it("keeps the end-year select hidden until 'Repeats every year' is checked", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "Holidays");

    expect(target.querySelector("select.end-year-select")).toBeNull();

    const checkboxes = () => [
      ...target.querySelectorAll('input[type="checkbox"]'),
    ];
    const recurringCheckbox = checkboxes().find((cb) =>
      cb.closest("label")?.textContent?.includes("Repeats every year"),
    );
    const endCheckbox = checkboxes().find((cb) =>
      cb.closest("label")?.textContent?.includes("End"),
    );
    expect(recurringCheckbox).not.toBeNull();
    expect(endCheckbox).not.toBeNull();
    expect(endCheckbox.disabled).toBe(true);

    recurringCheckbox.click();
    await settle();
    expect(endCheckbox.disabled).toBe(false);

    endCheckbox.click();
    await settle();
    expect(target.querySelector("select.end-year-select")).not.toBeNull();

    // Unchecking "Repeats every year" hides the end-year option again.
    recurringCheckbox.click();
    await settle();
    expect(target.querySelector("select.end-year-select")).toBeNull();
    expect(endCheckbox.checked).toBe(false);
  });

  it("sends recurring and recurrence_end_year in the add payload", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminHolidays, { target });
    await waitForText(target, "Holidays");

    const nameInput = target.querySelector("#holiday-name");
    nameInput.value = "Heiligabend";
    nameInput.dispatchEvent(new Event("input"));

    const dateButton = target.querySelector(".date-picker-button");
    expect(dateButton).not.toBeNull();
    dateButton.click();
    await settle();
    const calendar = document.querySelector(".flatpickr-calendar.open");
    expect(calendar).not.toBeNull();
    const todayCell = calendar.querySelector(".flatpickr-day.today");
    expect(todayCell).not.toBeNull();
    todayCell.click();
    await settle();

    const checkboxes = () => [
      ...target.querySelectorAll('input[type="checkbox"]'),
    ];
    checkboxes()
      .find((cb) =>
        cb.closest("label")?.textContent?.includes("Repeats every year"),
      )
      .click();
    await settle();
    checkboxes()
      .find((cb) => cb.closest("label")?.textContent?.includes("End"))
      .click();
    await settle();

    const addBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Add"),
    );
    addBtn.click();
    await settle();

    const postCall = apiMock.mock.calls.find(
      ([, opts]) => opts?.method === "POST",
    );
    expect(postCall).toBeDefined();
    expect(postCall[1].body.recurring).toBe(true);
    expect(typeof postCall[1].body.recurrence_end_year).toBe("number");
  });
});
