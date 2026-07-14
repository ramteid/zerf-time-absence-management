import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminSystemLog from "./AdminSystemLog.svelte";
import { setLanguage } from "../i18n.js";
import { settings } from "../stores.js";

const mockState = vi.hoisted(() => ({
  // offset -> { entries, total }
  pages: {},
  calls: [],
}));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: vi.fn(async (urlPath) => {
    mockState.calls.push(urlPath);
    const url = new URL(urlPath, "http://localhost");
    if (url.pathname === "/logs") {
      const offset = Number(url.searchParams.get("offset") || 0);
      return mockState.pages[offset] ?? { entries: [], total: 0 };
    }
    throw new Error(`Unhandled API path: ${urlPath}`);
  }),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function logEntry(overrides = {}) {
  return {
    id: 1,
    level: "warn",
    message: "something odd happened",
    target: "zerf::email",
    fields: null,
    occurred_at: "2026-05-05T09:00:00Z",
    ...overrides,
  };
}

describe("AdminSystemLog", () => {
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
    mockState.pages = {};
    mockState.calls = [];
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("renders warn and error rows with level labels and truncated messages", async () => {
    const longMessage = "x".repeat(300);
    mockState.pages[0] = {
      entries: [
        logEntry({ id: 2, level: "error", message: longMessage }),
        logEntry({ id: 1, level: "warn", message: "smtp connect refused" }),
      ],
      total: 2,
    };

    component = mount(AdminSystemLog, { target });
    await settle();

    const rows = target.querySelectorAll(".log-row");
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("Error");
    expect(rows[1].textContent).toContain("Warning");
    // Long messages are cut in the list...
    expect(rows[0].textContent).toContain(`${"x".repeat(200)}…`);
    expect(rows[0].textContent).not.toContain("x".repeat(201));
    expect(rows[1].textContent).toContain("smtp connect refused");
  });

  it("shows the full message, source and metadata in the detail popup", async () => {
    const longMessage = `boom ${"y".repeat(280)}`;
    mockState.pages[0] = {
      entries: [
        logEntry({
          id: 5,
          level: "error",
          message: longMessage,
          fields: { user_id: "7", retry: "true" },
        }),
      ],
      total: 1,
    };

    component = mount(AdminSystemLog, { target });
    await settle();

    target.querySelector(".log-row").click();
    await settle();

    // ...but the popup shows the full text plus context.
    expect(target.textContent).toContain(longMessage);
    expect(target.textContent).toContain("zerf::email");
    expect(target.textContent).toContain("user_id");
    expect(target.textContent).toContain("7");
    expect(target.textContent).toContain("retry");
  });

  it("paginates through pages of 100 without reloading", async () => {
    mockState.pages[0] = {
      entries: [logEntry({ id: 300, message: "newest entry" })],
      total: 250,
    };
    mockState.pages[100] = {
      entries: [logEntry({ id: 150, message: "second page entry" })],
      total: 250,
    };

    component = mount(AdminSystemLog, { target });
    await settle();

    expect(target.textContent).toContain("Page 1 of 3");
    const [prevButton, nextButton] =
      target.querySelectorAll(".log-pager button");
    expect(prevButton.disabled).toBe(true);

    nextButton.click();
    await settle();

    expect(mockState.calls).toContain("/logs?limit=100&offset=100");
    expect(target.textContent).toContain("Page 2 of 3");
    expect(target.textContent).toContain("second page entry");
    expect(target.querySelectorAll(".log-pager button")[0].disabled).toBe(
      false,
    );
  });

  it("shows an empty state when there are no log entries", async () => {
    component = mount(AdminSystemLog, { target });
    await settle();

    expect(target.querySelectorAll(".log-row")).toHaveLength(0);
    expect(target.textContent).toContain("No log entries.");
    // No pager for a single (empty) page.
    expect(target.querySelector(".log-pager")).toBeNull();
  });
});
