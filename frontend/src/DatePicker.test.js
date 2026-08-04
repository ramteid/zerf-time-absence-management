import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import DatePicker from "./DatePicker.svelte";
import { setLanguage } from "./i18n.js";

vi.mock("svelte", async () => {
  return await import("../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("DatePicker", () => {
  let dialog;
  let target;
  let component;
  let originalUserAgent;

  beforeEach(() => {
    component = null;
    dialog = document.createElement("dialog");
    dialog.setAttribute("open", "");
    target = document.createElement("div");
    dialog.appendChild(target);
    document.body.appendChild(dialog);
    originalUserAgent = Object.getOwnPropertyDescriptor(
      Navigator.prototype,
      "userAgent",
    );
    setLanguage("en");
  });

  afterEach(() => {
    if (component) unmount(component);
    dialog.remove();
    if (originalUserAgent) {
      Object.defineProperty(
        Navigator.prototype,
        "userAgent",
        originalUserAgent,
      );
    } else {
      delete Navigator.prototype.userAgent;
    }
  });

  it("closes only the calendar when Escape is pressed inside a dialog", async () => {
    component = mount(DatePicker, {
      target,
      props: {
        id: "entry-date",
        value: "2024-01-15",
        container: dialog,
      },
    });
    await settle();

    target.querySelector(".date-picker-button").click();
    await settle();
    const escapeEvent = new KeyboardEvent("keydown", {
      key: "Escape",
      keyCode: 27,
      bubbles: true,
      cancelable: true,
    });
    target.querySelector(".date-picker-button").dispatchEvent(escapeEvent);
    await settle();

    expect(escapeEvent.defaultPrevented).toBe(true);
    expect(dialog.querySelector(".flatpickr-calendar.open")).toBeNull();
    expect(dialog.open).toBe(true);
    expect(document.activeElement.id).toBe("entry-date");
  });

  it("uses a labelled and styled native date input on mobile browsers", async () => {
    Object.defineProperty(Navigator.prototype, "userAgent", {
      configurable: true,
      value:
        "Mozilla/5.0 (iPhone; CPU iPhone OS 15_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148",
    });
    component = mount(DatePicker, {
      target,
      props: {
        id: "entry-date",
        value: "2024-01-15",
        mobileNative: true,
        container: dialog,
      },
    });
    await settle();

    const mobileInput = dialog.querySelector("#entry-date");
    expect(mobileInput.type).toBe("date");
    expect(mobileInput.classList.contains("flatpickr-mobile")).toBe(true);
    expect(mobileInput.classList.contains("zf-input")).toBe(true);
    expect(mobileInput.tabIndex).toBe(0);
  });
});
