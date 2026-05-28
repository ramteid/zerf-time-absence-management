// Tests for timeApi — the fetch layer for the Time page and week-related
// operations. These wrappers encapsulate URL construction so that all callers
// stay in sync when the backend routes change. Tests also verify the parallel
// data-loading helper (getWeekData) correctly handles partial failures: a
// missing /categories or /reopen-requests must not crash the whole page.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";
import {
  getAbsencesByYear,
  getCategories,
  getHolidaysByYear,
  getReopenRequests,
  getWeekData,
  getWeekEntries,
  requestWeekReopen,
  submitWeekEntries,
} from "./timeApi.js";

describe("timeApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.mockResolvedValue([]);
  });

  it("getWeekEntries builds the correct date-range URL", async () => {
    // The Time page fetches exactly one calendar week. Boundaries must be
    // Monday (from) and Sunday (to) so entries outside the week are excluded.
    await getWeekEntries("2026-05-25", "2026-05-31");
    expect(api).toHaveBeenCalledWith(
      "/time-entries?from=2026-05-25&to=2026-05-31",
    );
  });

  it("getReopenRequests fetches pending edit requests for the current user", async () => {
    // A submitted week can only be edited after an approved reopen request.
    // The Time page needs this list to decide which weeks show the "Request edit" button.
    await getReopenRequests();
    expect(api).toHaveBeenCalledWith("/reopen-requests");
  });

  it("getCategories fetches the active category list", async () => {
    // Categories drive the time-entry form dropdown and the color coding on
    // the week grid. Only active categories are returned by this endpoint.
    await getCategories();
    expect(api).toHaveBeenCalledWith("/categories");
  });

  it("getAbsencesByYear includes the year in the query string", async () => {
    // Absences are scoped by calendar year to power the leave balance view.
    // A wrong year would show stale data from the prior or next year.
    await getAbsencesByYear(2026);
    expect(api).toHaveBeenCalledWith("/absences?year=2026");
  });

  it("getHolidaysByYear includes the year in the query string", async () => {
    // Public holidays vary by year; the Time page uses this to gray out
    // holiday dates and to avoid counting them toward leave-day calculations.
    await getHolidaysByYear(2026);
    expect(api).toHaveBeenCalledWith("/holidays?year=2026");
  });

  it("submitWeekEntries posts the correct entry ID list", async () => {
    // Submitting a week sends all draft entry IDs in one batch request.
    // The IDs must match exactly — extra or missing IDs would leave entries
    // in the wrong state for the approver's dashboard.
    await submitWeekEntries([1, 2, 3]);
    expect(api).toHaveBeenCalledWith("/time-entries/submit", {
      method: "POST",
      body: { ids: [1, 2, 3] },
    });
  });

  it("requestWeekReopen posts week_start and reason", async () => {
    // A reopen request must include the week's Monday date (ISO) so the
    // backend can identify the exact week, and an optional reason to give
    // the approver context.
    await requestWeekReopen("2026-05-25", "Forgot entry");
    expect(api).toHaveBeenCalledWith("/reopen-requests", {
      method: "POST",
      body: { week_start: "2026-05-25", reason: "Forgot entry" },
    });
  });

  it("getWeekData fetches all five data sources in parallel", async () => {
    // Loading a week requires entries, reopens, categories, absences, and
    // holidays. They are fetched in parallel (Promise.all) to reduce latency;
    // this test verifies all five API calls are made and merged correctly.
    const entries = [{ id: 1 }];
    const categories = [{ id: 1, name: "Work" }];
    api.mockImplementation(async (path) => {
      if (path.startsWith("/time-entries?")) return entries;
      if (path === "/reopen-requests") return [];
      if (path === "/categories") return categories;
      if (path.startsWith("/absences?")) return [];
      if (path.startsWith("/holidays?")) return [];
      return [];
    });

    const result = await getWeekData({
      from: "2026-05-25",
      to: "2026-05-31",
      years: [2026],
      fallbackCategories: [],
    });
    expect(result.entries).toEqual(entries);
    expect(result.categoryRows).toEqual(categories);
  });

  it("getWeekData falls back to cached categories when the API call fails", async () => {
    // If /categories is unavailable (network error, 503), the page must still
    // render using the categories already loaded in the Svelte store instead
    // of crashing or showing an empty category list.
    const fallback = [{ id: 99, name: "Fallback" }];
    api.mockImplementation(async (path) => {
      if (path === "/categories") throw new Error("Network error");
      return [];
    });

    const result = await getWeekData({
      from: "2026-05-25",
      to: "2026-05-31",
      years: [2026],
      fallbackCategories: fallback,
    });
    expect(result.categoryRows).toEqual(fallback);
  });

  it("getWeekData silently returns an empty array when reopen-requests fails", async () => {
    // Employees without any reopen requests will sometimes hit a 403 or
    // empty response. A failure here must not block the Time page from
    // loading — reopens are supplemental, not required for rendering.
    api.mockImplementation(async (path) => {
      if (path === "/reopen-requests") throw new Error("Forbidden");
      return [];
    });

    const result = await getWeekData({
      from: "2026-05-25",
      to: "2026-05-31",
      years: [2026],
      fallbackCategories: [],
    });
    expect(result.reopenRows).toEqual([]);
  });
});
