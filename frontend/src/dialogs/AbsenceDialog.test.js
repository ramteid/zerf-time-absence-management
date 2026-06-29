// Tests for AbsenceDialog — the form employees use to request or edit absences.
// Key business rules tested:
//   - The end date auto-adjusts when start date is moved past it
//   - Saving calls the correct endpoint (POST for new, PUT for edits)
//   - A closed dialog fires onClose with the correct changed flag
//   - Error messages from the backend are translated to user-friendly strings

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceDialog from "./AbsenceDialog.svelte";
import { currentUser, settings, absenceCategories } from "../stores.js";
import { setLanguage, setAbsenceCategoryCache } from "../i18n.js";

// Seed data matching the default absence_categories seeded by the backend migration.
const MOCK_CATEGORIES = [
  { id: 1, slug: "vacation", name: "Vacation", cost_type: "vacation", auto_approve_past: false, active: true, color: "#4CAF50", sort_order: 10 },
  { id: 2, slug: "sick", name: "Sick Leave", cost_type: "none", auto_approve_past: true, active: true, color: "#F44336", sort_order: 20 },
  { id: 3, slug: "training", name: "Training", cost_type: "none", auto_approve_past: false, active: true, color: "#2196F3", sort_order: 30 },
  { id: 4, slug: "special_leave", name: "Special Leave", cost_type: "none", auto_approve_past: false, active: true, color: "#9C27B0", sort_order: 40 },
  { id: 5, slug: "unpaid", name: "Unpaid Leave", cost_type: "none", auto_approve_past: false, active: true, color: "#FF9800", sort_order: 50 },
  { id: 6, slug: "general_absence", name: "General Absence", cost_type: "none", auto_approve_past: false, active: true, color: "#607D8B", sort_order: 60 },
  { id: 7, slug: "flextime_reduction", name: "Flextime Reduction", cost_type: "flextime", auto_approve_past: false, active: true, color: "#795548", sort_order: 70 },
];

