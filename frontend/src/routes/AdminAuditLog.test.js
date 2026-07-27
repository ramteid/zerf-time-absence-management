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
    if (urlPath.startsWith("/audit-log"))
      return { entries: mockState.entries, total: mockState.entries.length };
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
    settings.set({
      ui_language: "en",
      time_format: "24h",
      timezone: "Europe/Berlin",
    });
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

  it("shows each individual time entry change as its own row", async () => {
    // Editing single entries produces one audit row per entry — these must
    // never be merged, even for the same user, action, and week.
    mockState.users = [{ id: 7, first_name: "Alex", last_name: "Admin" }];
    mockState.entries = [
      {
        id: 1,
        user_id: 7,
        action: "updated",
        table_name: "time_entries",
        record_id: 101,
        before_data: JSON.stringify({ entry_date: "2026-05-05" }),
        after_data: JSON.stringify({ entry_date: "2026-05-05" }),
        occurred_at: "2026-05-05T09:00:00Z",
      },
      {
        id: 2,
        user_id: 7,
        action: "updated",
        table_name: "time_entries",
        record_id: 102,
        before_data: JSON.stringify({ entry_date: "2026-05-06" }),
        after_data: JSON.stringify({ entry_date: "2026-05-06" }),
        occurred_at: "2026-05-05T09:00:25Z",
      },
    ];

    component = mount(AdminAuditLog, { target });
    await settle();

    const rows = target.querySelectorAll(".audit-row");
    expect(rows).toHaveLength(2);
  });

  it("shows a week-level decision as a single row with the reason and every day entry in its popup", async () => {
    mockState.users = [
      { id: 1, first_name: "Ada", last_name: "Lead" },
      { id: 7, first_name: "Alex", last_name: "Admin" },
    ];
    mockState.entries = [
      {
        id: 5,
        user_id: 1,
        action: "rejected",
        table_name: "time_entry_weeks",
        record_id: 7,
        before_data: JSON.stringify({ status: "submitted" }),
        after_data: JSON.stringify({
          status: "rejected",
          user_id: 7,
          week_start_date: "2026-05-04",
          entry_count: 2,
          reason: "Wednesday is missing",
          entries: [
            {
              id: 101,
              entry_date: "2026-05-04",
              start_time: "08:00",
              end_time: "16:00",
              category_id: 3,
              category_name: "Core Duties",
              comment: "on-site",
            },
            {
              id: 102,
              entry_date: "2026-05-05",
              start_time: "09:00",
              end_time: "17:00",
              category_id: 3,
              category_name: "Core Duties",
              comment: null,
            },
          ],
        }),
        occurred_at: "2026-05-06T09:00:00Z",
      },
    ];

    component = mount(AdminAuditLog, { target });
    await settle();

    const rows = target.querySelectorAll(".audit-row");
    expect(rows).toHaveLength(1);

    rows[0].click();
    await settle();
    expect(target.textContent).toContain("Wednesday is missing");
    expect(target.textContent).toContain("Core Duties");
    expect(target.textContent).toContain("on-site");
    // Both days show up individually, not just a count.
    expect(target.textContent).toMatch(/08:00.*16:00/);
    expect(target.textContent).toMatch(/09:00.*17:00/);
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
