import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  translate,
  resolveLanguage,
  DEFAULT_LANGUAGE,
  LANGUAGES,
  setLanguage,
  getLanguage,
  getLocale,
  roleLabel,
  statusLabel,
  absenceKindLabel,
  t,
} from "./i18n.js";

beforeEach(() => {
  setLanguage("en");
});

describe("translate", () => {
  it("returns the key when no translation exists", () => {
    expect(translate("en", "Unknown key")).toBe("Unknown key");
  });

  it("returns German translation", () => {
    expect(translate("de", "Sign in")).toBe("Anmelden");
  });

  it("falls back to key for missing German translation", () => {
    expect(translate("de", "nonexistent.key")).toBe("nonexistent.key");
  });

  it("interpolates parameters", () => {
    expect(translate("en", "Week {week}", { week: 12 })).toBe("Week 12");
  });

  it("interpolates multiple parameters", () => {
    expect(
      translate("de", "Reason: {reason}", { reason: "Arzttermin" }),
    ).toBe("Begründung: Arzttermin");
  });

  it("keeps placeholder when param is missing", () => {
    expect(translate("en", "Week {week}", {})).toBe("Week {week}");
  });
});

describe("resolveLanguage", () => {
  it("returns known language as-is", () => {
    expect(resolveLanguage("de")).toBe("de");
    expect(resolveLanguage("en")).toBe("en");
  });

  it("falls back to default for unknown languages", () => {
    expect(resolveLanguage("xx")).toBe(DEFAULT_LANGUAGE);
    expect(resolveLanguage("")).toBe(DEFAULT_LANGUAGE);
    expect(resolveLanguage(null)).toBe(DEFAULT_LANGUAGE);
  });
});

describe("LANGUAGES", () => {
  it("lists supported locales", () => {
    expect(Object.keys(LANGUAGES)).toEqual(["en", "de"]);
  });

  it("each language has a label and locale", () => {
    for (const lang of Object.values(LANGUAGES)) {
      expect(lang).toHaveProperty("label");
      expect(lang).toHaveProperty("locale");
    }
  });
});

describe("language store and helpers", () => {
  it("setLanguage updates the store", () => {
    setLanguage("de");
    expect(getLanguage()).toBe("de");
  });

  it("setLanguage rejects invalid languages", () => {
    setLanguage("xx");
    expect(getLanguage()).toBe(DEFAULT_LANGUAGE);
  });

  it("getLocale returns locale string", () => {
    setLanguage("en");
    expect(getLocale()).toBe("en-US");
    setLanguage("de");
    expect(getLocale()).toBe("de-DE");
  });

  it("t derived store produces a translate function", () => {
    setLanguage("de");
    const fn = get(t);
    expect(fn("Sign in")).toBe("Anmelden");
  });
});

describe("label helpers", () => {
  it("roleLabel translates known roles", () => {
    setLanguage("de");
    expect(roleLabel("admin")).toBe("Admin");
    expect(roleLabel("employee")).toBe("Mitarbeitende");
  });

  it("roleLabel falls back for unknown roles", () => {
    expect(roleLabel("unknown_role")).toBe("unknown_role");
  });

  it("statusLabel translates known statuses", () => {
    setLanguage("de");
    expect(statusLabel("draft")).toBe("Entwurf");
    expect(statusLabel("approved")).toBe("Genehmigt");
  });

  it("statusLabel falls back for unknown statuses", () => {
    expect(statusLabel("weird")).toBe("weird");
  });

  it("absenceKindLabel translates known kinds", () => {
    setLanguage("de");
    expect(absenceKindLabel("vacation")).toBe("Urlaub");
    expect(absenceKindLabel("sick")).toBe("Krank");
  });

  it("absenceKindLabel falls back for unknown kinds", () => {
    expect(absenceKindLabel("other")).toBe("other");
  });
});
