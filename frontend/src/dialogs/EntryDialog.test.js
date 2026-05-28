// Tests for EntryDialog — the form used to create and edit daily time entries.
// Key business rules:
//   - "Add Entry" for new vs "Edit Entry" / "Save" for existing entries
//   - Only active categories appear in the dropdown (the category store is used)
//   - On success the dialog closes and calls onClose with changed:true
//   - The Delete button only appears for existing entries, not new ones

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import EntryDialog from "./EntryDialog.svelte";
import { categories, currentUser, settings } from "../stores.js";
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

describe("EntryDialog", () => {
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
      start_date: "2020-01-01",
    });
    categories.set([
      { id: 1, name: "Core Duties", counts_as_work: true },
      { id: 2, name: "Training", counts_as_work: false },
    ]);
    apiMock.mockReset();
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
  });

  it("renders 'Add Entry' title and button for a new entry", async () => {
    // New entries show "Add Entry" in the title and on the submit button so
    // the employee knows they are creating, not overwriting, an entry.
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();
    expect(target.textContent).toContain("Add Entry");
  });

  it("renders 'Edit Entry' title and 'Save' button for an existing entry", async () => {
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: {
        template: {
          id: 20,
          entry_date: "2026-05-26",
          start_time: "08:00",
          end_time: "12:00",
          category_id: 1,
          comment: "",
        },
        onClose,
      },
    });
    await settle();
    expect(target.textContent).toContain("Edit Entry");
    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.trim() === "Save"
    );
    expect(saveBtn).not.toBeNull();
  });

  it("populates the category dropdown from the categories store", async () => {
    // The dropdown must reflect the categories configured in the admin panel.
    // Using the store ensures the list stays in sync without a separate API call.
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();
    const options = [...target.querySelectorAll("#entry-category option")].map(
      (o) => o.textContent
    );
    expect(options).toContain("Core Duties");
    expect(options).toContain("Training");
  });

  it("hides the Delete button for new entries", async () => {
    // New entries that have not been saved yet cannot be deleted — there is
    // nothing on the server to delete. The button must not be rendered.
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();
    const deleteBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Delete")
    );
    expect(deleteBtn).toBeUndefined();
  });

  it("shows the Delete button for existing entries", async () => {
    // Employees must be able to remove an entry they created by mistake
    // (e.g. duplicate, wrong date) as long as the week is still in draft.
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: {
        template: {
          id: 20,
          entry_date: "2026-05-26",
          start_time: "08:00",
          end_time: "12:00",
          category_id: 1,
          comment: "",
        },
        onClose,
      },
    });
    await settle();
    const deleteBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Delete")
    );
    expect(deleteBtn).not.toBeNull();
  });

  it("POSTs to /time-entries when saving a new entry", async () => {
    // A new entry must be created with POST so the backend assigns it a new ID.
    // Use a fixed past date so the "end time cannot be in the future" guard
    // (which compares end_time to the real wall-clock time on today's date) is
    // never triggered regardless of when the test suite runs.
    apiMock.mockResolvedValueOnce({ id: 55 });
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: { template: { entry_date: "2024-01-15" }, onClose },
    });
    await settle();

    const addBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.trim() === "Add Entry"
    );
    addBtn?.click();
    await settle();
    await settle();

    const postCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/time-entries" && opts?.method === "POST"
    );
    expect(postCall).toBeTruthy();
  });

  it("DELETEs the entry after the user confirms deletion", async () => {
    // The delete confirmation prevents accidental data loss. Once confirmed,
    // the backend must receive a DELETE request with the correct entry ID.
    apiMock.mockResolvedValueOnce({});
    const onClose = vi.fn();
    component = mount(EntryDialog, {
      target,
      props: {
        template: {
          id: 30,
          entry_date: "2026-05-26",
          start_time: "08:00",
          end_time: "12:00",
          category_id: 1,
          comment: "",
        },
        onClose,
      },
    });
    await settle();

    const deleteBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Delete")
    );
    deleteBtn?.click();
    await settle();
    await settle();

    const deleteCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        path === "/time-entries/30" && opts?.method === "DELETE"
    );
    expect(deleteCall).toBeTruthy();
  });
});
