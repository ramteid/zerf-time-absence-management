import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import Setup from "./Setup.svelte";
import { setLanguage } from "../i18n.js";

const apiMock = vi.hoisted(() => vi.fn());

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
}));

vi.mock("../passwordCredentials.js", () => ({
  storePasswordCredential: vi.fn().mockResolvedValue(false),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("Setup", () => {
  let target;
  let component;
  let appDiv;

  beforeEach(() => {
    appDiv = document.createElement("div");
    appDiv.id = "app";
    document.body.appendChild(appDiv);
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    apiMock.mockReset();
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    appDiv.remove();
  });

  function fillForm(opts = {}) {
    const firstName = target.querySelector("#setup-first-name");
    const lastName = target.querySelector("#setup-last-name");
    const email = target.querySelector("#setup-email");
    const password = target.querySelector("#setup-password");
    const confirm = target.querySelector("#setup-confirm");

    const setVal = (el, val) => {
      el.value = val;
      el.dispatchEvent(new Event("input"));
    };

    setVal(firstName, opts.firstName ?? "Alice");
    setVal(lastName, opts.lastName ?? "Admin");
    setVal(email, opts.email ?? "alice@example.com");
    setVal(password, opts.password ?? "SuperSecret123!");
    setVal(confirm, opts.confirm ?? opts.password ?? "SuperSecret123!");
  }

  it("renders the setup form", async () => {
    component = mount(Setup, { target });
    await settle();
    expect(target.textContent).toContain("Create admin account");
    expect(target.querySelector("#setup-first-name")).not.toBeNull();
    expect(target.querySelector("#setup-email")).not.toBeNull();
  });

  it("shows error when first name is empty", async () => {
    component = mount(Setup, { target });
    await settle();
    fillForm({ firstName: "" });
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    expect(target.querySelector(".error-text").textContent).toContain("first name");
  });

  it("shows error when email is invalid", async () => {
    component = mount(Setup, { target });
    await settle();
    fillForm({ email: "not-an-email" });
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    expect(target.querySelector(".error-text").textContent).toContain("email");
  });

  it("shows error when password is too short", async () => {
    component = mount(Setup, { target });
    await settle();
    fillForm({ password: "Short1!", confirm: "Short1!" });
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    expect(target.querySelector(".error-text").textContent).toContain("12 characters");
  });

  it("shows error when password lacks complexity", async () => {
    component = mount(Setup, { target });
    await settle();
    fillForm({ password: "alllowercase123", confirm: "alllowercase123" });
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    expect(target.querySelector(".error-text").textContent).toContain("3 of");
  });

  it("shows error when passwords do not match", async () => {
    component = mount(Setup, { target });
    await settle();
    fillForm({ password: "SuperSecret123!", confirm: "DifferentPass456#" });
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    expect(target.querySelector(".error-text").textContent).toContain("match");
  });

  it("calls onComplete after successful setup", async () => {
    apiMock.mockResolvedValueOnce({});
    const onComplete = vi.fn();
    component = mount(Setup, { target, props: { onComplete } });
    await settle();
    fillForm({});
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    await settle();
    expect(onComplete).toHaveBeenCalledWith("alice@example.com");
  });

  it("shows API error on failure", async () => {
    apiMock.mockRejectedValueOnce({ message: "Email already taken" });
    component = mount(Setup, { target });
    await settle();
    fillForm({});
    const form = target.querySelector("form");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await settle();
    await settle();
    expect(target.querySelector(".error-text").textContent).toContain("Email already taken");
  });
});
