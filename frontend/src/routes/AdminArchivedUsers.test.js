// Tests for the AdminArchivedUsers page. This page lists archived accounts and
// lets admins restore them via a dialog. Tests verify list rendering, empty
// state, and that clicking Restore opens the RestoreUserDialog.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminArchivedUsers from "./AdminArchivedUsers.svelte";
import { currentUser } from "../stores.js";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

const apiMock = vi.hoisted(() => vi.fn());
vi.mock("../api.js", () => ({
  api: apiMock,
  csrfToken: { set: vi.fn() },
}));

// usersApi module re-exports api() calls; mock it to control archived-users data.
vi.mock("../lib/api/usersApi.js", () => ({
  getArchivedUsers: vi.fn(),
  restoreUser: vi.fn(),
}));

import { getArchivedUsers } from "../lib/api/usersApi.js";

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`Text not found: "${text}"`);
}

const sampleArchived = [
  {
    id: 10,
    first_name: "Frank",
    last_name: "Former",
    role: "employee",
    email: "frank@example.com",
    archived_at: "2025-03-15T10:00:00Z",
  },
  {
    id: 11,
    first_name: "Grace",
    last_name: "Gone",
    role: "team_lead",
    email: "grace@example.com",
    archived_at: "2025-01-01T00:00:00Z",
  },
];

describe("AdminArchivedUsers", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    currentUser.set({ id: 1, role: "admin", permissions: { is_admin: true } });
    getArchivedUsers.mockReset();
    apiMock.mockReset();

    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
  });

  it("renders the Archived Users heading", async () => {
    // Page title must be visible so admins know which section they are in.
    getArchivedUsers.mockResolvedValue(sampleArchived);
    component = mount(AdminArchivedUsers, { target });
    await waitForText(target, "Archived Users");
  });

  it("renders a row for each archived user", async () => {
    // Both archived members must appear in the list so admins can find and
    // restore them.
    getArchivedUsers.mockResolvedValue(sampleArchived);
    component = mount(AdminArchivedUsers, { target });
    await waitForText(target, "Frank");
    await waitForText(target, "Grace");
  });

  it("shows an empty state when no users are archived", async () => {
    // When no accounts have been archived, an informative message is shown
    // instead of an empty card with no rows.
    getArchivedUsers.mockResolvedValue([]);
    component = mount(AdminArchivedUsers, { target });
    await waitForText(target, "No archived users.");
  });

  it("opens the RestoreUserDialog when Restore is clicked", async () => {
    // Clicking Restore must open the dialog so the admin can configure the
    // start-date option and approver assignments before committing.
    getArchivedUsers.mockResolvedValue(sampleArchived);
    // RestoreUserDialog calls api("/users") on mount.
    apiMock.mockResolvedValue([]);
    component = mount(AdminArchivedUsers, { target });
    await waitForText(target, "Frank");

    const restoreBtn = [...target.querySelectorAll("button")].find(
      (b) => b.title === "Restore"
    );
    expect(restoreBtn).not.toBeNull();
    restoreBtn.click();
    await settle();
    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog?.hasAttribute("open")).toBe(true);
  });
});
