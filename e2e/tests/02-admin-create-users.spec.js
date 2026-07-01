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
import { isoOffset, setDate, storageStatePath, writeCredential } from "./helpers.js";
import { ADMIN, EMPLOYEE, TEAM_LEAD } from "./users.js";

// Resumes the admin session saved at the end of 01-bootstrap.spec.js — no
// fresh login needed. Every test() in this file gets its own page/context
// (Playwright's default `page` fixture), all backed by the same stored
// cookie, which is fine here since nothing in this file needs continuity
// between tests beyond what's already persisted server-side.
test.use({ storageState: storageStatePath("admin") });

// Shared "create a user via the admin's Add User dialog" flow, used for
// both the team lead and the employee below — they differ only in role and
// who approves them, so this one helper covers both instead of duplicating
// the whole dialog interaction twice.
async function createUser(page, { firstName, lastName, email, role, approverEmail }) {
  await page.goto("/settings/users");
  await page.getByRole("button", { name: "Add User" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();

  await dialog.locator("#user-first-name").fill(firstName);
  await dialog.locator("#user-last-name").fill(lastName);
  await dialog.locator("#user-email").fill(email);
  await dialog.locator("#user-role").selectOption(role);

  // Backdate the contract start so the user has bookable past weekdays once
  // they log in (per the user guide: an entry's date must be on/after the
  // user's start_date and on/before today — never in the future).
  await setDate(page, "user-start-date", isoOffset(-21));

  // The approver checklist lists every active team_lead/admin user except
  // the one being created; each row's label includes "(email)", so matching
  // on the approver's email uniquely selects their checkbox regardless of
  // how many other eligible approvers are listed.
  await dialog
    .locator("label", { hasText: approverEmail })
    .locator('input[type="checkbox"]')
    .check();

  // No password is supplied here, so the backend generates a random
  // temporary one (UserDialog.svelte only sends `password` in the request
  // body when the admin typed one) and the UI immediately surfaces it via
  // TempPasswordDialog — with a "no SMTP configured, deliver this in person"
  // warning, since 03-admin-config.spec.js's SMTP test runs later and leaves
  // SMTP disabled afterward anyway.
  await dialog.getByRole("button", { name: "Add User" }).click();

  const tempDialog = page.getByRole("dialog");
  await expect(tempDialog.getByText("Temporary password:")).toBeVisible();
  const password = (
    await tempDialog.locator("strong").first().innerText()
  ).trim();
  // Matches the backend's generated-password length floor (see
  // generate_password() in services/users.rs) — a loose sanity check that
  // we actually read a real generated value, not an empty string.
  expect(password.length).toBeGreaterThanOrEqual(12);
  await tempDialog.getByRole("button", { name: "OK" }).click();

  await expect(page.getByText(`${firstName} ${lastName}`)).toBeVisible();
  return password;
}

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
