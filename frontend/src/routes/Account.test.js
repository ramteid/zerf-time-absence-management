import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Account from "./Account.svelte";
import {
  currentUser,
  settings,
  categories,
  absenceCategories,
  earliestStartDate,
} from "../stores.js";
import { setLanguage } from "../i18n.js";

const apiMock = vi.hoisted(() => vi.fn());

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

const baseUser = {
  id: 1,
  first_name: "Alice",
  last_name: "Smith",
  email: "alice@example.com",
  role: "employee",
  weekly_hours: 40,
  workdays_per_week: 5,
  start_date: "2022-01-01",
  must_change_password: false,
  dark_mode: false,
  approvers: [],
};

describe("Account", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    currentUser.set({ ...baseUser });
    // The boot-loaded stores start empty/unset so the first-login reload paths
    // in changePassword() are exercised deterministically.
    categories.set([]);
    absenceCategories.set([]);
    earliestStartDate.set(null);
    apiMock.mockReset();
    apiMock.mockResolvedValue([]);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the user's name", async () => {
    component = mount(Account, { target });
    await settle();
    expect(target.textContent).toContain("Alice");
    expect(target.textContent).toContain("Smith");
  });

  it("renders the user's email as readonly", async () => {
    component = mount(Account, { target });
    await settle();
    const emailInput = target.querySelector("#account-email");
    expect(emailInput).not.toBeNull();
    expect(emailInput.value).toBe("alice@example.com");
    expect(emailInput.readOnly).toBe(true);
  });

  it("shows must-change-password banner when required", async () => {
    currentUser.set({ ...baseUser, must_change_password: true });
    component = mount(Account, { target });
    await settle();
    expect(target.textContent).toContain("temporary password");
  });

  it("hides current-password field when must_change_password is true", async () => {
    currentUser.set({ ...baseUser, must_change_password: true });
    component = mount(Account, { target });
    await settle();
    expect(target.querySelector("#account-current-password")).toBeNull();
  });

  it("shows approvers when present", async () => {
    currentUser.set({
      ...baseUser,
      approvers: [{ first_name: "Bob", last_name: "Manager" }],
    });
    component = mount(Account, { target });
    await settle();
    expect(target.textContent).toContain("Bob Manager");
  });

  it("shows password mismatch error", async () => {
    component = mount(Account, { target });
    await settle();

    const nw = target.querySelector("#account-new-password");
    const nw2 = target.querySelector("#account-confirm-password");
    nw.value = "NewPassword123!";
    nw.dispatchEvent(new Event("input"));
    nw2.value = "DifferentPass456!";
    nw2.dispatchEvent(new Event("input"));

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.trim() === "Save"
    );
    saveBtn.click();
    await settle();

    expect(target.querySelector(".error-text").textContent).toContain("match");
  });

  it("calls API to change password successfully", async () => {
    apiMock.mockResolvedValueOnce([]);
    apiMock.mockResolvedValueOnce({});

    component = mount(Account, { target });
    await settle();

    const cur = target.querySelector("#account-current-password");
    const nw = target.querySelector("#account-new-password");
    const nw2 = target.querySelector("#account-confirm-password");
    cur.value = "OldPass123!";
    cur.dispatchEvent(new Event("input"));
    nw.value = "NewPass456@!!";
    nw.dispatchEvent(new Event("input"));
    nw2.value = "NewPass456@!!";
    nw2.dispatchEvent(new Event("input"));

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.trim() === "Save"
    );
    saveBtn.click();
    await settle();
    await settle();

    expect(apiMock).toHaveBeenCalledWith(
      "/auth/password",
      expect.objectContaining({ method: "PUT" }),
    );
  });

  it("reloads time and absence categories after a first-login password change", async () => {
    // First-login user: forced password change, both category stores empty
    // because boot skipped loading them while must_change_password was true.
    currentUser.set({ ...baseUser, must_change_password: true });
    apiMock.mockResolvedValue([]);

    component = mount(Account, { target });
    await settle();

    // No current-password field in must_change_password mode — only new + confirm.
    const nw = target.querySelector("#account-new-password");
    const nw2 = target.querySelector("#account-confirm-password");
    nw.value = "NewPass456@!!";
    nw.dispatchEvent(new Event("input"));
    nw2.value = "NewPass456@!!";
    nw2.dispatchEvent(new Event("input"));

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.trim() === "Save"
    );
    saveBtn.click();
    await settle();
    await settle();

    // The fix: every store boot skipped must be repopulated, not just
    // /categories. Without the absence-categories reload the absence-request
    // dropdown stays empty for the rest of the session; earliest-start-date
    // backs report date-picker bounds.
    expect(apiMock).toHaveBeenCalledWith("/categories");
    expect(apiMock).toHaveBeenCalledWith("/absence-categories");
    expect(apiMock).toHaveBeenCalledWith("/users/earliest-start-date");
  });

  it("shows API error on password change failure", async () => {
    apiMock.mockResolvedValueOnce([]);
    apiMock.mockRejectedValueOnce({ message: "Wrong current password" });

    component = mount(Account, { target });
    await settle();

    const nw = target.querySelector("#account-new-password");
    const nw2 = target.querySelector("#account-confirm-password");
    nw.value = "NewPass456@!!";
    nw.dispatchEvent(new Event("input"));
    nw2.value = "NewPass456@!!";
    nw2.dispatchEvent(new Event("input"));

    const saveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.trim() === "Save"
    );
    saveBtn.click();
    await settle();
    await settle();

    expect(target.querySelector(".error-text").textContent).toContain(
      "Wrong current password"
    );
  });

  it("fetches leave days on load", async () => {
    apiMock.mockResolvedValue([
      { year: new Date().getFullYear(), days: 25 },
    ]);
    component = mount(Account, { target });
    await settle();
    expect(apiMock).toHaveBeenCalledWith(
      expect.stringContaining("/users/1/leave-days"),
    );
  });

  it("renders dark mode toggle button", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(Account, { target });
    await settle();
    expect(target.textContent).toContain("Dark mode");
  });
});
