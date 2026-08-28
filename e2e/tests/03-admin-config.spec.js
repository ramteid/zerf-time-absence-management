// File 3: admin configuration surfaces that aren't part of the user/approval
// flow — work categories, absence categories, holidays, the audit log, and
// SMTP. Each test here is self-contained (no dependency on the other tests
// in this file), but the file as a whole must run after 02 (it edits the
// already-bootstrapped admin's settings) and before 05 (which books an
// absence in the NO_COST_ABSENCE_CATEGORY created here, proving the category
// is actually usable end-to-end, not just creatable).

import { test, expect } from "@playwright/test";
import {
  fillSmtpSettings,
  freeHolidayDate,
  setDate,
  storageStatePath,
} from "./helpers.js";
// 05-employee-workflows.spec.js selects this exact category by name when
// requesting an absence — proving a category created through the admin UI
// is immediately available in the employee-facing dropdown. Defined in
// users.js, not exported from here, since Playwright disallows importing
// one spec file from another.
import {
  EMPLOYEE,
  LEAVE_ACCOUNT_CATEGORY,
  NO_COST_ABSENCE_CATEGORY,
} from "./users.js";

test.use({ storageState: storageStatePath("admin") });

test("admin: add a work category", async ({ page }) => {
  await page.goto("/settings/categories");
  // AdminCategories.svelte renders two sections on one page — "Time
  // Categories" then "Absence Categories" — each with its own "Add" button.
  // Both buttons have the same accessible name ("Add"), so the only way to
  // distinguish them is DOM order: the Time Categories one comes first.
  //
  // Wait for the seeded "Core Duties" category to render before clicking
  // Add: the page's own list of existing categories (loaded async via
  // GET /categories/all) is what the dialog uses to default a new
  // category's sort order to the end of the list. Clicking Add before that
  // fetch resolves would default the new category to sort_order 0 — tied
  // with every category still sitting at its seeded default — instead of
  // appending after them.
  await expect(page.getByText("Core Duties")).toBeVisible();
  await page.getByRole("button", { name: "Add" }).first().click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Add Category")).toBeVisible();
  await dialog.locator("#cat-name").fill("E2E Project Work");
  await dialog.locator("#cat-description").fill("Created by the e2e suite");
  // Leave "Counts as work" checked (the dialog's default) — this is a
  // billable/worked-hours category, not a flextime-reduction one.
  await dialog.getByRole("button", { name: "Save" }).click();

  await expect(page.getByText("E2E Project Work")).toBeVisible();
});

test("admin: add an absence category", async ({ page }) => {
  await page.goto("/settings/categories");
  // Same load-before-click reasoning as the work-category test above, but
  // for the seeded "Vacation" absence category and GET /absence-categories/all.
  // This matters even more here: a new category tied with Vacation at
  // sort_order 0 would sort *before* it alphabetically ("E2E ..." < "Vacation"),
  // making it the default pre-selected kind in every "Request Absence"
  // dialog instead of Vacation.
  await expect(page.getByText("Vacation")).toBeVisible();
  await page.getByRole("button", { name: "Add" }).nth(1).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Add Absence Category")).toBeVisible();
  await dialog.locator("#abscat-name").fill(NO_COST_ABSENCE_CATEGORY);
  // cost_type is a 3-state radio (none / vacation / flextime) per the user
  // guide. "none" (a free day with no balance impact) is deliberately
  // chosen over "flextime" or "vacation": the employee who later requests
  // this absence (05-employee-workflows.spec.js) is a brand-new hire with no
  // banked overtime and limited vacation days, and the backend rejects a
  // flextime-cost absence outright ("Not enough flextime balance") when the
  // requester's flextime account can't cover it. "none" is the only
  // cost_type guaranteed to succeed regardless of balance, while still
  // differing from the seeded "Vacation" category used elsewhere.
  await dialog.locator('input[type="radio"][value="none"]').check();
  await dialog.getByRole("button", { name: "Save" }).click();

  await expect(page.getByText(NO_COST_ABSENCE_CATEGORY)).toBeVisible();
});

