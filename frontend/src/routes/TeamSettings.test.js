// Tests for the TeamSettings page. Team leads / admins use this page to
// enable or disable "auto-approve edit requests" per employee. When enabled,
// an employee's reopen request is approved automatically without requiring
// the approver to take action manually. Tests verify:
//   - The roster loads and renders with the correct labels
//   - Toggling the checkbox calls the correct API endpoint
//   - The current user's own row is labelled with "(you)"

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import TeamSettings from "./TeamSettings.svelte";
import { currentUser } from "../stores.js";
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

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`Text not found: "${text}"`);
}

const sampleRows = [
  {
    user_id: 1,
    first_name: "Alice",
    last_name: "Lead",
    role: "team_lead",
    email: "alice@example.com",
    allow_reopen_without_approval: false,
  },
  {
    user_id: 2,
    first_name: "Bob",
    last_name: "Emp",
    role: "employee",
    email: "bob@example.com",
    allow_reopen_without_approval: true,
  },
];

describe("TeamSettings", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { is_admin: false },
    });
    apiMock.mockReset();
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Team Settings heading", async () => {
    apiMock.mockResolvedValue(sampleRows);
    component = mount(TeamSettings, { target });
    await waitForText(target, "Team Settings");
  });

  it("renders each team member's name in the list", async () => {
    apiMock.mockResolvedValue(sampleRows);
    component = mount(TeamSettings, { target });
    await waitForText(target, "Alice");
    await waitForText(target, "Bob");
  });

  it("marks the current user's row with '(you)'", async () => {
    // Helps team leads identify their own row, which has additional
    // restrictions (a non-admin cannot toggle their own auto-approve setting).
    apiMock.mockResolvedValue(sampleRows);
    component = mount(TeamSettings, { target });
    await waitForText(target, "you");
  });

  it("shows 'No data.' when the team list is empty", async () => {
    // An empty roster means no team members have been added yet or the
    // requester has no team to manage. A clear message avoids confusion.
    apiMock.mockResolvedValue([]);
    component = mount(TeamSettings, { target });
    await waitForText(target, "No data.");
  });

  it("shows the loading indicator before data arrives", async () => {
    // The loading state prevents the "No data." message from briefly
    // flashing before the first API response arrives.
    let resolveLoad;
    apiMock.mockReturnValue(new Promise((r) => { resolveLoad = r; }));
    component = mount(TeamSettings, { target });
    await settle();
    expect(target.textContent).toContain("Loading...");
    resolveLoad(sampleRows);
    await waitForText(target, "Alice");
  });

  it("calls the team-settings PUT endpoint when a checkbox is toggled", async () => {
    // Each toggle immediately saves to the backend. If the save fails, the
    // page re-fetches server state to keep the UI in sync with reality.
    apiMock.mockResolvedValue(sampleRows);
    component = mount(TeamSettings, { target });
    await waitForText(target, "Bob");

    const checkboxes = target.querySelectorAll('input[type="checkbox"]');
    // Bob's row is the second; his checkbox starts checked (true)
    const bobCheckbox = checkboxes[1];
    expect(bobCheckbox).not.toBeNull();
    bobCheckbox.click();
    await settle();
    await settle();

    const putCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        typeof path === "string" &&
        path.startsWith("/team-settings/") &&
        opts?.method === "PUT"
    );
    expect(putCall).toBeTruthy();
  });
});
