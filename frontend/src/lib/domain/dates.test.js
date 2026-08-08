import { describe, expect, it } from "vitest";
import {
  daysBetweenIsoDates,
  isReportRangeTooLong,
  monthStart,
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

  it("matches the backend's 366-day-inclusive report range limit", () => {
    // The backend caps an *inclusive* range at 366 days, i.e. a difference of
    // 365. Anything past that is rejected by the API, so the guard must not
    // wave it through.
    expect(daysBetweenIsoDates("2026-01-01", "2027-01-01")).toBe(365);
    expect(isReportRangeTooLong("2026-01-01", "2027-01-01")).toBe(false);
    expect(isReportRangeTooLong("2026-01-01", "2027-01-02")).toBe(true);
  });

  it("treats an unmeasurable range as too long so callers never fan out on it", () => {
    // Callers gate per-year request fan-out on this. A bound that can't even
    // be parsed must fail closed — treating it as "short enough" is what let
    // a blank period expand into thousands of requests.
    expect(isReportRangeTooLong("bad", "2027-01-03")).toBe(true);
    expect(isReportRangeTooLong("", "")).toBe(true);
  });

  it("refuses to expand a blank or nonsensical period into a year list", () => {
    // Regression: monthStart("") used to yield "-01", which JS parses as
    // 2001-01-01, and monthEnd("") yields "" → year 0. The resulting span was
    // ~2000 years, one API request each.
    expect(yearsBetweenDates("-01", "")).toEqual([]);
    expect(yearsBetweenDates("", "")).toEqual([]);
    expect(yearsBetweenDates("1926-01-01", "2026-01-01")).toEqual([]);
  });

  it("builds an empty month start for a blank or malformed month", () => {
    expect(monthStart("")).toBe("");
    expect(monthStart(undefined)).toBe("");
    expect(monthStart("2026-08")).toBe("2026-08-01");
  });
});
