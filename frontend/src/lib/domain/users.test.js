import { describe, expect, it } from "vitest";
import { findUserById, hasUserId } from "./users.js";

describe("users domain helpers", () => {
  const users = [
    { id: 1, first_name: "Alice" },
    { id: 2, first_name: "Bob" },
  ];

  it("findUserById matches numeric ids from string values", () => {
    expect(findUserById(users, "2")?.first_name).toBe("Bob");
  });

  it("hasUserId matches numeric ids from select string values", () => {
    expect(hasUserId(users, "1")).toBe(true);
    expect(hasUserId(users, "3")).toBe(false);
    expect(hasUserId(users, null)).toBe(false);
  });
});
