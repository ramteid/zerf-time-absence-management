import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminCategories from "./AdminCategories.svelte";
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

describe("AdminCategories", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    apiMock.mockReset();

    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
  });

  it("renders the Time Categories heading", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminCategories, { target });
    await waitForText(target, "Time Categories");
  });

  it("renders a list of categories", async () => {
    apiMock.mockResolvedValue([
      { id: 1, name: "Core Duties", color: "#4f87c7", active: true },
      { id: 2, name: "Flextime", color: "#a8d8a8", active: true },
    ]);
    component = mount(AdminCategories, { target });
    await waitForText(target, "Core Duties");
    await waitForText(target, "Flextime");
  });

  it("renders inactive badge for inactive categories", async () => {
    apiMock.mockResolvedValue([
      { id: 3, name: "Old Category", color: "#ccc", active: false },
    ]);
    component = mount(AdminCategories, { target });
    await waitForText(target, "Inactive");
  });

  it("opens dialog when Add Category button is clicked", async () => {
    apiMock.mockResolvedValue([]);
    component = mount(AdminCategories, { target });
    await waitForText(target, "Time Categories");

    const addBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Add Category")
    );
    expect(addBtn).not.toBeNull();
    addBtn.click();
    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog?.hasAttribute("open")).toBe(true);
  });

  it("opens dialog with category data when edit button is clicked", async () => {
    apiMock.mockResolvedValue([
      { id: 1, name: "Core Duties", color: "#4f87c7", active: true },
    ]);
    component = mount(AdminCategories, { target });
    await waitForText(target, "Core Duties");

    const editBtn = [...target.querySelectorAll("button")].find((b) =>
      b.querySelector("svg")
    );
    expect(editBtn).not.toBeNull();
    editBtn.click();
    await settle();

    const dialog = target.querySelector("dialog");
    expect(dialog?.hasAttribute("open")).toBe(true);
  });
});
