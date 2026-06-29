import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const apiMock = vi.hoisted(() => vi.fn());

vi.mock("./api.js", () => ({
  api: apiMock,
}));

import { loadPostAuthData } from "./appData.js";
import {
  categories,
  absenceCategories,
  earliestStartDate,
  toasts,
} from "./stores.js";
import { setLanguage } from "./i18n.js";

describe("loadPostAuthData", () => {
  beforeEach(() => {
    setLanguage("en");
    // All boot-loaded stores start unset so each fetch path is exercised.
    categories.set([]);
    absenceCategories.set([]);
    earliestStartDate.set(null);
    toasts.set([]);
    apiMock.mockReset();
  });

  afterEach(() => {
    toasts.set([]);
  });

  it("fetches all three datasets when the stores are unset", async () => {
    apiMock.mockImplementation((path) => {
      if (path === "/users/earliest-start-date") {
        return Promise.resolve({ earliest_start_date: "2022-01-01" });
      }
      return Promise.resolve([{ id: 1 }]);
    });

    await loadPostAuthData();

    expect(apiMock).toHaveBeenCalledWith("/users/earliest-start-date");
    expect(apiMock).toHaveBeenCalledWith("/categories");
    expect(apiMock).toHaveBeenCalledWith("/absence-categories");
    expect(get(earliestStartDate)).toBe("2022-01-01");
    expect(get(categories)).toEqual([{ id: 1 }]);
    expect(get(absenceCategories)).toEqual([{ id: 1 }]);
  });

  it("skips datasets that are already populated and issues no requests", async () => {
    categories.set([{ id: 9 }]);
    absenceCategories.set([{ id: 8 }]);
    earliestStartDate.set("2020-05-05");
    apiMock.mockResolvedValue([]);

    await loadPostAuthData();

    expect(apiMock).not.toHaveBeenCalled();
    // Pre-existing values must be left untouched (never clobbered).
    expect(get(categories)).toEqual([{ id: 9 }]);
    expect(get(absenceCategories)).toEqual([{ id: 8 }]);
    expect(get(earliestStartDate)).toBe("2020-05-05");
  });

  it("toasts on a categories failure but still loads absence categories", async () => {
    apiMock.mockImplementation((path) => {
      if (path === "/users/earliest-start-date") {
        return Promise.resolve({ earliest_start_date: null });
      }
      if (path === "/categories") {
        return Promise.reject(new Error("boom"));
      }
      return Promise.resolve([{ id: 2 }]);
    });

    await loadPostAuthData();

    // Failed categories load leaves the store empty but does not throw...
    expect(get(categories)).toEqual([]);
    // ...and must not abort the subsequent absence-category load.
    expect(get(absenceCategories)).toEqual([{ id: 2 }]);
    const activeToasts = get(toasts);
    expect(activeToasts.length).toBe(1);
    expect(activeToasts[0].type).toBe("error");
  });

  it("treats a missing earliest start date as null without throwing", async () => {
    apiMock.mockImplementation((path) => {
      if (path === "/users/earliest-start-date") {
        return Promise.resolve({}); // response without the key
      }
      return Promise.resolve([]);
    });

    await loadPostAuthData();

    expect(get(earliestStartDate)).toBe(null);
  });
});
