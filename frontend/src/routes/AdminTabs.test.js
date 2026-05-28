import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminTabs from "./AdminTabs.svelte";
import { path } from "../stores.js";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("AdminTabs", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    path.set("/admin/settings");
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders all admin navigation tabs", async () => {
    component = mount(AdminTabs, { target });
    await settle();
    const links = target.querySelectorAll("a[data-link]");
    expect(links.length).toBeGreaterThanOrEqual(5);
  });

  it("highlights the active tab for /admin/settings", async () => {
    path.set("/admin/settings");
    component = mount(AdminTabs, { target });
    await settle();
    const settingsLink = [...target.querySelectorAll("a")].find((a) =>
      a.getAttribute("href") === "/admin/settings"
    );
    expect(settingsLink).not.toBeNull();
    expect(settingsLink.style.color).toContain("var(--accent)");
  });

  it("highlights the active tab for /admin/users", async () => {
    path.set("/admin/users");
    component = mount(AdminTabs, { target });
    await settle();
    const usersLink = [...target.querySelectorAll("a")].find((a) =>
      a.getAttribute("href") === "/admin/users"
    );
    expect(usersLink).not.toBeNull();
    expect(usersLink.style.color).toContain("var(--accent)");
  });

  it("renders tab labels in English", async () => {
    component = mount(AdminTabs, { target });
    await settle();
    const text = target.textContent;
    expect(text).toContain("Settings");
    expect(text).toContain("Team");
    expect(text).toContain("Categories");
    expect(text).toContain("Holidays");
    expect(text).toContain("Email");
  });

  it("strips query string when matching active tab", async () => {
    path.set("/admin/audit-log?page=2");
    component = mount(AdminTabs, { target });
    await settle();
    const auditLink = [...target.querySelectorAll("a")].find((a) =>
      a.getAttribute("href") === "/admin/audit-log"
    );
    expect(auditLink).not.toBeNull();
    expect(auditLink.style.color).toContain("var(--accent)");
  });
});
