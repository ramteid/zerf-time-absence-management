// Tests for the usersApi wrappers. These thin functions protect callers from
// depending on raw URL strings; changing the backend route only requires
// editing one place. The tests verify URL construction is correct so
// typos in path templates are caught before they reach production.

import { describe, expect, it, vi } from "vitest";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";
import { getUser, getUsers } from "./usersApi.js";

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
});
