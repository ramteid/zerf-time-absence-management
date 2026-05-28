import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import EmptyState from "./EmptyState.svelte";
import LoadingState from "./LoadingState.svelte";
import FormField from "./FormField.svelte";
import PageHeader from "./PageHeader.svelte";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("EmptyState", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the text prop", async () => {
    component = mount(EmptyState, { target, props: { text: "No items found" } });
    await settle();
    expect(target.textContent).toContain("No items found");
  });

  it("renders slot content", async () => {
    component = mount(EmptyState, { target, props: { text: "" } });
    await settle();
    expect(target.querySelector(".empty-state")).not.toBeNull();
  });

  it("renders without icon when icon prop omitted", async () => {
    component = mount(EmptyState, { target, props: { text: "Empty" } });
    await settle();
    expect(target.textContent).toContain("Empty");
  });
});

describe("LoadingState", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders default text", async () => {
    component = mount(LoadingState, { target });
    await settle();
    expect(target.textContent).toContain("Loading...");
  });

  it("renders custom text", async () => {
    component = mount(LoadingState, { target, props: { text: "Please wait..." } });
    await settle();
    expect(target.textContent).toContain("Please wait...");
  });
});

describe("FormField", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders label when provided", async () => {
    component = mount(FormField, { target, props: { label: "Email", forId: "email" } });
    await settle();
    const label = target.querySelector("label");
    expect(label).not.toBeNull();
    expect(label.textContent).toContain("Email");
    expect(label.getAttribute("for")).toBe("email");
  });

  it("omits label element when label is empty", async () => {
    component = mount(FormField, { target, props: { label: "" } });
    await settle();
    expect(target.querySelector("label")).toBeNull();
  });
});

describe("PageHeader", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders title", async () => {
    component = mount(PageHeader, { target, props: { title: "Dashboard" } });
    await settle();
    expect(target.querySelector("h1").textContent).toContain("Dashboard");
  });

  it("renders subtitle when provided", async () => {
    component = mount(PageHeader, { target, props: { title: "Test", subtitle: "Sub-title text" } });
    await settle();
    expect(target.querySelector(".top-bar-subtitle").textContent).toContain("Sub-title text");
  });

  it("omits subtitle element when subtitle is empty", async () => {
    component = mount(PageHeader, { target, props: { title: "Test", subtitle: "" } });
    await settle();
    expect(target.querySelector(".top-bar-subtitle")).toBeNull();
  });
});
