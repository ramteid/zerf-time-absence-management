import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Dialog from "./Dialog.svelte";

vi.mock("svelte", async () => {
  return await import("../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("Dialog", () => {
  let target;
  let backgroundButton;
  let component;
  let originalShowModal;
  let originalClose;
  let additionalComponents;

  beforeEach(() => {
    component = null;
    additionalComponents = [];
    target = document.createElement("div");
    backgroundButton = document.createElement("button");
    document.body.append(backgroundButton, target);
    originalShowModal = HTMLDialogElement.prototype.showModal;
    originalClose = HTMLDialogElement.prototype.close;
  });

  afterEach(() => {
    for (const additionalComponent of additionalComponents.reverse()) {
      unmount(additionalComponent);
    }
    if (component) unmount(component);
    target.remove();
    backgroundButton.remove();
    if (originalShowModal) {
      HTMLDialogElement.prototype.showModal = originalShowModal;
    } else {
      delete HTMLDialogElement.prototype.showModal;
    }
    if (originalClose) {
      HTMLDialogElement.prototype.close = originalClose;
    } else {
      delete HTMLDialogElement.prototype.close;
    }
  });

  it("provides a closable modal fallback without the native dialog API", async () => {
    delete HTMLDialogElement.prototype.showModal;
    delete HTMLDialogElement.prototype.close;
    const onClose = vi.fn();
    component = mount(Dialog, {
      target,
      props: { title: "Fallback dialog", onClose },
    });

    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog.open).toBe(true);
    expect(dialog.classList.contains("dialog-fallback")).toBe(true);
    expect(dialog.getAttribute("role")).toBe("dialog");
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(target.querySelector(".dialog-backdrop")).not.toBeNull();
    expect(backgroundButton.inert).toBe(true);

    target.querySelector('button[aria-label="Close"]').click();
    await settle();

    expect(dialog.open).toBe(false);
    expect(target.querySelector(".dialog-backdrop")).toBeNull();
    expect(backgroundButton.inert).not.toBe(true);
    expect(backgroundButton.hasAttribute("aria-hidden")).toBe(false);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes only the topmost nested fallback dialog", async () => {
    delete HTMLDialogElement.prototype.showModal;
    delete HTMLDialogElement.prototype.close;
    const firstOnClose = vi.fn();
    const secondOnClose = vi.fn();
    backgroundButton.focus();
    component = mount(Dialog, {
      target,
      props: { title: "First dialog", onClose: firstOnClose },
    });
    await settle();
    const firstDialog = target.querySelector("dialog");
    const firstCloseButton = firstDialog.querySelector("button");
    firstCloseButton.focus();
    additionalComponents.push(
      mount(Dialog, {
        target,
        props: { title: "Second dialog", onClose: secondOnClose },
      }),
    );
    await settle();

    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await settle();

    expect(firstDialog.open).toBe(true);
    expect(target.querySelectorAll("dialog[open]")).toHaveLength(1);
    expect(firstOnClose).not.toHaveBeenCalled();
    expect(secondOnClose).toHaveBeenCalledTimes(1);
    expect(backgroundButton.inert).toBe(true);
    expect(document.activeElement).toBe(firstCloseButton);

    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await settle();

    expect(firstDialog.open).toBe(false);
    expect(firstOnClose).toHaveBeenCalledTimes(1);
    expect(backgroundButton.inert).not.toBe(true);
    expect(backgroundButton.hasAttribute("aria-hidden")).toBe(false);
    expect(document.activeElement).toBe(backgroundButton);
  });

  it("keeps the background inert when nested fallbacks close out of order", async () => {
    delete HTMLDialogElement.prototype.showModal;
    delete HTMLDialogElement.prototype.close;
    backgroundButton.focus();
    component = mount(Dialog, {
      target,
      props: { title: "First dialog", onClose: vi.fn() },
    });
    await settle();
    const secondComponent = mount(Dialog, {
      target,
      props: { title: "Second dialog", onClose: vi.fn() },
    });
    additionalComponents.push(secondComponent);
    await settle();

    component.close();
    await settle();

    expect(target.querySelectorAll("dialog[open]")).toHaveLength(1);
    expect(backgroundButton.inert).toBe(true);
    expect(backgroundButton.getAttribute("aria-hidden")).toBe("true");

    secondComponent.close();
    await settle();

    expect(target.querySelectorAll("dialog[open]")).toHaveLength(0);
    expect(backgroundButton.inert).not.toBe(true);
    expect(backgroundButton.hasAttribute("aria-hidden")).toBe(false);
  });

  it("falls back when a partial dialog close method does nothing", async () => {
    delete HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.close = vi.fn();
    const onClose = vi.fn();
    component = mount(Dialog, {
      target,
      props: { title: "Partial dialog", onClose },
    });
    await settle();

    target.querySelector('button[aria-label="Close"]').click();
    await settle();

    expect(target.querySelector("dialog").open).toBe(false);
    expect(target.querySelector(".dialog-backdrop")).toBeNull();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("prevents user-initiated closing while close is disabled", async () => {
    const onClose = vi.fn();
    component = mount(Dialog, {
      target,
      props: { title: "Busy dialog", onClose, closeDisabled: true },
    });
    await settle();
    const dialog = target.querySelector("dialog");
    const cancelEvent = new Event("cancel", { cancelable: true });

    dialog.dispatchEvent(cancelEvent);
    target.querySelector('button[aria-label="Close"]').click();
    await settle();

    expect(cancelEvent.defaultPrevented).toBe(true);
    expect(dialog.open).toBe(true);
    expect(target.querySelector('button[aria-label="Close"]').disabled).toBe(
      true,
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
