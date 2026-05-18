import { describe, expect, it } from "vitest";
import { sortByIsoDateAndStartTime, yearsBetweenDates } from "./dates.js";

describe("date domain helpers", () => {
  it("sorts rows by normalized date key and start time", () => {
    expect(
      sortByIsoDateAndStartTime([
        { entry_date: "2026-01-02T00:00:00Z", start_time: "10:00:00" },
        { entry_date: "2026-01-01", start_time: "11:00:00" },
        { entry_date: "2026-01-01", start_time: "09:00:00" },
      ]),
    ).toEqual([
      { entry_date: "2026-01-01", start_time: "09:00:00" },
      { entry_date: "2026-01-01", start_time: "11:00:00" },
      { entry_date: "2026-01-02T00:00:00Z", start_time: "10:00:00" },
    ]);
  });

  it("returns inclusive year ranges across date boundaries", () => {
    expect(yearsBetweenDates("2025-12-31", "2026-01-01")).toEqual([2025, 2026]);
  });
});
