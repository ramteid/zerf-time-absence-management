import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminAuditLog from "./AdminAuditLog.svelte";
import { setLanguage } from "../i18n.js";
import { settings } from "../stores.js";

const mockState = vi.hoisted(() => ({
  entries: [],
  users: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    if (urlPath === "/audit-log") return mockState.entries;
    if (urlPath === "/users") return mockState.users;
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("AdminAuditLog", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "Europe/Berlin" });
    mockState.entries = [];
    mockState.users = [];
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("merges time entry audit rows for the same week and action", async () => {
    mockState.users = [{ id: 7, first_name: "Alex", last_name: "Admin" }];
    mockState.entries = [
      {
        id: 1,
        user_id: 7,
        action: "approved",
        table_name: "time_entries",
        record_id: 101,
        before_data: null,
        after_data: JSON.stringify({ entry_date: "2026-05-05" }),
        occurred_at: "2026-05-05T09:00:00Z",
      },
      {
        id: 2,
        user_id: 7,
        action: "approved",
        table_name: "time_entries",
        record_id: 102,
        before_data: null,
        after_data: JSON.stringify({ entry_date: "2026-05-06" }),
        occurred_at: "2026-05-05T09:00:25Z",
      },
    ];

    component = mount(AdminAuditLog, { target });
    await settle();

    const rows = target.querySelectorAll(".audit-row");
    expect(rows).toHaveLength(1);
    expect(target.textContent).toMatch(/\(2 day entries\)/);

    rows[0].click();
    await settle();
    expect(target.textContent).toContain("Days");
    expect(target.textContent).toContain("2");
  });

  it("merges time entry rows regardless of time gap between them", async () => {
    mockState.users = [{ id: 7, first_name: "Alex", last_name: "Admin" }];
    mockState.entries = [
      {
        id: 11,
        user_id: 7,
        action: "approved",
        table_name: "time_entries",
        record_id: 201,
        before_data: null,
        after_data: JSON.stringify({ entry_date: "2026-05-05" }),
        occurred_at: "2026-05-05T09:00:00Z",
      },
      {
        id: 12,
        user_id: 7,
        action: "approved",
        table_name: "time_entries",
        record_id: 202,
        before_data: null,
        after_data: JSON.stringify({ entry_date: "2026-05-06" }),
        occurred_at: "2026-05-05T09:01:30Z",
      },
    ];

    component = mount(AdminAuditLog, { target });
    await settle();

    const rows = target.querySelectorAll(".audit-row");
    expect(rows).toHaveLength(1);
    expect(target.textContent).toMatch(/\(2 day entries\)/);
  });

  it("does not merge time entry rows from different weeks", async () => {
    mockState.users = [{ id: 7, first_name: "Alex", last_name: "Admin" }];
    mockState.entries = [
      {
        id: 13,
        user_id: 7,
        action: "approved",
        table_name: "time_entries",
        record_id: 301,
        before_data: null,
        after_data: JSON.stringify({ entry_date: "2026-05-04" }), // week 19
        occurred_at: "2026-05-04T09:00:00Z",
      },
      {
        id: 14,
        user_id: 7,
        action: "approved",
        table_name: "time_entries",
        record_id: 302,
        before_data: null,
        after_data: JSON.stringify({ entry_date: "2026-05-11" }), // week 20
        occurred_at: "2026-05-04T09:00:01Z",
      },
    ];

    component = mount(AdminAuditLog, { target });
    await settle();

    const rows = target.querySelectorAll(".audit-row");
    expect(rows).toHaveLength(2);
  });

  it("renders readable user summary instead of raw field keys", async () => {
    mockState.users = [{ id: 1, first_name: "Admin", last_name: "User" }];
    mockState.entries = [
      {
        id: 30,
        user_id: 1,
        action: "updated",
        table_name: "users",
        record_id: 99,
        before_data: null,
        after_data: JSON.stringify({
          first_name: "Max",
          last_name: "Mustermann",
          email: "max@example.com",
        }),
        occurred_at: "2026-05-05T09:00:00Z",
      },
    ];

    component = mount(AdminAuditLog, { target });
    await settle();

    expect(target.textContent).toContain("Max Mustermann (max@example.com)");
    expect(target.textContent).not.toContain("first_name:");
    expect(target.textContent).not.toContain("last_name:");
  });
});