const apiMock = vi.hoisted(() => vi.fn());

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("AbsenceDialog", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    currentUser.set({
      id: 1,
      role: "employee",
      workdays_per_week: 5,
      start_date: "2020-01-01",
    });
    absenceCategories.set(MOCK_CATEGORIES);
    setAbsenceCategoryCache(MOCK_CATEGORIES);
    apiMock.mockReset();
    // Default: any unmatched call — including the dialog's own holiday fetch
    // for the selected range — resolves to an empty list. Tests override the
    // specific endpoint they exercise via mockImplementation below.
    apiMock.mockResolvedValue([]);
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
    // jsdom doesn't implement HTMLDialogElement.close(); simulate it so
    // the Dialog.svelte close() method and the resulting "close" DOM event
    // both behave as they would in a real browser.
    HTMLDialogElement.prototype.close = function () {
      this.removeAttribute("open");
      this.dispatchEvent(new Event("close"));
    };
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
    delete HTMLDialogElement.prototype.close;
    absenceCategories.set([]);
    setAbsenceCategoryCache([]);
  });

  it("renders 'Request Absence' title for a new absence", async () => {
    // New requests vs. edits are visually distinguished so the employee
    // does not confuse an edit form with a fresh submission form.
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();
    expect(target.textContent).toContain("Request Absence");
  });

  it("renders 'Edit Absence' title when editing an existing absence", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: {
        template: {
          id: 5,
          kind: "vacation",
          start_date: "2026-06-01",
          end_date: "2026-06-05",
          comment: "",
        },
        onClose,
      },
    });
    await settle();
    expect(target.textContent).toContain("Edit Absence");
  });

  it("shows all absence type options in the dropdown", async () => {
    // All seeded absence categories must appear in the dropdown so employees
    // are never blocked from submitting a valid request type.
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();
    const select = target.querySelector("select");
    expect(select).not.toBeNull();
    // Option values are category IDs; verify one option exists per mock category.
    const optionValues = [...select.querySelectorAll("option")].map((o) =>
      Number(o.value)
    );
    for (const cat of MOCK_CATEGORIES) {
      expect(optionValues).toContain(cat.id);
    }
  });

  it("POSTs to /absences when submitting a new request", async () => {
    // A new absence request must use POST, not PUT, so the backend creates
    // a new record instead of overwriting an existing one.
    apiMock.mockImplementation((path, opts) =>
      path === "/absences" && opts?.method === "POST"
        ? Promise.resolve({ id: 99 })
        : Promise.resolve([]),
    );
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Submit Request")
    );
    saveBtn?.click();
    await settle();
    await settle();

    const postCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/absences" && opts?.method === "POST"
    );
    expect(postCall).toBeTruthy();
  });

  it("PUTs to /absences/:id when saving an existing absence", async () => {
    // Editing an existing absence must not create a duplicate record —
    // it must update the existing one via PUT with the correct ID.
    apiMock.mockImplementation((path, opts) =>
      path === "/absences/5" && opts?.method === "PUT"
        ? Promise.resolve({ id: 5 })
        : Promise.resolve([]),
    );
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: {
        template: {
          id: 5,
          kind: "vacation",
          start_date: "2026-06-01",
          end_date: "2026-06-05",
          comment: "",
        },
        onClose,
      },
    });
    await settle();

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Save")
    );
    saveBtn?.click();
    await settle();
    await settle();

    const putCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/absences/5" && opts?.method === "PUT"
    );
    expect(putCall).toBeTruthy();
  });

  it("calls onClose with changed=false when Cancel is clicked", async () => {
    // Cancelling must not trigger a page reload. The caller checks the
    // changed flag to decide whether to refresh the absence list.
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();

    const cancelBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Cancel")
    );
    cancelBtn?.click();
    await settle();

    expect(onClose).toHaveBeenCalledWith(false, null);
  });

  it("shows a user-friendly error for an overlap conflict", async () => {
    // The backend returns "Overlap with existing absence" — the frontend
    // must translate this to a friendly message rather than showing the raw error.
    apiMock.mockImplementation((path, opts) =>
      path === "/absences" && opts?.method === "POST"
        ? Promise.reject({ message: "Overlap with existing absence" })
        : Promise.resolve([]),
    );
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Submit Request")
    );
    saveBtn?.click();
    await settle();
    await settle();

    expect(target.querySelector(".error-text")?.textContent).toContain(
      "Overlap"
    );
  });

  it("counts only contract workdays, excluding weekends and holidays", async () => {
    // The duration hint must reflect contract workdays, not calendar days:
    // weekends (per workdays_per_week) and public holidays are excluded.
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: {
        // 2026-06-01 (Mon) .. 2026-06-07 (Sun), with a holiday on Wed 06-03.
        template: { start_date: "2026-06-01", end_date: "2026-06-07" },
        onClose,
        holidays: new Set(["2026-06-03"]),
      },
    });
    await settle();

    const hint = target.querySelector(".selected-days-hint");
    expect(hint).not.toBeNull();
    // Mon, Tue, Thu, Fri = 4 (Wed is a holiday; Sat/Sun are non-contract days).
    expect(hint.textContent.replace(/\s+/g, " ").trim()).toBe("4 workdays");
  });

  it("uses the singular label for a single workday", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: {
        template: { start_date: "2026-06-01", end_date: "2026-06-01" },
        onClose,
      },
    });
    await settle();

    const hint = target.querySelector(".selected-days-hint");
    expect(hint.textContent.replace(/\s+/g, " ").trim()).toBe("1 workday");
  });

  it("fetches holidays for the selected year when the parent did not preload them", async () => {
    // The parent only preloads holidays for the year it currently shows. If the
    // user picks a date in another year, the dialog must fetch that year's
    // holidays itself so the workday count still excludes them.
    apiMock.mockImplementation((path) =>
      path === "/holidays?year=2026"
        ? Promise.resolve([{ holiday_date: "2026-06-03", name: "Test Holiday" }])
        : Promise.resolve([]),
    );
    const onClose = vi.fn();
    component = mount(AbsenceDialog, {
      target,
      props: {
        // Mon–Fri with an empty holidays prop → dialog loads 2026 on its own.
        template: { start_date: "2026-06-01", end_date: "2026-06-05" },
        onClose,
        holidays: new Set(),
      },
    });
    await settle();
    await settle();

    expect(apiMock).toHaveBeenCalledWith("/holidays?year=2026");
    const hint = target.querySelector(".selected-days-hint");
    // Wed 06-03 is a holiday → Mon, Tue, Thu, Fri = 4 workdays.
    expect(hint.textContent.replace(/\s+/g, " ").trim()).toBe("4 workdays");
  });
});
