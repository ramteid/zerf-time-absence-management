import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Absences from "./Absences.svelte";
import { currentUser } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  absences: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (path) => {
    if (path.startsWith("/absences")) return mockState.absences;
    if (path.startsWith("/leave-balance")) {
      return {
        annual_entitlement: 30,
        already_taken: 0,
        approved_upcoming: 0,
        requested: 0,
        available: 30,
      };
    }
    if (path.startsWith("/holidays")) return [];
    throw new Error(`Unhandled API path: ${path}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("Absences", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    currentUser.set({ id: 1 });
    setLanguage("en");
    mockState.absences = [];
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function showModal() {
      this.setAttribute("open", "");
    };
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
  });

  it("opens the request dialog after the year select changes", async () => {
    component = mount(Absences, { target });
    await settle();

    const select = target.querySelector(".absence-year-select");
    const currentYear = String(new Date().getFullYear());
    select.value = currentYear;
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();

    target.querySelector(".kz-btn-primary").click();
    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog).not.toBeNull();
    expect(dialog.hasAttribute("open")).toBe(true);
  });

  it("falls back when modal opening is rejected after the year select changes", async () => {
    HTMLDialogElement.prototype.showModal = function showModal() {
      throw new DOMException("Modal opening rejected.", "InvalidStateError");
    };

    component = mount(Absences, { target });
    await settle();

    const select = target.querySelector(".absence-year-select");
    const currentYear = String(new Date().getFullYear());
    select.value = currentYear;
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();

    target.querySelector(".kz-btn-primary").click();
    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog).not.toBeNull();
    expect(dialog.hasAttribute("open")).toBe(true);
  });

  it("renders absence history fields and comment", async () => {
    const currentYear = new Date().getFullYear();
    mockState.absences = [
      {
        id: 7,
        user_id: 1,
        kind: "vacation",
        start_date: `${currentYear}-05-04`,
        end_date: `${currentYear}-05-06`,
        comment: "Family trip",
        status: "requested",
        reviewed_by: null,
        reviewed_at: null,
        rejection_reason: null,
        created_at: `${currentYear}-04-01`,
      },
    ];

    component = mount(Absences, { target });
    await settle();

    const entry = target.querySelector(".absence-entry");
    expect(entry).not.toBeNull();
    expect(entry.querySelector(".absence-entry-summary")).not.toBeNull();
    expect(entry.querySelector(".absence-entry-type").textContent).toContain(
      "Vacation",
    );
    expect(entry.querySelector(".absence-entry-days").textContent).toContain(
      "Days",
    );
    expect(entry.querySelector(".absence-entry-from").textContent).toContain(
      String(currentYear),
    );
    expect(entry.querySelector(".absence-entry-to").textContent).toContain(
      String(currentYear),
    );
    expect(entry.querySelector(".absence-entry-comment").textContent).toContain(
      "Family trip",
    );
    expect(
      entry.querySelector(".absence-entry-status .kz-chip-requested")
        .textContent,
    ).toContain("Requested");
  });
});
