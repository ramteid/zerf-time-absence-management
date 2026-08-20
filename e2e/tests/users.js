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

// A second, independent leave account created through the category UI after
// the initial users exist. The later employee workflow verifies that this
// account is seeded for existing users and is charged independently from the
// canonical Vacation account.
export const LEAVE_ACCOUNT_CATEGORY = "E2E Educational Leave";

// Created last, by 13-payroll-report.spec.js. Their contract start is
// backdated far enough (see PAYROLL_START_OFFSET_DAYS) that they always fall
// inside the previous calendar month, which is what the payroll report and
// its dashboard card cover. Without that guarantee the card's population
// would depend on which day of the month the suite happens to run.
export const PAYROLL_EMPLOYEE = {
  firstName: "Pia",
  lastName: "Payroll",
  email: "payroll.employee@e2e.test",
};

// The largest possible gap between today and the last day of the previous
// month is 31 days (on the 31st of a month), so any offset beyond that always
// starts the contract on or before the end of the previous month.
export const PAYROLL_START_OFFSET_DAYS = -70;

// Two more people, created by 11b-dashboard-absence-slider.spec.js to test
// the dashboard's "Who is absent" tile in isolation from every other spec's
// absence data. Kept separate from EMPLOYEE/ASSISTANT so that tile's
// week-by-week navigation can assert exactly who is (and isn't) shown in a
// given week without depending on dates other spec files happened to pick.
export const ABSENCE_SLIDER_EMPLOYEE_ONE = {
  firstName: "Sina",
  lastName: "SliderOne",
  email: "slider.one@e2e.test",
};

export const ABSENCE_SLIDER_EMPLOYEE_TWO = {
  firstName: "Theo",
  lastName: "SliderTwo",
  email: "slider.two@e2e.test",
};

// Backdated far enough (matches PAYROLL_START_OFFSET_DAYS) that every past
// week used by 11b — up to 4 weeks back — is safely after the contract
// start, regardless of which weekday the suite happens to run on.
export const ABSENCE_SLIDER_START_OFFSET_DAYS = -70;
