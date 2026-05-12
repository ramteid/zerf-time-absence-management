import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminEmail from "./AdminEmail.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    smtp_enabled: true,
    smtp_host: "smtp.example.com",
    smtp_port: 587,
    smtp_username: "mailer",
    smtp_from: "noreply@example.com",
    smtp_encryption: "starttls",
    smtp_password_set: true,
    submission_reminders_enabled: true,
    approval_reminders_enabled: true,
  },
}));

const apiMock = vi.hoisted(() => vi.fn(async (path, opts = {}) => {
  if (path === "/settings" && (!opts.method || opts.method === "GET")) {
    return mockState.settings;
  }
  if (path === "/settings/smtp" && opts.method === "PUT") {
    mockState.settings = {
      ...mockState.settings,
      ...opts.body,
      smtp_password_set:
        opts.body.smtp_password !== undefined
          ? true
          : mockState.settings.smtp_password_set,
    };
    return mockState.settings;
  }
  if (path === "/settings/smtp/test" && opts.method === "POST") {
    return { ok: true };
  }
  throw new Error(`Unhandled API path: ${path}`);
}));

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

describe("AdminEmail", () => {
  let component;
  let target;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    apiMock.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("renders submission reminders checkbox as checked when setting is true", async () => {
    mockState.settings = { ...mockState.settings, submission_reminders_enabled: true };
    component = mount(AdminEmail, { target });
    await settle();

    const checkboxes = target.querySelectorAll('input[type="checkbox"]');
    const remindersCheckbox = [...checkboxes].find(
      (cb) => cb.closest("label")?.textContent?.includes("reminders"),
    );
    expect(remindersCheckbox).not.toBeNull();
    expect(remindersCheckbox.checked).toBe(true);
  });

  it("renders submission reminders checkbox as unchecked when setting is false", async () => {
    mockState.settings = { ...mockState.settings, submission_reminders_enabled: false };
    component = mount(AdminEmail, { target });
    await settle();

    const checkboxes = target.querySelectorAll('input[type="checkbox"]');
    const remindersCheckbox = [...checkboxes].find(
      (cb) => cb.closest("label")?.textContent?.includes("reminders"),
    );
    expect(remindersCheckbox).not.toBeNull();
    expect(remindersCheckbox.checked).toBe(false);
  });

  it("includes submission_reminders_enabled in the save body", async () => {
    mockState.settings = { ...mockState.settings, submission_reminders_enabled: true };
    component = mount(AdminEmail, { target });
    await settle();

    const saveBtn = [...target.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === "Save",
    );
    expect(saveBtn).not.toBeNull();
    saveBtn.click();
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/settings/smtp" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
    expect(saveCall[1].body.submission_reminders_enabled).toBe(true);
  });

  it("includes approval_reminders_enabled in the save body", async () => {
    mockState.settings = { ...mockState.settings, approval_reminders_enabled: true };
    component = mount(AdminEmail, { target });
    await settle();

    const saveBtn = [...target.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === "Save",
    );
    expect(saveBtn).not.toBeNull();
    saveBtn.click();
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/settings/smtp" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
    expect(saveCall[1].body.approval_reminders_enabled).toBe(true);
  });
});