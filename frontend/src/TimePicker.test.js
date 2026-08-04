import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import TimePicker from "./TimePicker.svelte";
import { settings } from "./stores.js";

vi.mock("svelte", async () => {
  return await import("../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("TimePicker", () => {
  let target;
  let component;

  beforeEach(() => {
    component = null;
    target = document.createElement("div");
    document.body.appendChild(target);
    settings.set({ time_format: "24h", timezone: "UTC" });
  });

  afterEach(() => {
    if (component) unmount(component);
    target.remove();
  });

  it("associates the field id and accessible label with the visible control", async () => {
    component = mount(TimePicker, {
      target,
      props: { id: "start-time", label: "Start", value: "08:00" },
    });
    await settle();

    const button = target.querySelector(".tp-display");
    expect(button.id).toBe("start-time");
    expect(button.getAttribute("aria-label")).toBe("Start");
    expect(button.getAttribute("aria-controls")).toBe("start-time-picker");
    expect(target.querySelector('input[type="hidden"]')).toBeNull();
  });

  it("allows Tab to leave the time-picker panel", async () => {
    component = mount(TimePicker, {
      target,
      props: { id: "start-time", label: "Start", value: "08:00" },
    });
    await settle();

    target.querySelector(".tp-display").click();
    await settle();
    const panel = target.querySelector(".tp-drum");
    const tabEvent = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    panel.dispatchEvent(tabEvent);

    expect(tabEvent.defaultPrevented).toBe(false);
    expect(target.querySelector(".tp-drum")).not.toBeNull();
  });
});
