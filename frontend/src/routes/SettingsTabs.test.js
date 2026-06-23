import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import SettingsTabs from "./SettingsTabs.svelte";
import { path, currentUser } from "../stores.js";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

const adminUser = {
  id: 1,
  role: "admin",
  permissions: {
    is_admin: true,
    can_manage_settings: true,
    can_manage_team_settings: true,
  },
};

const leadUser = {
  id: 2,
  role: "team_lead",
  permissions: {
    is_admin: false,
    can_manage_settings: false,
    can_manage_team_settings: true,
  },
};

const leadUserWithAssistantManagement = {
  id: 3,
  role: "team_lead",
  permissions: {
    is_admin: false,
    can_manage_settings: false,
    can_manage_team_settings: true,
    can_manage_team_users: true,
  },
};

describe("SettingsTabs", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    path.set("/settings/general");
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders all admin tabs for an admin user", async () => {
    currentUser.set(adminUser);
    component = mount(SettingsTabs, { target });
    await settle();
    const links = target.querySelectorAll("a[data-link]");
    // 7 admin tabs + 1 team tab = 8
    expect(links.length).toBe(8);
    const text = target.textContent;
    expect(text).toContain("Settings");
    expect(text).toContain("Users");
    expect(text).toContain("Categories");
    expect(text).toContain("Holidays");
    expect(text).toContain("Email");
    expect(text).toContain("Team Settings");
  });

  it("renders only the Team Settings tab for a team lead", async () => {
    currentUser.set(leadUser);
    component = mount(SettingsTabs, { target });
    await settle();
    const links = target.querySelectorAll("a[data-link]");
    expect(links.length).toBe(1);
    expect(target.textContent).toContain("Team Settings");
    expect(target.textContent).not.toContain("Categories");
  });

  it("adds a scoped Users tab for a lead granted assistant management", async () => {
    currentUser.set(leadUserWithAssistantManagement);
    component = mount(SettingsTabs, { target });
    await settle();
    const links = target.querySelectorAll("a[data-link]");
    expect(links.length).toBe(2);
    const teamUsersLink = [...links].find(
      (a) => a.getAttribute("href") === "/settings/team-users",
    );
    expect(teamUsersLink).not.toBeNull();
    expect(target.textContent).toContain("Team Settings");
  });

  it("highlights the active tab based on path", async () => {
    currentUser.set(adminUser);
    path.set("/settings/users");
    component = mount(SettingsTabs, { target });
    await settle();
    const usersLink = [...target.querySelectorAll("a")].find(
      (a) => a.getAttribute("href") === "/settings/users"
    );
    expect(usersLink).not.toBeNull();
    expect(usersLink.style.color).toContain("var(--accent)");
  });

  it("strips query string when matching active tab", async () => {
    currentUser.set(adminUser);
    path.set("/settings/audit-log?page=2");
    component = mount(SettingsTabs, { target });
    await settle();
    const auditLink = [...target.querySelectorAll("a")].find(
      (a) => a.getAttribute("href") === "/settings/audit-log"
    );
    expect(auditLink).not.toBeNull();
    expect(auditLink.style.color).toContain("var(--accent)");
  });
});
