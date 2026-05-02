import { describe, it, expect } from "vitest";
import {
  isoDate,
  monday,
  addDays,
  minToHM,
  durMin,
  isoWeek,
} from "./format.js";

describe("format helpers", () => {
  it("isoDate yields YYYY-MM-DD", () => {
    expect(isoDate(new Date(2024, 0, 5))).toBe("2024-01-05");
  });
  it("monday() rounds to ISO Monday", () => {
    // 2024-03-13 is a Wednesday → Monday is 2024-03-11
    expect(isoDate(monday(new Date(2024, 2, 13)))).toBe("2024-03-11");
  });
  it("addDays adds days", () => {
    expect(isoDate(addDays(new Date(2024, 0, 1), 5))).toBe("2024-01-06");
  });
  it("minToHM formats minutes", () => {
    expect(minToHM(0)).toBe("0:00");
    expect(minToHM(125)).toBe("2:05");
    expect(minToHM(-30)).toBe("-0:30");
  });
  it("durMin computes duration", () => {
    expect(durMin("08:00", "12:30")).toBe(270);
  });
  it("isoWeek returns plausible ISO week numbers", () => {
    expect(isoWeek(new Date(2024, 0, 1))).toBe(1);
    expect(isoWeek(new Date(2024, 11, 30))).toBe(1);
  });
});
