import { describe, expect, it } from "vitest";
import {
  daysBetweenIsoDates,
  isReportRangeTooLong,
  sortByIsoDateAndStartTime,
  yearsBetweenDates,
} from "./dates.js";

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

  it("calculates report range spans for the 366-day backend limit", () => {
    expect(daysBetweenIsoDates("2026-01-01", "2027-01-02")).toBe(366);
    expect(isReportRangeTooLong("2026-01-01", "2027-01-02")).toBe(false);
    expect(isReportRangeTooLong("2026-01-01", "2027-01-03")).toBe(true);
    expect(isReportRangeTooLong("bad", "2027-01-03")).toBe(false);
  });
});
