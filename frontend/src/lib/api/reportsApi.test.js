import { beforeEach, describe, expect, it, vi } from "vitest";
import { getUsersForReports } from "./reportsApi.js";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";

describe("getUsersForReports", () => {
  const pureAdmin = {
    id: 1,
    first_name: "Arnold",
    last_name: "Admin",
    role: "admin",
    tracks_time: false,
    active: true,
  };
  const teamLead = {
    id: 2,
    first_name: "Tabea",
    last_name: "Teamlead",
    role: "team_lead",
    tracks_time: true,
    active: true,
  };
  const employee = {
    id: 3,
    first_name: "Eva",
    last_name: "Erzieherin",
    role: "employee",
    tracks_time: true,
    active: true,
  };
  const assistant = {
    id: 4,
    first_name: "Alina",
    last_name: "Aushilfe",
    role: "assistant",
    tracks_time: true,
    active: true,
  };
  const inactiveEmployee = {
    id: 5,
    first_name: "Ines",
    last_name: "Inaktiv",
    role: "employee",
    tracks_time: true,
    active: false,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("when canViewTeamReports is true (admin/lead view)", () => {
    it("excludes pure-admin user from the list", async () => {
      api.mockResolvedValue([pureAdmin, teamLead, employee, assistant]);
      const result = await getUsersForReports(true, pureAdmin);
      expect(result.map((u) => u.id)).not.toContain(pureAdmin.id);
      expect(result.map((u) => u.id)).toEqual(
        expect.arrayContaining([teamLead.id, employee.id, assistant.id]),
      );
    });

    it("excludes inactive users from the list", async () => {
      api.mockResolvedValue([
        pureAdmin,
        teamLead,
        employee,
        assistant,
        inactiveEmployee,
      ]);
      const result = await getUsersForReports(true, pureAdmin);
      expect(result.map((u) => u.id)).not.toContain(inactiveEmployee.id);
    });

    it("returns only active users who track time", async () => {
      api.mockResolvedValue([
        pureAdmin,
        teamLead,
        employee,
        assistant,
        inactiveEmployee,
      ]);
      const result = await getUsersForReports(true, pureAdmin);
      for (const user of result) {
        expect(user.tracks_time).not.toBe(false);
        expect(user.active).not.toBe(false);
      }
    });
  });

  describe("when canViewTeamReports is false (self-only view)", () => {
    it("returns current user if they track time", async () => {
      const result = await getUsersForReports(false, teamLead);
      expect(result).toEqual([teamLead]);
      expect(api).not.toHaveBeenCalled();
    });

    it("returns empty array for pure-admin", async () => {
      const result = await getUsersForReports(false, pureAdmin);
      expect(result).toEqual([]);
      expect(api).not.toHaveBeenCalled();
    });
  });
});
