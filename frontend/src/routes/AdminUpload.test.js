import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminUpload from "./AdminUpload.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    backup_upload_enabled: false,
    backup_upload_url: "",
    backup_upload_password_set: false,
    backup_interval_days: 1,
    report_upload_enabled: true,
    report_upload_url: "https://cloud.example.com/s/tok123",
    report_upload_password_set: true,
    report_upload_day_of_month: 5,
  },
}));

const apiMock = vi.hoisted(() =>
  vi.fn(async (path, opts = {}) => {
    if (path === "/settings" && (!opts.method || opts.method === "GET")) {
      return mockState.settings;
    }
    if (path === "/settings/uploads" && opts.method === "PUT") {
      mockState.settings = { ...mockState.settings, ...opts.body };
      return mockState.settings;
    }
    if (
      path === "/settings/uploads/report/run-now" &&
      opts.method === "POST"
    ) {
      return {};
    }
    throw new Error(`Unhandled API path: ${path}`);
  }),
);

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

describe("AdminUpload", () => {
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

  it("renders enable backup upload checkbox reflecting loaded setting", async () => {
    mockState.settings = { ...mockState.settings, backup_upload_enabled: false };
    component = mount(AdminUpload, { target });
    await settle();

    const checkboxes = target.querySelectorAll('input[type="checkbox"]');
    const backupCb = [...checkboxes].find((cb) =>
      cb.closest("label")?.textContent?.includes("DB backup"),
    );
    expect(backupCb).not.toBeNull();
    expect(backupCb.checked).toBe(false);
  });

  it("renders enable report upload checkbox as checked when enabled", async () => {
    mockState.settings = { ...mockState.settings, report_upload_enabled: true };
    component = mount(AdminUpload, { target });
    await settle();

    const checkboxes = target.querySelectorAll('input[type="checkbox"]');
    const reportCb = [...checkboxes].find((cb) =>
      cb.closest("label")?.textContent?.includes("report PDF"),
    );
    expect(reportCb).not.toBeNull();
    expect(reportCb.checked).toBe(true);
  });

  it("includes all upload fields in the save body", async () => {
    component = mount(AdminUpload, { target });
    await settle();

    const saveBtn = [...target.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === "Save",
    );
    expect(saveBtn).not.toBeNull();
    saveBtn.click();
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        path === "/settings/uploads" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
    const body = saveCall[1].body;
    expect(body).toHaveProperty("backup_upload_enabled");
    expect(body).toHaveProperty("report_upload_enabled");
    expect(body).toHaveProperty("backup_interval_days");
    expect(body).toHaveProperty("report_upload_day_of_month");
  });

  it("shows stored indicator when report_upload_password_set is true", async () => {
    mockState.settings = {
      ...mockState.settings,
      report_upload_password_set: true,
    };
    component = mount(AdminUpload, { target });
    await settle();

    expect(target.textContent).toContain("stored");
  });

  it("calls run-now endpoint when Upload now is clicked", async () => {
    mockState.settings = { ...mockState.settings, report_upload_enabled: true };
    component = mount(AdminUpload, { target });
    await settle();

    const uploadBtn = [...target.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === "Upload now",
    );
    expect(uploadBtn).not.toBeNull();
    uploadBtn.click();
    await settle();

    const runCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        path === "/settings/uploads/report/run-now" && opts?.method === "POST",
    );
    expect(runCall).toBeTruthy();
  });
});
