import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceCategoryDialog from "./AbsenceCategoryDialog.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  requests: [],
}));

function requestFor(path, method) {
  return mockState.requests.find(
    (r) => r.path === path && (!method || (r.options?.method || "GET") === method),
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
    if (path === "/users") {
      return [
        { id: 1, first_name: "Ada", last_name: "Lovelace" },
        { id: 2, first_name: "Grace", last_name: "Hopper" },
      ];
    }
    if (path.endsWith("/users")) {
      return [1];
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
        template: { id: 9, name: "Vacation", color: "#6D4C41", cost_type: "vacation" },
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
        template: { id: 9, name: "Vacation", color: "#6D4C41", cost_type: "vacation" },
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

  it("does not fetch the access table when creating a new category", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceCategoryDialog, {
      target,
      props: { template: {}, onClose },
    });

    await settle();

    expect(requestFor("/users")).toBeUndefined();
    expect(target.querySelector("table")).toBeNull();
  });
});
