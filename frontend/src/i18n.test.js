import { describe, it, expect } from "vitest";
import {
  translate,
  resolveLanguage,
  DEFAULT_LANGUAGE,
  LANGUAGES,
} from "./i18n.js";

describe("i18n", () => {
  it("falls back to the key for unknown translations", () => {
    expect(translate("en", "Unknown key")).toBe("Unknown key");
  });
  it("interpolates parameters", () => {
    expect(translate("en", "Week {week}", { week: 12 })).toBe("Week 12");
  });
  it("returns German translation when available", () => {
    expect(translate("de", "Sign in")).toBe("Anmelden");
  });
  it("falls back to English when German is missing", () => {
    expect(translate("de", "Some unknown phrase")).toBe("Some unknown phrase");
  });
  it("resolveLanguage rejects unknown languages", () => {
    expect(resolveLanguage("xx")).toBe(DEFAULT_LANGUAGE);
    expect(resolveLanguage("de")).toBe("de");
  });
  it("LANGUAGES lists supported locales", () => {
    expect(Object.keys(LANGUAGES)).toEqual(["en", "de"]);
  });
});
