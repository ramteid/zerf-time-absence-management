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
        opts.body.smtp_password === undefined
          ? mockState.settings.smtp_password_set
          : opts.body.smtp_password !== "",
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
  let originalSettings;
  let target;

  beforeEach(() => {
    originalSettings = structuredClone(mockState.settings);
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
    mockState.settings = originalSettings;
    target.remove();
  });

  it("sends an empty password when clearing the stored SMTP password", async () => {
    component = mount(AdminEmail, { target });
    await settle();
    await settle();

    const clearCheckbox = target.querySelector("#smtp-clear-password");
    clearCheckbox.click();
    await settle();

    target.querySelector(".kz-btn-primary").click();
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/settings/smtp" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
    expect(saveCall[1].body.smtp_password).toBe("");
  });
});