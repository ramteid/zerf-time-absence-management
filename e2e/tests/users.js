// The four identities created over the course of the suite, in one place.
//
// Playwright treats every *.spec.js file as an independent test suite root
// and refuses to let one spec file import another (it would make the
// dependency between them implicit and could duplicate test registration).
// These constants therefore can't live in the spec file that first creates
// each user (as exports) the way EMPLOYEE/TEAM_LEAD were originally defined
// in 02-admin-create-users.spec.js — every other spec file that needs to
// refer to "the employee" or "the team lead" imports them from here instead.

export const ADMIN = {
  firstName: "Ada",
  lastName: "Admin",
  email: "admin@e2e.test",
  password: "AdminPass123!",
};

export const TEAM_LEAD = {
  firstName: "Tom",
  lastName: "Lead",
  email: "team.lead@e2e.test",
};

export const EMPLOYEE = {
  firstName: "Eve",
  lastName: "Employee",
  email: "employee@e2e.test",
};

export const ASSISTANT = {
  firstName: "Amy",
  lastName: "Assistant",
  email: "assistant@e2e.test",
};

// The custom absence category 03-admin-config.spec.js creates and
// 05-employee-workflows.spec.js requests an absence under. cost_type="none"
// (a free day) deliberately, not "flextime" or "vacation": a flextime-cost
// absence is rejected by the backend ("Not enough flextime balance") unless
// the requester has already banked enough overtime, and a brand-new
// employee with only a couple of time entries never has — "none" is the only
// cost_type a fresh user can always request regardless of their balance.
export const NO_COST_ABSENCE_CATEGORY = "E2E Day Off";
