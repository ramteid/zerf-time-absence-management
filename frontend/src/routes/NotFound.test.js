import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import NotFound from "./NotFound.svelte";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("NotFound", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders 404 status code", async () => {
    component = mount(NotFound, { target });
    await settle();
    expect(target.textContent).toContain("404");
  });

  it("renders page not found message", async () => {
    component = mount(NotFound, { target });
    await settle();
    expect(target.textContent).toContain("Page not found");
  });
});
