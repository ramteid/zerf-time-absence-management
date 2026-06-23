import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Login from "./Login.svelte";
import { settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const apiMock = vi.hoisted(() => vi.fn());
const csrfTokenMock = vi.hoisted(() => ({ set: vi.fn() }));

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
  csrfToken: csrfTokenMock,
}));

vi.mock("../passwordCredentials.js", () => ({
  storePasswordCredential: vi.fn().mockResolvedValue(false),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("Login", () => {
  let target;
  let component;
  let appDiv;

  beforeEach(() => {
    // Login.svelte accesses document.getElementById("app")
    appDiv = document.createElement("div");
    appDiv.id = "app";
    document.body.appendChild(appDiv);

    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    apiMock.mockReset();
    // Default: suppress window.location.assign
    vi.spyOn(window, "location", "get").mockReturnValue({
      ...window.location,
      assign: vi.fn(),
      search: "",
      pathname: "/",
    });
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    appDiv.remove();
    vi.restoreAllMocks();
  });

  it("renders sign-in form by default", async () => {
    component = mount(Login, { target });
    await settle();
    expect(target.textContent).toContain("Sign in");
    expect(target.querySelector('input[type="email"]')).not.toBeNull();
    expect(target.querySelector('input[type="password"]')).not.toBeNull();
  });

  it("renders organization name from settings", async () => {
    settings.set({ organization_name: "Acme Corp", ui_language: "en" });
    component = mount(Login, { target });
    await settle();
    expect(target.textContent).toContain("Acme Corp");
  });

  it("shows forgot-password view when clicking Forgot password", async () => {
    component = mount(Login, { target });
    await settle();
    const forgotBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Forgot password"),
    );
    expect(forgotBtn).not.toBeNull();
    forgotBtn.click();
    await settle();
    expect(target.textContent).toContain("Send reset link");
  });

  it("shows login error on failed login", async () => {
    apiMock.mockRejectedValueOnce({ message: "invalid_credentials" });
    component = mount(Login, { target });
    await settle();

    const emailInput = target.querySelector('input[type="email"]');
    const passwordInput = target.querySelector('input[type="password"]');
    emailInput.value = "user@example.com";
    emailInput.dispatchEvent(new Event("input"));
    passwordInput.value = "wrongpassword";
    passwordInput.dispatchEvent(new Event("input"));

    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    await settle();

    // Should show an error message (not empty)
    const errorDiv = target.querySelector(".error-text");
    expect(errorDiv).not.toBeNull();
  });

  it("shows archived account error for account_archived", async () => {
    apiMock.mockRejectedValueOnce({ apiMessage: "account_archived" });
    component = mount(Login, { target });
    await settle();

    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    await settle();

    const errorDiv = target.querySelector(".error-text");
    expect(errorDiv).not.toBeNull();
  });

  it("navigates to /dashboard after successful login", async () => {
    const assignSpy = vi.fn();
    Object.defineProperty(window, "location", {
      value: { assign: assignSpy, search: "", pathname: "/" },
      writable: true,
    });

    apiMock
      .mockResolvedValueOnce({ csrf_token: "tok" })
      .mockResolvedValueOnce({
        nav: [{ key: "Dashboard", href: "/dashboard" }],
        must_change_password: false,
        must_configure_settings: false,
        home: "/dashboard",
      });

    component = mount(Login, { target });
    await settle();

    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    await settle();
    await settle();

    expect(assignSpy).toHaveBeenCalledWith("/dashboard");
  });

  it("navigates to /account when must_change_password", async () => {
    const assignSpy = vi.fn();
    Object.defineProperty(window, "location", {
      value: { assign: assignSpy, search: "", pathname: "/" },
      writable: true,
    });

    apiMock
      .mockResolvedValueOnce({ csrf_token: "tok" })
      .mockResolvedValueOnce({
        nav: [],
        must_change_password: true,
      });

    component = mount(Login, { target });
    await settle();

    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    await settle();
    await settle();

    expect(assignSpy).toHaveBeenCalledWith("/account");
  });

  it("forgot password view — sends reset link", async () => {
    apiMock.mockResolvedValueOnce({});
    component = mount(Login, { target });
    await settle();

    // Navigate to forgot view
    const forgotBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Forgot password"),
    );
    forgotBtn.click();
    await settle();

    const forgotForm = target.querySelector("form");
    forgotForm.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await settle();
    await settle();

    expect(target.textContent).toContain("If your email address is registered");
  });

  it("forgot password view — shows error on unavailable reset", async () => {
    apiMock.mockRejectedValueOnce({ apiMessage: "password_reset_unavailable" });
    component = mount(Login, { target });
    await settle();

    const forgotBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Forgot password"),
    );
    forgotBtn.click();
    await settle();

    const forgotForm = target.querySelector("form");
    forgotForm.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await settle();
    await settle();

    const errorDiv = target.querySelector(".error-text");
    expect(errorDiv?.textContent?.length).toBeGreaterThan(0);
  });

  it("forgot password view — back to sign in button works", async () => {
    component = mount(Login, { target });
    await settle();

    const forgotBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Forgot password"),
    );
    forgotBtn.click();
    await settle();

    const backBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Back to sign in"),
    );
    expect(backBtn).not.toBeNull();
    backBtn.click();
    await settle();

    expect(target.textContent).toContain("Sign in");
  });
});
