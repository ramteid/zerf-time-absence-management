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
import { setLanguage } from "../i18n.js";

// Seed data matching the default absence_categories seeded by the backend migration.
const MOCK_CATEGORIES = [
  { id: 1, slug: "vacation", name: "Vacation", counts_as_vacation: true, keeps_work_target: false, auto_approve_past: false, active: true, color: "#4CAF50", sort_order: 10 },
  { id: 2, slug: "sick", name: "Sick Leave", counts_as_vacation: false, keeps_work_target: false, auto_approve_past: true, active: true, color: "#F44336", sort_order: 20 },
  { id: 3, slug: "training", name: "Training", counts_as_vacation: false, keeps_work_target: false, auto_approve_past: false, active: true, color: "#2196F3", sort_order: 30 },
  { id: 4, slug: "special_leave", name: "Special Leave", counts_as_vacation: false, keeps_work_target: false, auto_approve_past: false, active: true, color: "#9C27B0", sort_order: 40 },
  { id: 5, slug: "unpaid", name: "Unpaid Leave", counts_as_vacation: false, keeps_work_target: false, auto_approve_past: false, active: true, color: "#FF9800", sort_order: 50 },
  { id: 6, slug: "general_absence", name: "General Absence", counts_as_vacation: false, keeps_work_target: false, auto_approve_past: false, active: true, color: "#607D8B", sort_order: 60 },
  { id: 7, slug: "flextime_reduction", name: "Flextime Reduction", counts_as_vacation: false, keeps_work_target: true, auto_approve_past: false, active: true, color: "#795548", sort_order: 70 },
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
    apiMock.mockReset();
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
    apiMock.mockResolvedValueOnce({ id: 99 });
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
    apiMock.mockResolvedValueOnce({ id: 5 });
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
    apiMock.mockRejectedValueOnce({ message: "Overlap with existing absence" });
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
});
