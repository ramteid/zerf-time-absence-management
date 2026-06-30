// File 3: admin configuration surfaces that aren't part of the user/approval
// flow — work categories, absence categories, holidays, the audit log, and
// SMTP. Each test here is self-contained (no dependency on the other tests
// in this file), but the file as a whole must run after 02 (it edits the
// already-bootstrapped admin's settings) and before 05 (which books an
// absence in the NO_COST_ABSENCE_CATEGORY created here, proving the category
// is actually usable end-to-end, not just creatable).

import { test, expect } from "@playwright/test";
import { isoOffset, setDate, storageStatePath } from "./helpers.js";
// 05-employee-workflows.spec.js selects this exact category by name when
// requesting an absence — proving a category created through the admin UI
// is immediately available in the employee-facing dropdown. Defined in
// users.js, not exported from here, since Playwright disallows importing
// one spec file from another.
import { NO_COST_ABSENCE_CATEGORY } from "./users.js";

test.use({ storageState: storageStatePath("admin") });

test("admin: add a work category", async ({ page }) => {
  await page.goto("/settings/categories");
  // AdminCategories.svelte renders two sections on one page — "Time
  // Categories" then "Absence Categories" — each with its own "Add" button.
  // Both buttons have the same accessible name ("Add"), so the only way to
  // distinguish them is DOM order: the Time Categories one comes first.
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

test("admin: add a manual holiday", async ({ page }) => {
  await page.goto("/settings/holidays");
  // Unlike categories, AdminHolidays.svelte has no separate add dialog — the
  // date/name inputs and the Add button all live on the page itself.
  await setDate(page, "holiday-date", isoOffset(60));
  await page.locator("#holiday-name").fill("E2E Company Holiday");
  await page.getByRole("button", { name: "Add" }).click();

  await expect(page.getByText("Holiday added.")).toBeVisible();
  await expect(page.getByText("E2E Company Holiday")).toBeVisible();
  // Per the user guide, holidays are excluded both from absence workday
  // counts and from the daily work target — this test only proves the
  // holiday is created, not those downstream effects (which would require a
  // time entry or absence dated on top of it).
});

test("admin: view an audit log entry's detail", async ({ page }) => {
  await page.goto("/settings/audit-log");
  // By this point in the suite there's already a rich audit trail (settings
  // updated, two users created) — just confirm clicking *any* row opens the
  // detail dialog rather than asserting on a specific entry's content.
  const firstRow = page.locator(".audit-row").first();
  await expect(firstRow).toBeVisible();
  await firstRow.click();

  await expect(page.locator(".detail-row").first()).toBeVisible();
  // AdminAuditLog.svelte's detail dialog has no footer/Close button — it
  // relies on the native <dialog> element's built-in Escape-to-close
  // behavior, which fires the same onClose handler the header's X button
  // would. Pressing Escape here exercises that path specifically.
  await page.keyboard.press("Escape");
  await expect(page.locator(".detail-row")).toHaveCount(0);
});

test("admin: configure and test SMTP settings", async ({ page }) => {
  // AdminEmail.svelte's load() fetches the current settings asynchronously
  // and then *replaces* the whole local `smtpSettings` object once it
  // resolves — exactly like AdminSettings.svelte in 01-bootstrap.spec.js.
  // Filling the form before that replacement lands would have our input
  // silently wiped out. Waiting for the GET /settings response that load()
  // triggers is a reliable way to know the replacement has already happened.
  // `.endsWith` (not `.includes`) matters here: App.svelte's own boot
  // sequence also calls GET /api/v1/settings/public, which would otherwise
  // false-match and resolve this promise too early.
  const settingsLoaded = page.waitForResponse(
    (response) =>
      response.url().endsWith("/api/v1/settings") &&
      response.request().method() === "GET",
  );
  await page.goto("/settings/email");
  await settingsLoaded;

  // Three checkboxes exist on this page in DOM order: Enable SMTP, Enable
  // reminders, Enable approval reminders. The latter two start disabled
  // until SMTP is enabled, so ".first()" unambiguously targets "Enable SMTP"
  // regardless of their disabled state.
  await page.locator('input[type="checkbox"]').first().check();
  // A deliberately unresolvable host — the goal isn't to prove email
  // actually sends (this stack has no SMTP server), it's to prove the "Test
  // Connection" button triggers a real network attempt against the backend
  // rather than a client-side mock, by observing it fail.
  await page.locator("#smtp-host").fill("smtp.invalid.e2e-test");
  await page.locator("#smtp-port").fill("587");
  await page.locator("#smtp-from").fill("Zerf <noreply@e2e.test>");

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
  await page.locator('input[type="checkbox"]').first().uncheck();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("SMTP settings saved.")).toBeVisible();
});
