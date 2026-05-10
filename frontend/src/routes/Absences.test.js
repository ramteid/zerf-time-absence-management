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

async function waitForElement(target, selector, timeout = 10000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const element = target.querySelector(selector);
    if (element) {
      return element;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`Element not found within ${timeout}ms: ${selector}`);
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

  it("opens the request dialog after the year slider changes", async () => {
    component = mount(Absences, { target });
    await settle();

    // Navigate to previous year via the prev button
    const prevBtn = await waitForElement(
      target,
      '[aria-label="Previous year"]',
      10000,
    );
    prevBtn.click();
    await settle();

    const requestButton = await waitForElement(target, ".kz-btn-primary", 10000);
    requestButton.click();
    await settle();

    const dialog = await waitForElement(target, "dialog", 10000);
    expect(dialog).not.toBeNull();
    expect(dialog.hasAttribute("open")).toBe(true);
  }, 20000);

  it("falls back when modal opening is rejected after the year slider changes", async () => {
    HTMLDialogElement.prototype.showModal = function showModal() {
      throw new DOMException("Modal opening rejected.", "InvalidStateError");
    };

    component = mount(Absences, { target });
    await settle();

    // Navigate to previous year via the prev button
    const prevBtn = await waitForElement(
      target,
      '[aria-label="Previous year"]',
      10000,
    );
    prevBtn.click();
    await settle();

    const requestButton = await waitForElement(target, ".kz-btn-primary", 10000);
    requestButton.click();
    await settle();

    const dialog = await waitForElement(target, "dialog", 10000);
    expect(dialog).not.toBeNull();
    expect(dialog.hasAttribute("open")).toBe(true);
  }, 20000);

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

  it("shows zero days for weekend-only training absences", async () => {
    mockState.absences = [
      {
        id: 8,
        user_id: 1,
        kind: "training",
        start_date: "2026-08-01",
        end_date: "2026-08-02",
        comment: "",
        status: "approved",
        reviewed_by: null,
        reviewed_at: null,
        rejection_reason: null,
        created_at: "2026-07-01",
      },
    ];

    component = mount(Absences, { target });
    await settle();

    const entry = target.querySelector(".absence-entry");
    expect(
      entry.querySelector(".absence-entry-days .absence-entry-value")
        .textContent,
    ).toBe("0");

    entry.click();
    await settle();

    const detailValues = [
      ...target.querySelectorAll("dialog .field-row .tab-num"),
    ].map((element) => element.textContent.trim());
    expect(detailValues[2]).toBe("0");
    expect(target.querySelector("dialog .kz-btn-danger").textContent).toContain(
      "Request cancellation",
    );
  });

  it("uses a distinct German label for cancelling an absence", async () => {
    setLanguage("de");
    mockState.absences = [
      {
        id: 9,
        user_id: 1,
        kind: "vacation",
        start_date: "2026-05-04",
        end_date: "2026-05-05",
        comment: "",
        status: "approved",
        reviewed_by: null,
        reviewed_at: null,
        rejection_reason: null,
        created_at: "2026-04-01",
      },
    ];

    component = mount(Absences, { target });
    await settle();

    target.querySelector(".absence-entry").click();
    await settle();

    expect(target.querySelector("dialog .kz-btn-danger").textContent).toContain(
      "Stornierung beantragen",
    );
  });
});
