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
  auditActionLabel,
  auditTableLabel,
  formatHours,
  fmtDecimal,
  formatDayCount,
  localizeErrorMessage,
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
    expect(translate("de", "Reason: {reason}", { reason: "Arzttermin" })).toBe(
      "Begründung: Arzttermin",
    );
  });

  it("keeps placeholder when param is missing", () => {
    expect(translate("en", "Week {week}", {})).toBe("Week {week}");
  });

  it("localizes direct error messages", () => {
    setLanguage("de");
    expect(localizeErrorMessage("Invalid email or password.")).toBe(
      "Ungültige E-Mail-Adresse oder ungültiges Passwort.",
    );
  });

  it("localizes parameterized error messages", () => {
    setLanguage("de");
    expect(
      localizeErrorMessage(
        "Cannot deactivate: 2 active user(s) still have this person as their approver. Reassign them first.",
      ),
    ).toBe(
      "Deaktivierung nicht möglich: 2 aktive Benutzer haben diese Person noch als verantwortliche Person. Weisen Sie sie zuerst neu zu.",
    );
  });

  it("localizes duplicate user conflicts", () => {
    setLanguage("de");
    expect(localizeErrorMessage("Conflict: Email already exists.")).toBe(
      "Konflikt: E-Mail existiert bereits.",
    );
    expect(
      localizeErrorMessage("Conflict: First name and last name already exist."),
    ).toBe(
      "Konflikt: Diese Kombination aus Vorname und Nachname existiert bereits.",
    );
  });

  it("localizes workday-required absence validation error", () => {
    setLanguage("de");
    expect(localizeErrorMessage("Absence must include at least one workday.")).toBe(
      "Die Abwesenheit muss mindestens einen Arbeitstag enthalten.",
    );
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
    expect(roleLabel("assistant")).toBe("Aushilfe");
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

  it("formats localized hour units", () => {
    setLanguage("en");
    expect(formatHours("39")).toBe("39h");
    setLanguage("de");
    expect(formatHours("39")).toBe("39 Std.");
  });

  it("formatHours formats numbers with locale-aware decimal", () => {
    setLanguage("en");
    expect(formatHours(5.5)).toBe("5.5h");
    expect(formatHours(5)).toBe("5h");
    setLanguage("de");
    expect(formatHours(5.5)).toBe("5,5 Std.");
    expect(formatHours(5)).toBe("5 Std.");
  });

  it("fmtDecimal formats decimals with locale separator", () => {
    setLanguage("en");
    expect(fmtDecimal(1.5, 1)).toBe("1.5");
    expect(fmtDecimal(2, 1)).toBe("2.0");
    setLanguage("de");
    expect(fmtDecimal(1.5, 1)).toBe("1,5");
    expect(fmtDecimal(2, 1)).toBe("2,0");
  });

  it("fmtDecimal respects fractionDigits=0", () => {
    setLanguage("en");
    expect(fmtDecimal(3, 0)).toBe("3");
    setLanguage("de");
    expect(fmtDecimal(3, 0)).toBe("3");
  });

  it("formatDayCount formats integer and half-day values by locale", () => {
    setLanguage("en");
    expect(formatDayCount(2)).toBe("2");
    expect(formatDayCount(2.5)).toBe("2.5");
    setLanguage("de");
    expect(formatDayCount(2)).toBe("2");
    expect(formatDayCount(2.5)).toBe("2,5");
  });

  it("formatDayCount keeps non-number values unchanged", () => {
    expect(formatDayCount("-")).toBe("-");
    expect(formatDayCount(null)).toBe(null);
    expect(formatDayCount(undefined)).toBe(undefined);
  });

  it("translates audit aliases and actions", () => {
    setLanguage("de");
    expect(auditTableLabel("change_requests")).toBe("Änderungsanfrage");
    expect(auditActionLabel("password_reset")).toBe("Passwort zurückgesetzt");
    expect(auditActionLabel("deactivated")).toBe("Deaktiviert");
    expect(auditActionLabel("reopened")).toBe("Bearbeitung freigegeben");
  });
});
