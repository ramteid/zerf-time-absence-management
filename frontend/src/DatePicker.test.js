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

function currentMonthLabel(calendar) {
  const dropdown = calendar.querySelector(".flatpickr-monthDropdown-months");
  if (dropdown) return dropdown.options[dropdown.selectedIndex]?.textContent || "";
  return calendar.querySelector(".cur-month")?.textContent || "";
}

describe("DatePicker", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    document.querySelectorAll(".flatpickr-calendar").forEach((el) => el.remove());
    target.remove();
  });

  it("allows repeated month navigation clicks in the popup", async () => {
    component = mount(DatePicker, {
      target,
      props: {
        value: "2026-05-04",
      },
    });
    await settle();

    target.querySelector(".date-picker-button").click();
    await settle();

    const calendar = document.querySelector(".flatpickr-calendar.open");
    expect(calendar).not.toBeNull();
    const nextButton = calendar.querySelector(".flatpickr-next-month");
    expect(nextButton).not.toBeNull();

    nextButton.click();
    await settle();
    expect(currentMonthLabel(calendar)).toContain("June");

    nextButton.click();
    await settle();
    expect(currentMonthLabel(calendar)).toContain("July");
  });

  it("allows repeated year navigation clicks in month mode", async () => {
    component = mount(DatePicker, {
      target,
      props: {
        mode: "month",
        value: "2026-05",
      },
    });
    await settle();

    target.querySelector(".date-picker-button").click();
    await settle();

    const calendar = document.querySelector(".flatpickr-calendar.open");
    expect(calendar).not.toBeNull();
    const nextButton = calendar.querySelector(".flatpickr-next-month");
    const yearInput = calendar.querySelector("input.cur-year");
    expect(nextButton).not.toBeNull();
    expect(yearInput).not.toBeNull();

    nextButton.click();
    await settle();
    expect(yearInput.value).toBe("2027");

    nextButton.click();
    await settle();
    expect(yearInput.value).toBe("2028");
  });
});
