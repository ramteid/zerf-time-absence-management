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

    expect(target.querySelector(".pull-to-refresh.ptr-open")).toBeNull();
  });

  it("arms pull to refresh when the page content starts at the top", async () => {
    component = mount(Layout, { target });
    await settle();
    const { contentArea, handle } = appendContentArea(target);
    contentArea.scrollTop = 0;

    dispatchTouch(handle, "touchstart", 120);
    dispatchTouch(handle, "touchmove", 220);
    await settle();

    expect(target.querySelector(".pull-to-refresh.ptr-open")).not.toBeNull();
  });
});

describe("Layout navigation for pure-admin", () => {
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
    target.remove();
  });

  it("shows Dashboard and Reports in the sidebar for pure-admin", async () => {
    currentUser.set({
      id: 1,
      first_name: "Arnold",
      last_name: "Admin",
      role: "admin",
      tracks_time: false,
      nav: [
        { href: "/dashboard", key: "Dashboard", icon: "🔔" },
        { href: "/reports", key: "Reports", icon: "📊" },
        { href: "/account", key: "Account", icon: "👤" },
        { href: "/team-settings", key: "TeamSettings", icon: "🛡" },
        { href: "/admin/settings", key: "Admin", icon: "⚙" },
      ],
      permissions: { can_approve: true, can_view_dashboard: true },
    });
    component = mount(Layout, { target });
    await settle();

    const navLinks = target.querySelectorAll(".sidebar-nav a.zf-nav-item");
    const navTexts = Array.from(navLinks).map((el) => el.textContent.trim());
    expect(navTexts).toContain("Dashboard");
    expect(navTexts).toContain("Reports");
  });

  it("does not show Time, Absences, or Calendar in the sidebar for pure-admin", async () => {
    currentUser.set({
      id: 1,
      first_name: "Arnold",
      last_name: "Admin",
      role: "admin",
      tracks_time: false,
      nav: [
        { href: "/dashboard", key: "Dashboard", icon: "🔔" },
        { href: "/reports", key: "Reports", icon: "📊" },
        { href: "/account", key: "Account", icon: "👤" },
        { href: "/team-settings", key: "TeamSettings", icon: "🛡" },
        { href: "/admin/settings", key: "Admin", icon: "⚙" },
      ],
      permissions: { can_approve: true, can_view_dashboard: true },
    });
    component = mount(Layout, { target });
    await settle();

    const navLinks = target.querySelectorAll(".sidebar-nav a.zf-nav-item");
    const navTexts = Array.from(navLinks).map((el) => el.textContent.trim());
    expect(navTexts).not.toContain("Time");
    expect(navTexts).not.toContain("Absences");
    expect(navTexts).not.toContain("Calendar");
  });

  it("shows Time, Absences, Calendar for normal employee", async () => {
    currentUser.set({
      id: 2,
      first_name: "Eva",
      last_name: "Employee",
      role: "employee",
      tracks_time: true,
      nav: [
        { href: "/time", key: "Time", icon: "⏱" },
        { href: "/absences", key: "Absences", icon: "📅" },
        { href: "/calendar", key: "Calendar", icon: "🗓" },
        { href: "/dashboard", key: "Dashboard", icon: "🔔" },
        { href: "/reports", key: "Reports", icon: "📊" },
        { href: "/account", key: "Account", icon: "👤" },
      ],
      permissions: { can_approve: false, can_view_dashboard: true },
    });
    component = mount(Layout, { target });
    await settle();

    const navLinks = target.querySelectorAll(".sidebar-nav a.zf-nav-item");
    const navTexts = Array.from(navLinks).map((el) => el.textContent.trim());
    expect(navTexts).toContain("Time");
    expect(navTexts).toContain("Absences");
    expect(navTexts).toContain("Calendar");
    expect(navTexts).toContain("Dashboard");
    expect(navTexts).toContain("Reports");
  });
});