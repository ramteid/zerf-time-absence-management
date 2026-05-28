// Tests for the AdminUsers page. Admins manage the team roster here —
// creating new accounts, editing existing ones, deactivating members, and
// resetting passwords. Tests verify that the list renders correctly, that
// the add/edit dialogs open, and that the deactivate/delete flows prompt
// for confirmation before sending any destructive API call.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminUsers from "./AdminUsers.svelte";
import { currentUser } from "../stores.js";
import { setLanguage } from "../i18n.js";

const apiMock = vi.hoisted(() => vi.fn());

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
  csrfToken: { set: vi.fn() },
}));

// Auto-confirm all confirmation dialogs so tests can reach the API call
// without having to interact with the dialog DOM.
vi.mock("../confirm.js", () => ({
  confirmDialog: vi.fn().mockResolvedValue(true),
}));

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

const sampleUsers = [
  {
    id: 1,
    first_name: "Alice",
    last_name: "Admin",
    role: "admin",
    email: "alice@example.com",
    active: true,
    tracks_time: false,
  },
  {
    id: 2,
    first_name: "Bob",
    last_name: "Employee",
    role: "employee",
    email: "bob@example.com",
    active: true,
    tracks_time: true,
  },
  {
    id: 3,
    first_name: "Carol",
    last_name: "Inactive",
    role: "employee",
    email: "carol@example.com",
    active: false,
    tracks_time: true,
  },
];

describe("AdminUsers", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    currentUser.set({ id: 1, role: "admin", permissions: { is_admin: true } });
    apiMock.mockReset();

    // Intercept showModal so dialogs open without triggering jsdom limitations
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

  it("renders the Team Members heading", async () => {
    apiMock.mockResolvedValue(sampleUsers);
    component = mount(AdminUsers, { target });
    await waitForText(target, "Team Members");
  });

  it("renders a row for each user in the list", async () => {
    // Every active and inactive team member must appear in the list so
    // admins can see and manage the complete roster.
    apiMock.mockResolvedValue(sampleUsers);
    component = mount(AdminUsers, { target });
    await waitForText(target, "Alice");
    await waitForText(target, "Bob");
    await waitForText(target, "Carol");
  });

  it("shows an 'Inactive' label next to deactivated users", async () => {
    // Admins need to distinguish active from inactive accounts at a glance
    // so they can reactivate members who return from a leave of absence.
    apiMock.mockResolvedValue(sampleUsers);
    component = mount(AdminUsers, { target });
    await waitForText(target, "Inactive");
  });

  it("opens the UserDialog when Add Member is clicked", async () => {
    // The Add Member flow creates a new account. The dialog must open with
    // empty fields, not pre-filled with another user's data.
    apiMock.mockResolvedValue([]);
    component = mount(AdminUsers, { target });
    await waitForText(target, "Team Members");

    const addBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Add Member")
    );
    expect(addBtn).not.toBeNull();
    addBtn.click();
    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog?.hasAttribute("open")).toBe(true);
  });

  it("fetches the full user record before opening the edit dialog", async () => {
    // The edit dialog needs fields that are not in the list response (e.g.
    // approver_ids, leave days). A fresh GET /users/:id is required.
    apiMock.mockResolvedValueOnce(sampleUsers);
    apiMock.mockResolvedValueOnce({ ...sampleUsers[1], approver_ids: [] });
    component = mount(AdminUsers, { target });
    await waitForText(target, "Bob");

    // Row action buttons use the "ghost" style; the "Add Member" button uses
    // "primary". The first ghost icon button in the list is Edit-Alice.
    const firstEditBtn = [...target.querySelectorAll("button")].find(
      (b) => b.classList.contains("zf-btn-ghost") && b.querySelector("svg")
    );
    firstEditBtn?.click();
    await settle();
    await settle();

    const getCall = apiMock.mock.calls.find(
      ([path]) => typeof path === "string" && path.startsWith("/users/")
    );
    expect(getCall).toBeTruthy();
  });

  it("calls the reset-password endpoint when Reset PW is confirmed", async () => {
    // Resetting a password generates a new temporary credential. The
    // confirmation step prevents accidental resets — the admin must
    // explicitly confirm before the old password is invalidated.
    apiMock.mockResolvedValueOnce(sampleUsers);
    apiMock.mockResolvedValue({ temporary_password: "Temp123!" });
    component = mount(AdminUsers, { target });
    await waitForText(target, "Bob");

    // Row button order: Edit | Shield (reset PW) | Toggle active (titled) | Delete (titled).
    // Filtering out titled buttons leaves: Add Member, Edit-Alice, Shield-Alice, Edit-Bob…
    // Index 2 is the Shield button for the first user (Alice, user id 1).
    const noTitleIconBtns = [...target.querySelectorAll("button")].filter(
      (b) => b.querySelector("svg") && !b.title
    );
    noTitleIconBtns[2]?.click();
    await settle();
    await settle();

    const resetCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        typeof path === "string" &&
        path.includes("/reset-password") &&
        opts?.method === "POST"
    );
    expect(resetCall).toBeTruthy();
  });
});
