import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminSettings from "./AdminSettings.svelte";
import { currentUser, settings as appSettings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    ui_language: "en",
    time_format: "24h",
    country: "DE",
    region: "",
    default_weekly_hours: 40,
    default_annual_leave_days: 30,
    carryover_expiry_date: "03-31",
    submission_deadline_day: null,
  },
  countries: [
    { countryCode: "DE", name: "Germany" },
    { countryCode: "US", name: "United States" },
  ],
  regionsByCountry: {
    DE: ["DE-BW", "DE-BY"],
    US: ["US-CA"],
  },
}));

const apiMock = vi.hoisted(() => vi.fn(async (path, opts = {}) => {
  if (path === "/settings" && (!opts.method || opts.method === "GET")) {
    return mockState.settings;
  }
  if (path === "/holidays/countries") {
    return mockState.countries;
  }
  if (path.startsWith("/holidays/regions/")) {
    const country = path.split("/").at(-1);
    return mockState.regionsByCountry[country] || [];
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

describe("AdminSettings", () => {
  let target;
  let component;
  let originalRegionsByCountry;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    originalRegionsByCountry = structuredClone(mockState.regionsByCountry);
    currentUser.set({
      id: 1,
      first_name: "Admin",
      last_name: "User",
      must_configure_settings: true,
    });
    appSettings.set({ ui_language: "en", time_format: "24h" });
    setLanguage("en");
    apiMock.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    mockState.regionsByCountry = originalRegionsByCountry;
    target.remove();
  });

  it("loads region options on initial render when the country is already set", async () => {
    component = mount(AdminSettings, { target });
    await settle();
    await settle();

    expect(apiMock).toHaveBeenCalledWith("/holidays/regions/DE");

    const regionSelect = target.querySelector("#settings-region");
    expect(regionSelect).not.toBeNull();
    expect(regionSelect.tagName).toBe("SELECT");

    const optionValues = [...regionSelect.querySelectorAll("option")].map(
      (option) => option.value,
    );
    expect(optionValues).toContain("DE-BW");
    expect(optionValues).toContain("DE-BY");
  });

  it("keeps the region field API-driven when no regions are returned", async () => {
    mockState.regionsByCountry = { ...mockState.regionsByCountry, DE: [] };

    component = mount(AdminSettings, { target });
    await settle();
    await settle();

    const regionField = target.querySelector("#settings-region");
    expect(regionField).not.toBeNull();
    expect(regionField.tagName).toBe("SELECT");
    expect(regionField.disabled).toBe(true);
    expect(target.querySelector("input#settings-region")).toBeNull();
  });
});