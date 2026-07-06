import { describe, expect, it } from "vitest";
import {
  findUserById,
  hasUserId,
  sortUsersByRoleThenName,
  timeTrackingUsers,
  userAvatarClass,
  userAvatarClassFromRows,
  userFullName,
  userInitials,
  userInitialsFromRows,
  userNameFromRows,
  userWorkdaysPerWeek,
  userWorkdaysPerWeekById,
} from "./users.js";

describe("users domain helpers", () => {
  const users = [
    { id: 1, first_name: "Alice", last_name: "Admin", workdays_per_week: 5, tracks_time: false, role: "admin" },
    { id: 2, first_name: "Bob", last_name: "Emp", workdays_per_week: 4, tracks_time: true, role: "employee" },
  ];

  it("findUserById matches numeric ids from string values", () => {
    expect(findUserById(users, "2")?.first_name).toBe("Bob");
  });

  it("findUserById returns the fallback user when id matches it but not the list", () => {
    // Used in EmployeeReport when the current user is a pure-admin who isn't
    // in the `users` list but the report is still for their own id.
    const fallback = { id: 99, first_name: "Admin" };
    expect(findUserById([], "99", fallback)).toBe(fallback);
  });

  it("findUserById returns null when no match and no fallback", () => {
    expect(findUserById(users, 99)).toBeNull();
  });

  it("hasUserId matches numeric ids from select string values", () => {
    expect(hasUserId(users, "1")).toBe(true);
    expect(hasUserId(users, "3")).toBe(false);
    expect(hasUserId(users, null)).toBe(false);
  });

  it("timeTrackingUsers filters out pure-admin users who do not track time", () => {
    // Pure admins (tracks_time=false) never appear in employee-selection
    // dropdowns — their reports and absence data do not exist.
    const result = timeTrackingUsers(users);
    expect(result.map((u) => u.id)).toEqual([2]);
  });

  it("timeTrackingUsers returns empty array for null input", () => {
    expect(timeTrackingUsers(null)).toEqual([]);
  });

  it("userFullName joins first and last name", () => {
    expect(userFullName({ first_name: "Alice", last_name: "Admin" })).toBe(
      "Alice Admin",
    );
  });

  it("userFullName returns the fallback when user is null", () => {
    expect(userFullName(null, "Unknown")).toBe("Unknown");
  });

  it("userNameFromRows falls back to #id when user not found", () => {
    expect(userNameFromRows(99, users)).toBe("#99");
  });

  it("userInitials upper-cases the first letters of first and last name", () => {
    expect(userInitials({ first_name: "alice", last_name: "admin" })).toBe("AA");
  });

  it("userInitials returns empty string for null user", () => {
    expect(userInitials(null)).toBe("");
  });

  it("userInitialsFromRows looks up initials from the list", () => {
    expect(userInitialsFromRows(1, users)).toBe("AA");
  });

  it("userAvatarClass maps each role to its own CSS class", () => {
    expect(userAvatarClass({ role: "admin" })).toBe("avatar-role-admin");
    expect(userAvatarClass({ role: "team_lead" })).toBe("avatar-role-team_lead");
    expect(userAvatarClass({ role: "employee" })).toBe("avatar-role-employee");
    expect(userAvatarClass({ role: "assistant" })).toBe("avatar-role-assistant");
  });

  it("userAvatarClass returns an empty string for an unknown or missing role", () => {
    expect(userAvatarClass({ role: "bogus" })).toBe("");
    expect(userAvatarClass(null)).toBe("");
  });

  it("userAvatarClassFromRows looks up the role class from the list", () => {
    expect(userAvatarClassFromRows(1, users)).toBe("avatar-role-admin");
    expect(userAvatarClassFromRows(2, users)).toBe("avatar-role-employee");
  });

  it("sortUsersByRoleThenName groups by role (team lead, employee, assistant, admin), then by name", () => {
    const mixed = [
      { first_name: "Zoe", last_name: "Admin", role: "admin" },
      { first_name: "Amy", last_name: "Assist", role: "assistant" },
      { first_name: "Bob", last_name: "Zephyr", role: "employee" },
      { first_name: "Cara", last_name: "Anderson", role: "employee" },
      { first_name: "Dan", last_name: "Lead", role: "team_lead" },
    ];
    expect(sortUsersByRoleThenName(mixed).map((u) => u.first_name)).toEqual([
      "Dan", // team_lead
      "Cara", // employee, "Anderson" before "Zephyr"
      "Bob", // employee
      "Amy", // assistant
      "Zoe", // admin
    ]);
  });

  it("sortUsersByRoleThenName does not mutate the input array and handles null", () => {
    const original = [
      { first_name: "B", last_name: "B", role: "admin" },
      { first_name: "A", last_name: "A", role: "team_lead" },
    ];
    const copy = [...original];
    sortUsersByRoleThenName(original);
    expect(original).toEqual(copy);
    expect(sortUsersByRoleThenName(null)).toEqual([]);
  });

  it("userWorkdaysPerWeek returns the user's configured value", () => {
    expect(userWorkdaysPerWeek({ workdays_per_week: 4 })).toBe(4);
  });

  it("userWorkdaysPerWeek returns the fallback for invalid values", () => {
    // Guards against corrupted or missing workdays data in legacy records.
    expect(userWorkdaysPerWeek({ workdays_per_week: 0 })).toBe(5);
    expect(userWorkdaysPerWeek({ workdays_per_week: 8 })).toBe(5);
    expect(userWorkdaysPerWeek(null)).toBe(5);
  });

  it("userWorkdaysPerWeekById looks up by id and returns the workdays value", () => {
    expect(userWorkdaysPerWeekById(users, 2)).toBe(4);
  });

  it("userWorkdaysPerWeekById returns the fallback for an unknown id", () => {
    expect(userWorkdaysPerWeekById(users, 99)).toBe(5);
  });
});
