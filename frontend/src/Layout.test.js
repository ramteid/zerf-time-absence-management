import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Layout from "./Layout.svelte";
import { currentUser } from "./stores.js";
import { setLanguage } from "./i18n.js";

vi.mock("svelte", async () => {
  return await import("../node_modules/svelte/src/index-client.js");
});

vi.mock("./api.js", () => ({
  api: vi.fn(),
}));

vi.mock("./notificationService.js", () => ({
  clearNotifications: vi.fn(async () => {}),
  markAllNotificationsRead: vi.fn(async () => {}),
  markNotificationRead: vi.fn(async () => {}),
  refreshNotifications: vi.fn(async () => {}),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function appendContentArea(target) {
  const contentArea = document.createElement("div");
  contentArea.className = "content-area";
  const handle = document.createElement("div");
  handle.textContent = "Scrollable content";
  contentArea.appendChild(handle);
  target.querySelector(".main-content").appendChild(contentArea);
  return { contentArea, handle };
}

function dispatchTouch(target, type, clientY) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "touches", {
    configurable: true,
    value: clientY === null ? [] : [{ clientY, target }],
  });
  target.dispatchEvent(event);
}

describe("Layout pull to refresh", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    currentUser.set({
      id: 1,
      first_name: "Admin",
      last_name: "User",
      role: "admin",
      nav: [],
    });
    setLanguage("en");
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("does not arm pull to refresh when the page content is already scrolled", async () => {
    component = mount(Layout, { target });
    await settle();
    const { contentArea, handle } = appendContentArea(target);
    contentArea.scrollTop = 120;

    dispatchTouch(handle, "touchstart", 120);
    dispatchTouch(handle, "touchmove", 220);
    await settle();

    expect(target.querySelector(".pull-to-refresh")).toBeNull();
  });

  it("arms pull to refresh when the page content starts at the top", async () => {
    component = mount(Layout, { target });
    await settle();
    const { contentArea, handle } = appendContentArea(target);
    contentArea.scrollTop = 0;

    dispatchTouch(handle, "touchstart", 120);
    dispatchTouch(handle, "touchmove", 220);
    await settle();

    expect(target.querySelector(".pull-to-refresh")).not.toBeNull();
  });
});