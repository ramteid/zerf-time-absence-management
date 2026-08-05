import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceCategoryDialog from "./AbsenceCategoryDialog.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  requests: [],
  usersGate: Promise.resolve(),
  failUsersPutOnce: false,
}));

function requestFor(path, method) {
  return mockState.requests.find(
    (r) =>
      r.path === path && (!method || (r.options?.method || "GET") === method),
  );
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
  await Promise.resolve();
}

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (path, options) => {
    mockState.requests.push({ path, options });
    if (path === "/absence-categories/42/users" && options?.method === "PUT") {
      if (mockState.failUsersPutOnce) {
        mockState.failUsersPutOnce = false;
        throw new Error("Network error");
      }
      return { ok: true };
    }
    if (path === "/users") {
      await mockState.usersGate;
      return [
        { id: 1, first_name: "Ada", last_name: "Lovelace" },
        { id: 2, first_name: "Grace", last_name: "Hopper" },
      ];
    }
    if (path.endsWith("/users")) {
      return [1];
    }
    if (path === "/absence-categories" && options?.method === "POST") {
      return { id: 42, ...options.body };
    }
    return { ok: true };
  }),
}));

describe("AbsenceCategoryDialog", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function showModal() {
      this.setAttribute("open", "open");
    };
    setLanguage("en");
    mockState.requests = [];
    mockState.usersGate = Promise.resolve();
    mockState.failUsersPutOnce = false;
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    HTMLDialogElement.prototype.showModal = originalShowModal;
    target.remove();
  });

  it("loads and renders the per-employee access table when editing", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: {
        template: {
          id: 9,
          name: "Vacation",
          color: "#6D4C41",
          cost_type: "vacation",
          leave_account_default_days: 30,
          leave_account_carryover_expiry: "03-31",
        },
        onClose,
      },
    });

    await settle();

    expect(requestFor("/users")).toBeDefined();
    expect(requestFor("/absence-categories/9/users")).toBeDefined();
    const rows = target.querySelectorAll("table tbody tr");
    expect(rows.length).toBe(2);
  });

  it("saves the selected user ids to the absence category users endpoint", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: {
        template: {
          id: 9,
          name: "Vacation",
          color: "#6D4C41",
          cost_type: "vacation",
          leave_account_default_days: 30,
          leave_account_carryover_expiry: "03-31",
        },
        onClose,
      },
    });

    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const usersRequest = requestFor("/absence-categories/9/users", "PUT");
    expect(usersRequest).toBeDefined();
    expect(usersRequest.options.body).toEqual({ user_ids: [1] });
  });

  it("loads and renders the per-employee access table when creating a new category, pre-selecting everyone", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    expect(requestFor("/users")).toBeDefined();
    const rows = target.querySelectorAll("table tbody tr");
    expect(rows.length).toBe(2);
    const checkboxes = target.querySelectorAll(
      "table tbody input[type=checkbox]",
    );
    expect([...checkboxes].every((cb) => cb.checked)).toBe(true);
  });

  it("saves the selected user ids to the newly created absence category's users endpoint", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    const checkboxes = target.querySelectorAll(
      "table tbody input[type=checkbox]",
    );
    checkboxes[0].click();
    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const createRequest = requestFor("/absence-categories", "POST");
    expect(createRequest).toBeDefined();

    const usersRequest = requestFor("/absence-categories/42/users", "PUT");
    expect(usersRequest).toBeDefined();
    expect(usersRequest.options.body).toEqual({ user_ids: [1] });
  });

  it("does not call the users endpoint when creating a category with everyone left selected", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    expect(requestFor("/absence-categories", "POST")).toBeDefined();
    expect(requestFor("/absence-categories/42/users", "PUT")).toBeUndefined();
  });

  it("does not wipe access when Save is clicked before the employee list finishes loading", async () => {
    let releaseUsers;
    mockState.usersGate = new Promise((resolve) => {
      releaseUsers = resolve;
    });
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    // Click Save immediately, before onMount's /users fetch resolves.
    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    releaseUsers();
    await settle();

    expect(requestFor("/absence-categories", "POST")).toBeDefined();
    expect(requestFor("/absence-categories/42/users", "PUT")).toBeUndefined();
  });

  it("does not create a duplicate category when retrying save after the users endpoint fails", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    const checkboxes = target.querySelectorAll(
      "table tbody input[type=checkbox]",
    );
    checkboxes[0].click();
    await settle();

    mockState.failUsersPutOnce = true;
    const saveButton = target.querySelector("button.zf-btn.zf-btn-primary");

    saveButton.click();
    await settle();

    expect(onClose).not.toHaveBeenCalled();
    expect(
      mockState.requests.filter(
        (r) => r.path === "/absence-categories" && r.options?.method === "POST",
      ),
    ).toHaveLength(1);

    saveButton.click();
    await settle();

    expect(
      mockState.requests.filter(
        (r) => r.path === "/absence-categories" && r.options?.method === "POST",
      ),
    ).toHaveLength(1);
    expect(
      mockState.requests.filter(
        (r) =>
          r.path === "/absence-categories/42/users" &&
          r.options?.method === "PUT",
      ),
    ).toHaveLength(2);
  });

  it("persists field edits made between a failed users-PUT and the retry", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    mockState.failUsersPutOnce = true;
    const saveButton = target.querySelector("button.zf-btn.zf-btn-primary");
    const checkboxes = target.querySelectorAll(
      "table tbody input[type=checkbox]",
    );
    checkboxes[0].click();
    await settle();

    saveButton.click();
    await settle();

    // Edit the name after the first (failed) attempt already created the
    // category server-side.
    const nameInput = target.querySelector("#abscat-name");
    nameInput.value = "Renamed after failure";
    nameInput.dispatchEvent(new Event("input"));
    await settle();

    saveButton.click();
    await settle();

    const fieldUpdate = requestFor("/absence-categories/42");
    expect(fieldUpdate).toBeDefined();
    expect(fieldUpdate.options.body).toMatchObject({
      name: "Renamed after failure",
    });
  });

  it("only shows the Unpaid checkbox for cost_type 'none' and includes it in the save payload", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: {
        template: { id: 9, name: "Special leave", color: "#0ea5e9" },
        onClose,
      },
    });
    await settle();

    function unpaidCheckbox() {
      return [...target.querySelectorAll("label")]
        .find((label) => label.textContent.includes("Unpaid"))
        ?.querySelector('input[type="checkbox"]');
    }

    // Defaults to cost_type "none", so the checkbox is visible.
    expect(unpaidCheckbox()).toBeTruthy();
    unpaidCheckbox().click();
    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const updateRequest = requestFor("/absence-categories/9", "PUT");
    expect(updateRequest).toBeDefined();
    expect(updateRequest.options.body).toMatchObject({ unpaid: true });
  });

  it("resets unpaid to false when cost_type changes from 'none' to 'flextime'", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: {
        template: { id: 9, name: "Special leave", color: "#0ea5e9" },
        onClose,
      },
    });
    await settle();

    const unpaidCheckbox = [...target.querySelectorAll("label")]
      .find((label) => label.textContent.includes("Unpaid"))
      .querySelector('input[type="checkbox"]');
    unpaidCheckbox.click();
    await settle();

    const flextimeRadio = target.querySelector(
      'input[type="radio"][value="flextime"]',
    );
    flextimeRadio.click();
    await settle();

    // The checkbox (and its row) only render for cost_type "none".
    expect(
      [...target.querySelectorAll("label")].find((label) =>
        label.textContent.includes("Unpaid"),
      ),
    ).toBeUndefined();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const updateRequest = requestFor("/absence-categories/9", "PUT");
    expect(updateRequest.options.body).toMatchObject({ unpaid: false });
  });

  it("requires valid account fields for a new leave-account category", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();

    target.querySelector('input[type="radio"][value="vacation"]').click();
    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    expect(target.textContent).toContain("valid carryover expiry date");
    expect(requestFor("/absence-categories", "POST")).toBeUndefined();
  });

  it("sends leave-account fields without exposing an internal start year", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });
    await settle();

    target.querySelector('input[type="radio"][value="vacation"]').click();
    await settle();
    const defaultDays = target.querySelector(
      "#abscat-leave-account-default-days",
    );
    const expiry = target.querySelector(
      "#abscat-leave-account-carryover-expiry",
    );
    defaultDays.value = "5";
    defaultDays.dispatchEvent(new Event("input"));
    expiry.value = "01-31";
    expiry.dispatchEvent(new Event("input"));
    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const createRequest = requestFor("/absence-categories", "POST");
    expect(createRequest.options.body).toMatchObject({
      cost_type: "vacation",
      leave_account_default_days: 5,
      leave_account_carryover_expiry: "01-31",
    });
    expect(createRequest.options.body).not.toHaveProperty(
      "leave_account_start_year",
    );
    expect(target.textContent).not.toContain("start year");
  });

  it("does not let an existing non-account category become a leave account", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: {
        template: { id: 9, name: "Special leave", color: "#0ea5e9" },
        onClose,
      },
    });
    await settle();

    expect(
      target.querySelector('input[type="radio"][value="vacation"]').disabled,
    ).toBe(true);
  });

  it("keeps an existing leave-account category on its account type", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: {
        template: {
          id: 9,
          name: "Vacation",
          color: "#6D4C41",
          cost_type: "vacation",
          leave_account_default_days: 30,
          leave_account_carryover_expiry: "03-31",
        },
        onClose,
      },
    });
    await settle();

    expect(target.querySelector('input[value="none"]').disabled).toBe(true);
    expect(target.querySelector('input[value="flextime"]').disabled).toBe(true);
    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    expect(
      requestFor("/absence-categories/9", "PUT").options.body,
    ).toMatchObject({
      cost_type: "vacation",
    });
  });

  it("does not create a duplicate category when Save is double-clicked before the first request resolves", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    const saveButton = target.querySelector("button.zf-btn.zf-btn-primary");
    saveButton.click();
    saveButton.click();
    await settle();

    expect(
      mockState.requests.filter(
        (r) => r.path === "/absence-categories" && r.options?.method === "POST",
      ),
    ).toHaveLength(1);
  });
});
