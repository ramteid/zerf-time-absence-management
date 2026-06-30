// File 1 of the e2e suite: bootstraps the very first administrator and
// completes the mandatory first-run settings. This is the only spec file
// that ever sees the Setup screen — a fresh Docker stack (provisioned by
// run.sh) has an empty `users` table, so the backend's GET /auth/setup-status
// reports needs_setup=true and the SPA shows Setup.svelte instead of Login.
// Every later spec file resumes this admin's session via the `storageState`
// snapshot saved at the end of this file (see helpers.js for how that works
// and why it's safe — restoring just the session cookie is enough, the app
// re-fetches a fresh CSRF token on boot).
//
// ADMIN's identity lives in users.js (not exported from here) — Playwright
// disallows importing one spec file from another, so anything other files
// need to reference is defined in that shared, non-test module instead.

import { test, expect } from "@playwright/test";
import { storageStatePath, writeCredential } from "./helpers.js";
import { ADMIN } from "./users.js";

// A manually-managed context/page (rather than the `page` test fixture) is
// used here, and in every other file that performs a *fresh* login, because
// we need a handle on `context` itself to call `context.storageState()`
// after the relevant state-changing step (sign-in + settings here; sign-in +
// forced password change in the role-onboarding files). Files that only ever
// *resume* an already-saved session (most of files 02 onward) don't need
// this — they just declare `test.use({ storageState: ... })` and use the
// default `page` fixture, which is simpler.
let context;
let page;

test.beforeAll(async ({ browser }) => {
  context = await browser.newContext();
  page = await context.newPage();
});

test.afterAll(async () => {
  await context?.close();
});

test("admin: bootstrap the first administrator account", async () => {
  await page.goto("/");

  // A pristine instance shows the Setup screen (needs_setup === true) instead
  // of Login — this is the one-time flow described in the user guide for
  // standing up a new Zerf instance: the very first account created always
  // becomes an administrator, there is no separate "invite the first admin"
  // step.
  await expect(page.locator("#setup-email")).toBeVisible();

  await page.locator("#setup-first-name").fill(ADMIN.firstName);
  await page.locator("#setup-last-name").fill(ADMIN.lastName);
  await page.locator("#setup-email").fill(ADMIN.email);
  await page.locator("#setup-password").fill(ADMIN.password);
  await page.locator("#setup-confirm").fill(ADMIN.password);
  await page.getByRole("button", { name: "Create admin account" }).click();

  // After setup the app drops to the login screen with the email prefilled
  // (App.svelte's onComplete callback stores the email and switches the
  // boot-time view from Setup to Login) — the admin still has to sign in
  // with the password just chosen, setup does not auto-login.
  await expect(page.locator("#email")).toBeVisible();
});

test("admin: sign in and complete first-run settings", async () => {
  await page.locator("#email").fill(ADMIN.email);
  await page.locator("#password").fill(ADMIN.password);
  await page.getByRole("button", { name: "Sign in" }).click();

  // A brand-new admin always has must_configure_settings=true until country,
  // default weekly hours, and default annual leave days are all set — the
  // SPA's router (App.svelte resolveRoute) redirects any route to
  // /settings/general until that's done, even if the admin tries to go
  // straight to e.g. /dashboard. This is the app's own "you can't use Zerf
  // until basic settings exist" guard, not something this test enforces.
  await page.waitForURL("**/settings/general");

  // AdminSettings.svelte's load() fetches both the current settings object
  // and the Nager.Date country list asynchronously, then *replaces* the
  // whole local form object (settingsForm = loadedSettings) once both
  // resolve. If we started typing into the form before that replacement
  // landed, our input would be silently wiped. Waiting for the Germany
  // option to exist in the <select> is a reliable signal that the country
  // list (and therefore the whole load) has finished.
  await page
    .locator('#settings-country option[value="DE"]')
    .waitFor({ state: "attached" });

  // Germany is chosen because the Nager.Date public-holiday API (which the
  // backend calls to seed the `holidays` table) always has Germany.
  await page.locator("#settings-country").selectOption("DE");
  await page.locator("#settings-language").selectOption("en");
  await page.locator("#settings-default-hours").fill("40");
  await page.locator("#settings-default-leave").fill("30");

  // Selecting a country kicks off an async fetch of that country's regions
  // (for region-specific holiday calendars); the Save button is disabled
  // for the duration (`disabled={saving || regionLoading}`), so waiting for
  // it to become enabled again avoids racing a save against that fetch.
  const saveButton = page.getByRole("button", { name: "Save Changes" });
  await expect(saveButton).toBeEnabled();
  await saveButton.click();

  await expect(page.getByText("Settings saved.")).toBeVisible();

  // From this point on, must_configure_settings is false and the admin can
  // reach any route. Persist the credential (for completeness/symmetry with
  // the other roles — the admin never has a temporary password to look up)
  // and snapshot the authenticated session so every later admin-acting spec
  // file can resume it directly via `test.use({ storageState: ... })`.
  writeCredential("admin", ADMIN.email, ADMIN.password);
  await context.storageState({ path: storageStatePath("admin") });
});
