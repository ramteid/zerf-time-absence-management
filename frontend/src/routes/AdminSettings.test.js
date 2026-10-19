import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminSettings from "./AdminSettings.svelte";
import { currentUser, settings as appSettings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    ui_language: "en",
    time_format: "24h",
    timezone: "Europe/Berlin",
    country: "DE",
    region: "",
    default_weekly_hours: 40,
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

const apiMock = vi.hoisted(() =>
  vi.fn(async (path, opts = {}) => {
    if (path === "/settings" && (!opts.method || opts.method === "GET")) {
      return mockState.settings;
    }
    if (path === "/settings" && opts.method === "PUT") {
      mockState.settings = structuredClone(opts.body);
      return mockState.settings;
    }
    if (path === "/holidays/countries") {
      return mockState.countries;
    }
    if (path.startsWith("/holidays/regions/")) {
      const country = path.split("/").at(-1);
      const result = mockState.regionsByCountry[country];
      if (result instanceof Error) {
        throw result;
      }
      return result || [];
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

describe("AdminSettings", () => {
  let target;
  let component;
  let originalSettings;
  let originalRegionsByCountry;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    originalSettings = structuredClone(mockState.settings);
    originalRegionsByCountry = structuredClone(mockState.regionsByCountry);
    currentUser.set({
      id: 1,
      first_name: "Admin",
      last_name: "User",
      must_configure_settings: true,
    });
    appSettings.set({
      ui_language: "en",
      time_format: "24h",
      timezone: "Europe/Berlin",
    });
    setLanguage("en");
    apiMock.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    mockState.settings = originalSettings;
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

  it("allows saving without selecting a region", async () => {
    component = mount(AdminSettings, { target });
    await settle();
    await settle();

    target.querySelector(".zf-btn-primary").click();
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/settings" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
    expect(saveCall[1].body.region).toBe("");
    expect(saveCall[1].body).not.toHaveProperty("default_annual_leave_days");
    expect(saveCall[1].body).not.toHaveProperty("carryover_expiry_date");
  });

  it("allows saving even when region loading fails", async () => {
    mockState.regionsByCountry = {
      ...mockState.regionsByCountry,
      DE: new Error("boom"),
    };

    component = mount(AdminSettings, { target });
    await settle();
    await settle();

    target.querySelector(".zf-btn-primary").click();
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) => path === "/settings" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
  });

  describe("forbidding too much work in a row", () => {
    const breakDeductionLabel = "Enable automatic break deduction";
    const forbidLabel = "Forbid entries with too long work time in a row";

    function withBreakDeduction(overrides = {}) {
      mockState.settings = {
        ...mockState.settings,
        auto_break_enabled: true,
        auto_break_threshold_hours: 6,
        auto_break_deduction_minutes: 30,
        auto_break_threshold_hours_2: null,
        auto_break_deduction_minutes_2: null,
        auto_break_forbid_long_stretch: false,
        auto_break_min_rest_minutes: 15,
        ...overrides,
      };
    }

    function minimumBreakInput() {
      return target.querySelector("#settings-break-min-rest");
    }

    function enter(input, value) {
      input.value = value;
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }

    function saveWasSent() {
      return apiMock.mock.calls.some(
        ([path, opts]) => path === "/settings" && opts?.method === "PUT",
      );
    }

    function checkboxLabelled(text) {
      const label = [...target.querySelectorAll("label")].find((element) =>
        element.textContent.includes(text),
      );
      return label?.querySelector("input[type='checkbox']") ?? null;
    }

    function toggle(checkbox) {
      checkbox.checked = !checkbox.checked;
      checkbox.dispatchEvent(new Event("change", { bubbles: true }));
    }

    function savedBody() {
      const saveCall = apiMock.mock.calls.find(
        ([path, opts]) => path === "/settings" && opts?.method === "PUT",
      );
      expect(saveCall).toBeTruthy();
      return saveCall[1].body;
    }

    async function mountAndSave(change) {
      component = mount(AdminSettings, { target });
      await settle();
      await settle();
      change();
      await settle();
      target.querySelector(".zf-btn-primary").click();
      await settle();
      return savedBody();
    }

    it("offers the option only while the automatic break deduction is on", async () => {
      component = mount(AdminSettings, { target });
      await settle();
      await settle();
      expect(checkboxLabelled(forbidLabel)).toBeNull();

      toggle(checkboxLabelled(breakDeductionLabel));
      await settle();
      expect(checkboxLabelled(forbidLabel)).not.toBeNull();
    });

    it("saves the option as chosen", async () => {
      withBreakDeduction();
      const body = await mountAndSave(() =>
        toggle(checkboxLabelled(forbidLabel)),
      );

      expect(body.auto_break_enabled).toBe(true);
      expect(body.auto_break_forbid_long_stretch).toBe(true);
    });

    it("shows the stored choice", async () => {
      withBreakDeduction({ auto_break_forbid_long_stretch: true });
      component = mount(AdminSettings, { target });
      await settle();
      await settle();

      expect(checkboxLabelled(forbidLabel).checked).toBe(true);
    });

    it("clears the option when the automatic break deduction is switched off", async () => {
      withBreakDeduction({ auto_break_forbid_long_stretch: true });
      const body = await mountAndSave(() =>
        toggle(checkboxLabelled(breakDeductionLabel)),
      );

      expect(body.auto_break_enabled).toBe(false);
      expect(body.auto_break_forbid_long_stretch).toBe(false);
    });

    it("does not bring the option back when the deduction is switched off and on again", async () => {
      withBreakDeduction({ auto_break_forbid_long_stretch: true });
      component = mount(AdminSettings, { target });
      await settle();
      await settle();

      toggle(checkboxLabelled(breakDeductionLabel));
      await settle();
      toggle(checkboxLabelled(breakDeductionLabel));
      await settle();

      expect(checkboxLabelled(forbidLabel).checked).toBe(false);
    });

    it("asks for the minimum break length only while the option is on", async () => {
      withBreakDeduction({ auto_break_forbid_long_stretch: false });
      component = mount(AdminSettings, { target });
      await settle();
      await settle();
      expect(minimumBreakInput()).toBeNull();

      toggle(checkboxLabelled(forbidLabel));
      await settle();
      expect(minimumBreakInput()).not.toBeNull();
    });

    it("shows the stored minimum break length", async () => {
      withBreakDeduction({
        auto_break_forbid_long_stretch: true,
        auto_break_min_rest_minutes: 20,
      });
      component = mount(AdminSettings, { target });
      await settle();
      await settle();

      expect(minimumBreakInput().value).toBe("20");
    });

    it("saves the minimum break length as entered", async () => {
      withBreakDeduction({ auto_break_forbid_long_stretch: true });
      const body = await mountAndSave(() => enter(minimumBreakInput(), "10"));

      expect(body.auto_break_forbid_long_stretch).toBe(true);
      expect(body.auto_break_min_rest_minutes).toBe(10);
    });

    it("does not save an empty or unusable minimum break length", async () => {
      withBreakDeduction({ auto_break_forbid_long_stretch: true });
      component = mount(AdminSettings, { target });
      await settle();
      await settle();

      for (const unusable of ["", "0", "481", "12.5"]) {
        enter(minimumBreakInput(), unusable);
        await settle();
        target.querySelector(".zf-btn-primary").click();
        await settle();
        expect(saveWasSent(), `"${unusable}" must not be sent`).toBe(false);
      }
    });

    it("leaves the minimum break length out of a save while the option is off", async () => {
      withBreakDeduction({
        auto_break_forbid_long_stretch: false,
        auto_break_min_rest_minutes: 20,
      });
      const body = await mountAndSave(() => {});

      expect(body.auto_break_forbid_long_stretch).toBe(false);
      expect(body.auto_break_min_rest_minutes).toBeUndefined();
    });
  });
});
