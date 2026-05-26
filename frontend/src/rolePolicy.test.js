import { describe, expect, it } from "vitest";
import {
  isAssistantUser,
  isPureAdminUser,
  tracksOwnTime,
  hasFlextimeAccount,
} from "./rolePolicy.js";

describe("rolePolicy", () => {
  describe("tracksOwnTime", () => {
    it("returns false for null/undefined", () => {
      expect(tracksOwnTime(null)).toBe(false);
      expect(tracksOwnTime(undefined)).toBe(false);
    });

    it("returns false for pure-admin (tracks_time=false)", () => {
      const pureAdmin = { id: 1, role: "admin", tracks_time: false };
      expect(tracksOwnTime(pureAdmin)).toBe(false);
    });

    it("returns true for admin with tracks_time=true", () => {
      const admin = { id: 1, role: "admin", tracks_time: true };
      expect(tracksOwnTime(admin)).toBe(true);
    });

    it("returns true for regular employee", () => {
      const employee = { id: 2, role: "employee", tracks_time: true };
      expect(tracksOwnTime(employee)).toBe(true);
    });

    it("returns true for team_lead", () => {
      const lead = { id: 3, role: "team_lead", tracks_time: true };
      expect(tracksOwnTime(lead)).toBe(true);
    });

    it("returns true for assistant", () => {
      const assistant = { id: 4, role: "assistant", tracks_time: true };
      expect(tracksOwnTime(assistant)).toBe(true);
    });

    it("returns true when tracks_time is undefined (default)", () => {
      const user = { id: 5, role: "employee" };
      expect(tracksOwnTime(user)).toBe(true);
    });
  });

  describe("isPureAdminUser", () => {
    it("returns true for admin with tracks_time=false", () => {
      const pureAdmin = { id: 1, role: "admin", tracks_time: false };
      expect(isPureAdminUser(pureAdmin)).toBe(true);
    });

    it("returns false for admin with tracks_time=true", () => {
      const admin = { id: 1, role: "admin", tracks_time: true };
      expect(isPureAdminUser(admin)).toBe(false);
    });

    it("returns false for non-admin even if tracks_time=false", () => {
      const user = { id: 2, role: "employee", tracks_time: false };
      expect(isPureAdminUser(user)).toBe(false);
    });

    it("returns false for null/undefined", () => {
      expect(isPureAdminUser(null)).toBe(false);
      expect(isPureAdminUser(undefined)).toBe(false);
    });
  });

  describe("isAssistantUser", () => {
    it("returns true for assistant role", () => {
      expect(isAssistantUser({ role: "assistant" })).toBe(true);
    });

    it("returns false for other roles", () => {
      expect(isAssistantUser({ role: "admin" })).toBe(false);
      expect(isAssistantUser({ role: "employee" })).toBe(false);
      expect(isAssistantUser({ role: "team_lead" })).toBe(false);
    });
  });

  describe("hasFlextimeAccount", () => {
    it("returns true for non-assistant users", () => {
      expect(hasFlextimeAccount({ role: "admin" })).toBe(true);
      expect(hasFlextimeAccount({ role: "employee" })).toBe(true);
      expect(hasFlextimeAccount({ role: "team_lead" })).toBe(true);
    });

    it("returns false for assistant", () => {
      expect(hasFlextimeAccount({ role: "assistant" })).toBe(false);
    });

    it("returns false for null/undefined", () => {
      expect(hasFlextimeAccount(null)).toBe(false);
      expect(hasFlextimeAccount(undefined)).toBe(false);
    });
  });
});
