import { describe, expect, it } from "vitest";
import { periodBounds, timeQueryRange, leaveYearForPeriod } from "./reportPeriod.js";

describe("periodBounds", () => {
  it("expands a month into its first/last day", () => {
    expect(periodBounds({ mode: "month", month: "2026-02" })).toEqual({
      from: "2026-02-01",
      to: "2026-02-28",
    });
  });

  it("passes a custom range through unchanged", () => {
    expect(
      periodBounds({ mode: "range", from: "2026-01-10", to: "2026-03-05" }),
    ).toEqual({ from: "2026-01-10", to: "2026-03-05" });
  });
});

describe("timeQueryRange", () => {
  it("leaves a fully-past range untouched and marks it active", () => {
    const result = timeQueryRange(
      { mode: "range", from: "2026-01-01", to: "2026-01-31" },
      "2026-06-15",
    );
    expect(result).toEqual({
      from: "2026-01-01",
      to: "2026-01-31",
      active: true,
    });
  });

  it("caps `to` at today for a range straddling the present", () => {
    const result = timeQueryRange(
      { mode: "range", from: "2026-06-01", to: "2026-06-30" },
      "2026-06-15",
    );
    expect(result).toEqual({
      from: "2026-06-01",
      to: "2026-06-15",
      active: true,
    });
  });

  it("marks a fully-future range inactive without swapping from/to", () => {
    const result = timeQueryRange(
      { mode: "range", from: "2026-07-01", to: "2026-07-31" },
      "2026-06-15",
    );
    expect(result.active).toBe(false);
    // `from` must stay the real start date — callers must check `active`
    // before using `from`/`to` together, since `to` alone is capped and can
    // end up earlier than `from` for a fully-future range.
    expect(result.from).toBe("2026-07-01");
    expect(result.to).toBe("2026-06-15");
  });

  it("treats a range ending exactly today as active", () => {
    const result = timeQueryRange(
      { mode: "month", month: "2026-06" },
      "2026-06-15",
    );
    expect(result.active).toBe(true);
    expect(result.to).toBe("2026-06-15");
  });
});

describe("leaveYearForPeriod", () => {
  it("returns the month's year in month mode", () => {
    expect(leaveYearForPeriod({ mode: "month", month: "2026-03" })).toBe(
      "2026",
    );
  });

  it("returns the shared year for a single-year custom range", () => {
    expect(
      leaveYearForPeriod({
        mode: "range",
        from: "2026-01-05",
        to: "2026-11-20",
      }),
    ).toBe("2026");
  });

  it("returns null for a custom range spanning more than one year", () => {
    expect(
      leaveYearForPeriod({
        mode: "range",
        from: "2025-12-01",
        to: "2026-01-31",
      }),
    ).toBeNull();
  });
});
