// Post-restore UI verification, invoked by backup-restore-check.sh after the
// database has been restored from a backup. The bash check already confirms
// the restore at the data layer (every table's row count matches the
// pre-backup snapshot, the post-backup mutation is gone). This adds the
// "through the UI" half: it drives a real browser to prove the restored
// instance is genuinely usable — the admin can sign in and the restored
// users are actually rendered in the app — rather than inferring that from
// row counts alone.
//
// ADMIN is imported from the same single source of truth the Playwright
// specs use, so there is no duplicated credential here. The admin password
// is never changed during the suite, so it still works against the restored
// database (the backup was taken at the very end of the run).
//
// Usage: node post-restore-ui-check.mjs <base-url>

import { chromium } from "@playwright/test";
import { ADMIN } from "./tests/users.js";

const baseURL = process.argv[2];
if (!baseURL) {
  console.error("usage: node post-restore-ui-check.mjs <base-url>");
  process.exit(1);
}

const browser = await chromium.launch();
const page = await (await browser.newContext()).newPage();
try {
  await page.goto(baseURL);
  await page.locator("#email").fill(ADMIN.email);
  await page.locator("#password").fill(ADMIN.password);
  await page.getByRole("button", { name: "Sign in" }).click();

  // Login.svelte performs window.location.assign() once the session is
  // established. A race between that navigation and a page.goto() call
  // on the same page object causes the subsequent URL to be unpredictable.
  // Waiting for the dashboard URL is the definitive signal that login has
  // completed and the session is live.
  await page.waitForURL("**/dashboard", { timeout: 15000 });

  // The team members created during the run must be present in the restored
  // database and rendered in the UI. waitFor throws on timeout, which exits
  // non-zero and fails the calling bash check.
  await page.goto(`${baseURL}/settings/users`);
  await page.getByText("Tom Lead").waitFor({ timeout: 15000 });
  await page.getByText("Amy Assistant").waitFor({ timeout: 15000 });

  console.log(
    "  ok: admin signed in via the UI and the restored team members are rendered.",
  );
} finally {
  await browser.close();
}
