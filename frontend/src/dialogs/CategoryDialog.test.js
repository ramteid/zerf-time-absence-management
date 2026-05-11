import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import CategoryDialog from "./CategoryDialog.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  lastRequest: null,
}));

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
}

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (path, options) => {
    mockState.lastRequest = { path, options };
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
    mockState.lastRequest = null;
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

    target.querySelector("button.kz-btn.kz-btn-primary").click();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockState.lastRequest).not.toBeNull();
    expect(mockState.lastRequest.path).toBe("/categories/17");
    expect(mockState.lastRequest.options.body).toMatchObject({
      name: "Flextime Reduction",
      color: "#6D4C41",
      sort_order: 7,
      description: null,
      counts_as_work: false,
    });
  });
});