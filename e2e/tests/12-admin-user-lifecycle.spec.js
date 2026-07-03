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
import { freeHolidayDate, isoOffset, setDate, storageStatePath } from "./helpers.js";
import { EMPLOYEE, TEAM_LEAD } from "./users.js";

test.use({ storageState: storageStatePath("admin") });

// AdminUsers.svelte renders both the active and archived rosters as direct
// child <div> elements of `.zf-card` list containers, so this one locator
// works for either section — just filter by the visible name text.
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

  // Archived users move from the active Users list to a dedicated archived
  // section displayed below the main list on the same page; they keep all
  // their historical data (time entries, absences) but can no longer log in
  // until restored.
  await page.goto("/settings/users");
  await expect(page.getByRole("heading", { name: "Archived Users" })).toBeVisible();
  await expect(
    page.getByText(`${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`),
  ).toBeVisible();
});

test("admin: restore the employee with a reset start date", async ({ page }) => {
  await page.goto("/settings/users");
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

  // Restored accounts reappear in the active Users list immediately
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

test("admin: audit log still loads after the acting user is hard-deleted (null user_id regression)", async ({
  page,
  browser,
}, testInfo) => {
  // Regression test: audit_log.user_id is nullable (migration 005 added
  // ON DELETE SET NULL) but the Rust LogEntry struct had user_id: i64
  // (non-optional). sqlx panicked with "unexpected null" the moment any
  // user who had acted on something was later deleted, and the API returned
  // 500 — leaving the Audit Log settings page completely empty.
  //
  // This test exercises the exact failure path:
  //   1. Create a throwaway admin user (admins need no approver).
  //   2. As that admin (via the API), create a holiday so their user_id
  //      appears in audit_log as the *actor*.
  //   3. Hard-delete the throwaway admin via the API. PostgreSQL fires the
  //      ON DELETE SET NULL FK constraint, NULLing their actor rows.
  //   4. Verify the audit log page still loads and the entry is visible.
  //
  // The email and holiday name are scoped to the Playwright retry index so a
  // throwaway user / audit row left behind by a timed-out earlier attempt can't
  // make the retry fail on a duplicate email or match two rows in the final
  // audit assertion. The holiday date comes from freeHolidayDate, which both
  // avoids the seeded public holidays (holidays.holiday_date is UNIQUE) and
  // skips any holiday a prior attempt already created.
  const runId = testInfo.retry;
  const actorEmail = `temp.actor.${runId}@e2e.test`;
  const holidayName = `Temp Actor Holiday ${runId}`;
  const holidayDate = await freeHolidayDate(page.request, 120);

  // Step 1: Create the throwaway admin via the normal "Add User" flow and read
  // the temporary password off the confirmation dialog.
  await page.goto("/settings/users");
  await page.getByRole("button", { name: "Add User" }).click();
  const createDialog = page.getByRole("dialog");
  await expect(createDialog).toBeVisible();
  await createDialog.locator("#user-first-name").fill("Temp");
  await createDialog.locator("#user-last-name").fill("Actor");
  await createDialog.locator("#user-email").fill(actorEmail);
  await createDialog.locator("#user-role").selectOption("admin");
  await setDate(page, "user-start-date", isoOffset(-7));
  // Admins do not require an approver (user guide: "non-admin users must have
  // an approver"), so no approver checkbox is needed here.
  await createDialog.getByRole("button", { name: "Add User" }).click();
  const pwDialog = page.getByRole("dialog");
  await expect(pwDialog.getByText("Temporary password:")).toBeVisible();
  const tempPassword = (await pwDialog.locator("strong").first().innerText()).trim();
  expect(tempPassword.length).toBeGreaterThanOrEqual(12);
  await pwDialog.getByRole("button", { name: "OK" }).click();

  // Step 2: Make the throwaway admin an *audit actor* entirely through the API.
  // Booting a second browser context to drive the UI here was both slow (a cold
  // SPA load per action) and the reason this test used to exceed its timeout — a
  // few API calls on a throwaway request context are far faster and fully
  // deterministic. The context keeps its own cookie jar, independent of the
  // admin `page` session.
  const actorContext = await browser.newContext();
  const actorApi = actorContext.request;

  // Log in with the temporary password. The login response carries the CSRF
  // token and the new user's id (the flat /auth/me payload has no `user` key).
  const loginResp = await actorApi.post("/api/v1/auth/login", {
    data: { email: actorEmail, password: tempPassword },
  });
  expect(loginResp.ok()).toBeTruthy();
  const login = await loginResp.json();
  expect(login.must_change_password).toBeTruthy();
  const actorCsrf = login.csrf_token;
  const actorUserId = login.user.id;

  // Complete the forced first-login password change. Until it's done, the
  // must_change_password gate blocks every endpoint except the identity/password
  // ones — including creating a holiday (see middleware/auth.rs).
  const changeResp = await actorApi.put("/api/v1/auth/password", {
    headers: { "X-CSRF-Token": actorCsrf },
    data: { new_password: "TempActorPass!77" },
  });
  expect(changeResp.ok()).toBeTruthy();

  // Create a holiday as the throwaway admin — this writes an audit_log row with
  // user_id = throwaway admin's id and action = "created".
  const holidayResp = await actorApi.post("/api/v1/holidays", {
    headers: { "X-CSRF-Token": actorCsrf },
    data: { holiday_date: holidayDate, name: holidayName },
  });
  expect(holidayResp.ok()).toBeTruthy();
  await actorContext.close();

  // Step 3: Hard-delete the throwaway admin via the API. A user with no time
  // entries or absences (only audit_log rows) satisfies the backend's
  // "no historical data" guard and can be permanently removed. PostgreSQL
  // then fires ON DELETE SET NULL on audit_log.user_id for all rows where
  // user_id = throwaway admin's id — including the holiday entry above.
  const adminMe = await page.request.get("/api/v1/auth/me");
  const { csrf_token: adminCsrf } = await adminMe.json();
  const deleteResp = await page.request.delete(`/api/v1/users/${actorUserId}`, {
    headers: { "X-CSRF-Token": adminCsrf },
  });
  expect(deleteResp.ok()).toBeTruthy();

  // Step 4: The audit log must still load. Before the fix the LogEntry
  // struct decoded user_id as i64 (non-optional), so any NULL row caused a
  // 500 and the page showed nothing. With user_id: Option<i64> the NULL
  // decodes cleanly and the frontend renders "System" for that actor.
  await page.goto("/settings/audit-log");
  await expect(page.locator(".audit-row").first()).toBeVisible();
  // The throwaway admin's holiday entry is still present, now labelled
  // "System" (i18n key: audit_system_user) because their account was deleted.
  await expect(
    page.locator(".audit-row", { hasText: holidayName }),
  ).toBeVisible();
});
