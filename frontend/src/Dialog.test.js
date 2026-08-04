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

  beforeEach(() => {
    component = null;
    target = document.createElement("div");
    backgroundButton = document.createElement("button");
    document.body.append(backgroundButton, target);
    originalShowModal = HTMLDialogElement.prototype.showModal;
    originalClose = HTMLDialogElement.prototype.close;
  });

  afterEach(() => {
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
});