test("admin: add a second leave-account category", async ({ page }) => {
  await page.goto("/settings/categories");
  // Same load-before-click reasoning as "admin: add an absence category"
  // above — wait for the list (and this file's own "E2E Day Off", already
  // created above) so this category's sort order appends after both.
  await expect(page.getByText(NO_COST_ABSENCE_CATEGORY)).toBeVisible();
  await page.getByRole("button", { name: "Add" }).nth(1).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Add Absence Category")).toBeVisible();
  await dialog.locator("#abscat-name").fill(LEAVE_ACCOUNT_CATEGORY);
  await dialog.locator("#abscat-cost-type-vacation").check();
  await dialog.locator("#abscat-leave-account-default-days").fill("5");
  await dialog.locator("#abscat-leave-account-carryover-expiry").fill("01-01");
  await dialog.getByRole("button", { name: "Save" }).click();

  await expect(page.getByText(LEAVE_ACCOUNT_CATEGORY)).toBeVisible();

  const usersResponse = await page.request.get("/api/v1/users");
  expect(usersResponse.ok()).toBeTruthy();
  const employee = (await usersResponse.json()).find(
    (user) => user.email === EMPLOYEE.email,
  );
  expect(employee).toBeTruthy();
  const accountsResponse = await page.request.get(
    `/api/v1/users/${employee.id}/leave-accounts`,
  );
  expect(accountsResponse.ok()).toBeTruthy();
  const account = (await accountsResponse.json()).find(
    (entry) => entry.category_name === LEAVE_ACCOUNT_CATEGORY,
  );
  expect(account?.base_days).toBe(5);

  const row = page.locator(".zf-card > div", { hasText: LEAVE_ACCOUNT_CATEGORY });
  await row.getByRole("button").first().click();
  await expect(dialog.locator("#abscat-cost-type-vacation")).toBeDisabled();
  await expect(dialog.locator("#abscat-leave-account-default-days")).toHaveValue("5");
  await expect(dialog.locator("#abscat-leave-account-carryover-expiry")).toHaveValue("01-01");
  await dialog.getByRole("button", { name: "Cancel" }).click();
});

test("admin: add a manual holiday", async ({ page }) => {
  await page.goto("/settings/holidays");
  // Unlike categories, AdminHolidays.svelte has no separate add dialog — the
  // date/name inputs and the Add button all live on the page itself.
  // Pick a future date that isn't already a seeded public holiday so the
  // create can't hit the holidays.holiday_date UNIQUE constraint (see
  // freeHolidayDate for the full rationale).
  await setDate(page, "holiday-date", await freeHolidayDate(page.request, 60));
  await page.locator("#holiday-name").fill("E2E Company Holiday");
  await page.getByRole("button", { name: "Add" }).click();

  await expect(page.getByText("Holiday added.")).toBeVisible();
  await expect(page.getByText("E2E Company Holiday")).toBeVisible();
  // Per the user guide, holidays are excluded both from absence workday
  // counts and from the daily work target — this test only proves the
  // holiday is created, not those downstream effects (which would require a
  // time entry or absence dated on top of it).
});

test("admin: category and holiday creation appear in the audit log", async ({
  page,
}) => {
  // Regression test: categories and holidays used to be created without ever
  // calling audit::log, so admin settings > Audit Log silently omitted them
  // (see backend/src/services/categories.rs and
  // backend/src/services/holidays.rs). The category and holiday added earlier
  // in this file must show up here.
  await page.goto("/settings/audit-log");
  await expect(
    page.locator(".audit-row", { hasText: "E2E Project Work" }),
  ).toBeVisible();
  await expect(
    page.locator(".audit-row", { hasText: "E2E Company Holiday" }),
  ).toBeVisible();
});

