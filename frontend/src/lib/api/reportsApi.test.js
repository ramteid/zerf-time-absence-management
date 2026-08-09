import { beforeEach, describe, expect, it, vi } from "vitest";
import { getLeaveBalances, getUsersForReports } from "./reportsApi.js";

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
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("when canViewTeamReports is true (admin/lead view)", () => {
    // Filtering (pure-admins, inactive) is done by the backend /reports/users
    // endpoint. The client just returns what the backend sends, sorted by role
    // then name. Mock returns only the already-filtered set the backend would
    // return.
    it("calls /reports/users and returns the result sorted by role then name", async () => {
      api.mockResolvedValue([assistant, employee, teamLead]);
      const result = await getUsersForReports(true, pureAdmin);
      expect(api).toHaveBeenCalledWith("/reports/users");
      // team_lead and employee rank above assistant
      expect(result.map((u) => u.id)).toEqual([
        teamLead.id,
        employee.id,
        assistant.id,
      ]);
    });

    it("returns an empty array when the backend returns null or empty", async () => {
      api.mockResolvedValue(null);
      const result = await getUsersForReports(true, pureAdmin);
      expect(result).toEqual([]);
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

describe("getLeaveBalances", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("uses the plural leave-balances route", async () => {
    api.mockResolvedValue([]);

    await getLeaveBalances({ userId: 7, year: 2026 });

    expect(api).toHaveBeenCalledWith("/leave-balances/7?year=2026");
  });
});
