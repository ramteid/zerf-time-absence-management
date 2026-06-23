// Tests for the usersApi wrappers. These thin functions protect callers from
// depending on raw URL strings; changing the backend route only requires
// editing one place. The tests verify URL construction is correct so
// typos in path templates are caught before they reach production.

import { describe, expect, it, vi } from "vitest";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";
import {
  getUser,
  getUsers,
  getArchivedUsers,
  archiveUser,
  restoreUser,
} from "./usersApi.js";

describe("usersApi", () => {
  it("getUsers fetches the full team roster from /users", async () => {
    // The AdminUsers page and approval queues both depend on this endpoint
    // returning all users so admins can manage accounts and assign approvers.
    api.mockResolvedValue([{ id: 1, first_name: "Alice" }]);
    const result = await getUsers();
    expect(api).toHaveBeenCalledWith("/users");
    expect(result).toEqual([{ id: 1, first_name: "Alice" }]);
  });

  it("getUser fetches a single user profile by ID from /users/:id", async () => {
    // The UserDialog pre-fills its form by fetching the full user object
    // (including approver IDs and leave days) before opening. The :id
    // segment must be injected without URL encoding issues.
    api.mockResolvedValue({ id: 42, first_name: "Bob" });
    const result = await getUser(42);
    expect(api).toHaveBeenCalledWith("/users/42");
    expect(result).toEqual({ id: 42, first_name: "Bob" });
  });

  it("getArchivedUsers fetches the archived roster from /users/archived", async () => {
    // The AdminArchivedUsers page calls this to list users that have been
    // archived by an admin. All archived records are returned regardless of role.
    api.mockResolvedValue([{ id: 5, first_name: "Eve", archived_at: "2025-01-01T00:00:00Z" }]);
    const result = await getArchivedUsers();
    expect(api).toHaveBeenCalledWith("/users/archived");
    expect(result[0].first_name).toBe("Eve");
  });

  it("archiveUser posts to /users/:id/archive with approver_replacements", async () => {
    // The archive endpoint requires a replacements map when the target user
    // approves active members. String keys are required by the JSON body spec.
    api.mockResolvedValue({ ok: true });
    await archiveUser(7, { "42": 3 });
    expect(api).toHaveBeenCalledWith("/users/7/archive", {
      method: "POST",
      body: { approver_replacements: { "42": 3 } },
    });
  });

  it("archiveUser sends an empty replacements map by default", async () => {
    // When the target user does not approve anyone, the caller omits the map
    // and the function must send an empty object — not undefined or null.
    api.mockResolvedValue({ ok: true });
    await archiveUser(9);
    expect(api).toHaveBeenCalledWith("/users/9/archive", {
      method: "POST",
      body: { approver_replacements: {} },
    });
  });

  it("restoreUser posts to /users/:id/restore with start_date and approver_ids", async () => {
    // Restoring requires the new approver list and optionally a new start date
    // to prevent a flextime gap from accumulating during the archived period.
    api.mockResolvedValue({ id: 7, first_name: "Eve" });
    const result = await restoreUser(7, "2025-06-01", [3]);
    expect(api).toHaveBeenCalledWith("/users/7/restore", {
      method: "POST",
      body: { start_date: "2025-06-01", approver_ids: [3] },
    });
    expect(result.first_name).toBe("Eve");
  });

  it("restoreUser sends null start_date when none is provided", async () => {
    // When the admin keeps the original start date, the payload must use null
    // so the backend does not reset the field.
    api.mockResolvedValue({ id: 7, first_name: "Eve" });
    await restoreUser(7, null, [3]);
    expect(api).toHaveBeenCalledWith("/users/7/restore", {
      method: "POST",
      body: { start_date: null, approver_ids: [3] },
    });
  });
});
