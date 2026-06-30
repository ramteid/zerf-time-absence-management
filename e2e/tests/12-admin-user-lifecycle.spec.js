// File 12 (last) of the e2e suite: admin's user lifecycle operations —
// archive, restore (including the optional start-date reset), and a
// standalone password reset. Run last on purpose: archiving deactivates the
// employee's account, and restoring resets their password and forces another
// password change, so no other spec file can depend on the employee's
// session/credentials after this file runs — in particular it must come
// after 11-final-ui-state.spec.js, which still needs Eve's live session.
//
// Per docs/user-guide.md ("Archiving and restoring users"): archiving
// deactivates a user without deleting their data (so it's reversible), and —
// because a non-admin user always needs at least one approver — restoring a
// non-admin re-requires picking approver(s) and issues a fresh temporary
// password with must_change_password=true, exactly like creating a brand new
// user. If the archived user was themselves someone else's approver, the
// guide says archiving requires a replacement approver for every affected
// report; that branch isn't exercised here because the employee being
// archived doesn't approve anyone (only team leads/admins can be approvers).

import { test, expect } from "@playwright/test";
import { isoOffset, setDate, storageStatePath } from "./helpers.js";
import { EMPLOYEE, TEAM_LEAD } from "./users.js";

test.use({ storageState: storageStatePath("admin") });

// Both AdminUsers.svelte and AdminArchivedUsers.svelte render each user as a
// direct child <div> of the same `.zf-card` list container, so this one
// locator works on either page — just filter by the visible name text.
function userRow(page, fullName) {
  return page.locator(".zf-card > div", { hasText: fullName });
}

test("admin: archive the employee", async ({ page }) => {
  await page.goto("/settings/users");
  const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
  // The Archive button is the only icon-only action button that carries a
  // `title` attribute (Edit and "Reset PW" don't), so it's the only one of
  // the three reachable by accessible name rather than position.
  await row.getByRole("button", { name: "Archive" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Archive user?")).toBeVisible();
  // Eve doesn't approve anyone, so ArchiveUserDialog has nothing to show in
  // its "choose a replacement approver" section — confirming archive is a
  // single click.
  await dialog.getByRole("button", { name: "Archive" }).click();
  await expect(page.getByText("User archived.")).toBeVisible();

  // Archived users move from the active Team Members list to a dedicated
  // Archived Users page; they keep all their historical data (time entries,
  // absences) but can no longer log in until restored.
  await page.goto("/settings/archived-users");
  await expect(
    page.getByText(`${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`),
  ).toBeVisible();
});

test("admin: restore the employee with a reset start date", async ({ page }) => {
  await page.goto("/settings/archived-users");
  const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
  await row.getByRole("button", { name: "Restore" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Restore user?")).toBeVisible();

  // Exercise the optional "reset start date" path (per the user guide: "to
  // avoid a large negative flextime balance from accumulating during the
  // archived period"). Selecting this radio reveals a date field that is
  // then required client-side (RestoreUserDialog blocks submit with
  // "Invalid date." if left empty after opting in).
  await dialog
    .locator('input[name="start-date-mode"][value="true"]')
    .check();
  await setDate(page, "restore-start-date", isoOffset(-10));

  // Restoring a non-admin always requires at least one approver, same rule
  // as creating one — reassign Eve to her original team lead. Unlike
  // UserDialog's approver checklist, RestoreUserDialog's labels render only
  // the name (no "(email)" suffix), so matching must use the name here.
  await dialog
    .locator("label", { hasText: `${TEAM_LEAD.firstName} ${TEAM_LEAD.lastName}` })
    .locator('input[type="checkbox"]')
    .check();
  await dialog.getByRole("button", { name: "Restore" }).click();
  await expect(page.getByText("User restored.")).toBeVisible();

  // Restored accounts reappear in the active Team Members list immediately
  // (no separate "pending restore" state).
  await page.goto("/settings/users");
  await expect(
    page.getByText(`${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`),
  ).toBeVisible();
});

test("admin: reset the employee's password", async ({ page }) => {
  await page.goto("/settings/users");
  const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
  // Row buttons in DOM order: Edit, Reset password (Shield icon), Archive.
  // Edit and Reset have no accessible name (icon-only, no title attribute),
  // so they're targeted positionally rather than by role name.
  await row.getByRole("button").nth(1).click();

  const confirm = page.getByRole("dialog");
  await expect(confirm.getByText("Reset password?")).toBeVisible();
  await confirm.getByRole("button", { name: "Reset PW" }).click();

  // Resetting a password generates a new temporary one and forces a change
  // on next login — the same TempPasswordDialog component used for new-user
  // creation, just with mode="reset" (different title, same "no SMTP
  // configured, deliver this in person" warning when SMTP is off).
  const tempDialog = page.getByRole("dialog");
  await expect(tempDialog.getByText("Password reset.")).toBeVisible();
  await expect(tempDialog.getByText("Temporary password:")).toBeVisible();
  await tempDialog.getByRole("button", { name: "OK" }).click();
});