test("admin: view an audit log entry's detail", async ({ page }) => {
  await page.goto("/settings/audit-log");
  // By this point in the suite there's already a rich audit trail (settings
  // updated, two users created) — just confirm clicking *any* row opens the
  // detail dialog rather than asserting on a specific entry's content.
  const firstRow = page.locator(".audit-row").first();
  await expect(firstRow).toBeVisible();
  await firstRow.click();

  // .zf-detail-row: the shared detail-dialog row class used since the audit
  // page moved onto the LogList component (System Log refactor).
  await expect(page.locator(".zf-detail-row").first()).toBeVisible();
  // AdminAuditLog.svelte's detail dialog has no footer/Close button — it
  // relies on the native <dialog> element's built-in Escape-to-close
  // behavior, which fires the same onClose handler the header's X button
  // would. Pressing Escape here exercises that path specifically.
  await page.keyboard.press("Escape");
  await expect(page.locator(".zf-detail-row")).toHaveCount(0);
});

test("admin: configure and test SMTP settings", async ({ page }) => {
  // A deliberately unresolvable host — the goal isn't to prove email actually
  // sends (the stack's own mail server covers that in 13-payroll-report), it's
  // to prove the "Test Connection" button triggers a real network attempt
  // against the backend rather than a client-side mock, by observing it fail.
  await fillSmtpSettings(page, {
    host: "smtp.invalid.e2e-test",
    from: "Zerf <noreply@e2e.test>",
    enabled: true,
  });

  await page.getByRole("button", { name: "Test Connection" }).click();
  // Waiting for the "Testing..." button label to disappear is NOT a valid
  // completion signal here: a DNS lookup failure against an unresolvable
  // host resolves in well under a second, so by the time this assertion
  // starts polling, the button may already have reverted — `toBeHidden()`
  // on a locator that matches zero elements passes trivially, proving
  // nothing about whether the request ever actually ran. The status text
  // next to the status dot is a real state-change signal instead: it reads
  // "Not tested" until `testResult` is populated by the response, so
  // waiting for that text to go away only succeeds once a result has
  // genuinely landed. The backend's SMTP client (see email.rs
  // test_connection) caps the attempt at a 10s timeout, so 15s gives
  // comfortable headroom without the test hanging indefinitely if something
  // regresses.
  await expect(page.getByText("Not tested")).toBeHidden({ timeout: 15000 });

  // Saving is *not* tried here with SMTP still enabled: PUT /settings/smtp
  // re-validates the connection server-side whenever smtp_enabled=true (see
  // handlers/settings.rs update_smtp_settings) and rejects the whole save
  // with 400 if it fails — so an unreachable host can never actually be
  // persisted in the enabled state, only tested. Disabling first sidesteps
  // that re-validation (an admin turning SMTP off doesn't need a working
  // host) and proves the plain save path succeeds.
  //
  // Email therefore stays OFF for the rest of the suite, which is what the
  // later files assert against (the "deliver this password in person" notice
  // in 12-admin-user-lifecycle). 13-payroll-report turns it on for real, and
  // runs last precisely so that switch cannot affect anything before it.
  await page.locator('input[type="checkbox"]').first().uncheck();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("SMTP settings saved.")).toBeVisible();
});

test("admin: enable team lead assistant management", async ({ page }) => {
  // The `allow_team_lead_manage_assistants` setting is off by default.
  // 04-team-lead-onboarding.spec.js relies on it being enabled so the team
  // lead can access /settings/team-users and create an assistant. This test
  // enables the setting through the admin Users page (above the user list),
  // where it was moved to be more contextually relevant.
  const settingsLoaded = page.waitForResponse(
    (response) =>
      response.url().endsWith("/api/v1/settings") &&
      response.request().method() === "GET",
  );
  await page.goto("/settings/users");
  await settingsLoaded;

  // Locate the checkbox by the text of its wrapping label.
  const label = page.locator("label", {
    hasText: "Allow team leads to create assistant users",
  });
  await label.locator('input[type="checkbox"]').check();

  const saveButton = page.getByRole("button", { name: "Save Changes" });
  await expect(saveButton).toBeEnabled();
  await saveButton.click();
  await expect(page.getByText("Settings saved.")).toBeVisible();
});
