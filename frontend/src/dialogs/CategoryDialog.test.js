import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import CategoryDialog from "./CategoryDialog.svelte";
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
    if (path === "/categories/42/users" && options?.method === "PUT") {
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
    if (path === "/categories" && options?.method === "POST") {
      return { id: 42, ...options.body };
    }
    return { ok: true };
  }),
}));

describe("CategoryDialog", () => {
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

  it("sends counts_as_work when saving an edited category", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
      target,
      props: {
        template: {
          id: 17,
          name: "Flextime Reduction",
          counts_as_work: false,
          color: "#6D4C41",
          sort_order: 7,
          description: "",
        },
        onClose,
      },
    });

    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog).not.toBeNull();
    expect(dialog.hasAttribute("open")).toBe(true);

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const updateRequest = requestFor("/categories/17");
    expect(updateRequest).toBeDefined();
    expect(updateRequest.options.body).toMatchObject({
      name: "Flextime Reduction",
      color: "#6D4C41",
      sort_order: 7,
      description: null,
      counts_as_work: false,
    });
  });

  it("loads and renders the per-employee access table when editing", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
      target,
      props: {
        template: { id: 17, name: "Flextime Reduction", color: "#6D4C41" },
        onClose,
      },
    });

    await settle();

    expect(requestFor("/users")).toBeDefined();
    expect(requestFor("/categories/17/users")).toBeDefined();
    const rows = target.querySelectorAll("table tbody tr");
    expect(rows.length).toBe(2);
  });

  it("saves the selected user ids to the category users endpoint", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
      target,
      props: {
        template: { id: 17, name: "Flextime Reduction", color: "#6D4C41" },
        onClose,
      },
    });

    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    const usersRequest = requestFor("/categories/17/users", "PUT");
    expect(usersRequest).toBeDefined();
    expect(usersRequest.options.body).toEqual({ user_ids: [1] });
  });

  it("loads and renders the per-employee access table when creating a new category, pre-selecting everyone", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
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

  it("saves the selected user ids to the newly created category's users endpoint", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
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

    const createRequest = requestFor("/categories", "POST");
    expect(createRequest).toBeDefined();

    const usersRequest = requestFor("/categories/42/users", "PUT");
    expect(usersRequest).toBeDefined();
    expect(usersRequest.options.body).toEqual({ user_ids: [1] });
  });

  it("does not call the users endpoint when creating a category with everyone left selected", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    expect(requestFor("/categories", "POST")).toBeDefined();
    expect(requestFor("/categories/42/users", "PUT")).toBeUndefined();
  });

  it("does not wipe access when Save is clicked before the employee list finishes loading", async () => {
    let releaseUsers;
    mockState.usersGate = new Promise((resolve) => {
      releaseUsers = resolve;
    });
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    // Click Save immediately, before onMount's /users fetch resolves.
    target.querySelector("button.zf-btn.zf-btn-primary").click();
    await settle();

    releaseUsers();
    await settle();

    expect(requestFor("/categories", "POST")).toBeDefined();
    expect(requestFor("/categories/42/users", "PUT")).toBeUndefined();
  });

  it("does not create a duplicate category when retrying save after the users endpoint fails", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
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
        (r) => r.path === "/categories" && r.options?.method === "POST",
      ),
    ).toHaveLength(1);

    saveButton.click();
    await settle();

    expect(
      mockState.requests.filter(
        (r) => r.path === "/categories" && r.options?.method === "POST",
      ),
    ).toHaveLength(1);
    expect(
      mockState.requests.filter(
        (r) => r.path === "/categories/42/users" && r.options?.method === "PUT",
      ),
    ).toHaveLength(2);
  });

  it("persists field edits made between a failed users-PUT and the retry", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
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
    const nameInput = target.querySelector("#cat-name");
    nameInput.value = "Renamed after failure";
    nameInput.dispatchEvent(new Event("input"));
    await settle();

    saveButton.click();
    await settle();

    const fieldUpdate = requestFor("/categories/42");
    expect(fieldUpdate).toBeDefined();
    expect(fieldUpdate.options.body).toMatchObject({
      name: "Renamed after failure",
    });
  });

  it("does not create a duplicate category when Save is double-clicked before the first request resolves", async () => {
    const onClose = vi.fn();
    component = mount(CategoryDialog, {
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
        (r) => r.path === "/categories" && r.options?.method === "POST",
      ),
    ).toHaveLength(1);
  });
});
