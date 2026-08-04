// File 2: admin creates the organizational hierarchy the rest of the suite
// operates on — a team lead (approved by the admin, since no other team lead
// or admin exists yet) and an employee (approved by that team lead). This
// mirrors how a real org actually onboards people: leads first, then their
// reports, rather than everyone reporting straight to the admin. Per
// docs/user-guide.md, every non-admin user always needs at least one
// approver (a team lead or another admin) — the UserDialog enforces this
// client-side and the backend re-validates it.
//
// TEAM_LEAD and EMPLOYEE's identities live in users.js, the single source of
// truth every other spec file imports from instead of re-typing the strings
// (Playwright disallows importing one spec file from another).

import { test, expect } from "@playwright/test";
import {
  createUserViaAdminUi,
  storageStatePath,
  writeCredential,
} from "./helpers.js";
import { ADMIN, EMPLOYEE, TEAM_LEAD } from "./users.js";

// Resumes the admin session saved at the end of 01-bootstrap.spec.js — no
// fresh login needed. Every test() in this file gets its own page/context
// (Playwright's default `page` fixture), all backed by the same stored
// cookie, which is fine here since nothing in this file needs continuity
// between tests beyond what's already persisted server-side.
test.use({ storageState: storageStatePath("admin") });

// The "create a user via the admin's Add User dialog" flow lives in
// helpers.js as createUserViaAdminUi — 13-payroll-report.spec.js onboards a
// user too, so the interaction is shared rather than duplicated.
const createUser = createUserViaAdminUi;

test("admin: create a team lead, approved by the admin", async ({ page }) => {
  // At this point in the suite the admin is the *only* existing
  // team_lead/admin user, so it's the only eligible approver for a new
  // team lead — there's no other team lead yet to approve this one.
  const password = await createUser(page, {
    ...TEAM_LEAD,
    role: "team_lead",
    approverEmail: ADMIN.email,
  });
  // Written to credentials.json so 04-team-lead-onboarding.spec.js can sign
  // in as Tom for the very first time using this exact password.
  writeCredential("team_lead", TEAM_LEAD.email, password);
});

test("admin: create an employee, approved by the team lead", async ({ page }) => {
  // Now that Tom exists, route Eve's approvals to him instead of the admin —
  // this is what lets 06/10's "team lead reviews the employee" specs exist
  // as team-lead-specific approval coverage rather than duplicating the
  // admin's own approval path.
  const password = await createUser(page, {
    ...EMPLOYEE,
    role: "employee",
    approverEmail: TEAM_LEAD.email,
  });
  writeCredential("employee", EMPLOYEE.email, password);
});
