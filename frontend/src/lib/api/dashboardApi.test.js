import { beforeEach, describe, expect, it, vi } from "vitest";
import { getApprovalDashboard } from "./dashboardApi.js";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";

describe("getApprovalDashboard", () => {
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
    api.mockImplementation(async (path) => {
      if (path === "/time-entries/all?status=submitted") return [];
      if (path === "/absences/all?status=pending_review") return [];
      if (path === "/reopen-requests/pending") return [];
      if (path === "/users")
        return [pureAdmin, teamLead, employee, inactiveEmployee];
      return [];
    });
  });

  it("excludes pure-admin from the users list", async () => {
    const result = await getApprovalDashboard();
    expect(result.users.map((u) => u.id)).not.toContain(pureAdmin.id);
  });

  it("excludes inactive users from the users list", async () => {
    const result = await getApprovalDashboard();
    expect(result.users.map((u) => u.id)).not.toContain(inactiveEmployee.id);
  });

  it("includes active users who track time", async () => {
    const result = await getApprovalDashboard();
    expect(result.users.map((u) => u.id)).toEqual(
      expect.arrayContaining([teamLead.id, employee.id]),
    );
  });

  it("returns submitted time entries, absences, and reopen requests", async () => {
    const mockEntries = [{ id: 10, user_id: 2, status: "submitted" }];
    const mockAbsences = [{ id: 20, user_id: 3, status: "pending_review" }];
    const mockReopens = [{ id: 30, user_id: 2 }];
    api.mockImplementation(async (path) => {
      if (path === "/time-entries/all?status=submitted") return mockEntries;
      if (path === "/absences/all?status=pending_review") return mockAbsences;
      if (path === "/reopen-requests/pending") return mockReopens;
      if (path === "/users") return [teamLead, employee];
      return [];
    });
    const result = await getApprovalDashboard();
    expect(result.submittedTimeEntries).toEqual(mockEntries);
    expect(result.requestedAbsences).toEqual(mockAbsences);
    expect(result.pendingReopenRequests).toEqual(mockReopens);
  });
});
