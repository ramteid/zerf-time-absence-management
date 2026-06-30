// File 6: the team lead reviews their report's (the employee's) pending
// submissions from 05 — approve the week, approve one absence, reject
// another with a reason. This is deliberately the team lead's dashboard,
// not the admin's: per docs/user-guide.md, a non-admin team lead can only
// act on users explicitly assigned to them, and Eve's approver was set to
// Tom (not the admin) back in 02-admin-create-users.spec.js specifically so
// this approval path is exercised by the team lead role, distinct from
// 11's admin-driven user-management operations.

import { test, expect } from "@playwright/test";
import { storageStatePath } from "./helpers.js";
import { EMPLOYEE } from "./users.js";

// Resumes Tom's session saved in 04-team-lead-onboarding.spec.js.
test.use({ storageState: storageStatePath("team_lead") });

test("team lead: approve the employee's week", async ({ page }) => {
  await page.goto("/dashboard");

  // Exactly one pending week is expected: Eve's, submitted in 05. No other
  // user has submitted anything yet at this point in the suite.
  const weekRow = page
    .locator(".dashboard-click-row")
    .filter({ hasText: `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}` });
  await expect(weekRow).toHaveCount(1);

  // "Approve All" batch-approves every pending week in one action — using it
  // here (rather than the per-row approve button) exercises that bulk path,
  // which the individual-row approve/reject buttons used elsewhere in the
  // suite do not.
  await page.getByRole("button", { name: "Approve All" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Approve all" })
    .click();
  await expect(page.getByText("All approved.")).toBeVisible();
  // UI-state confirmation (beyond the toast): the approved week leaves the
  // pending queue, so the Week Approvals card flips to its empty state and
  // the employee's row is gone.
  await expect(weekRow).toHaveCount(0);
  await expect(page.getByText("All caught up!")).toBeVisible();
});

test("team lead: approve one absence and reject another", async ({ page }) => {
  await page.goto("/dashboard");
  // Two pending absences are expected here: "E2E vacation" and "E2E day
  // off" from 05 ("E2E training" was self-cancelled by Eve before it ever
  // reached this queue, so it never shows up here).
  await expect(page.locator(".absence-row")).toHaveCount(2);

  // Each absence row has two icon buttons: Approve (Check, first) and Reject
  // (X, second) — see ApprovalQueues.svelte. Approve the vacation request...
  const vacationRow = page.locator(".absence-row", { hasText: "E2E vacation" });
  await vacationRow.locator("button").first().click();
  await expect(page.getByText("Approved.")).toBeVisible();

  // ...and reject the other one, to exercise both outcomes of a review (not
  // just "approve everything", which 06's first test and 08 already cover
  // via "Approve All" / single approve). Rejecting requires a reason — the
  // Confirm.svelte dialog's #confirm-reason textarea is mandatory here.
  const dayOffRow = page.locator(".absence-row", { hasText: "E2E day off" });
  await dayOffRow.locator("button").nth(1).click();
  const confirm = page.getByRole("dialog");
  await confirm.locator("#confirm-reason").fill("E2E: rejecting on purpose");
  await confirm.getByRole("button", { name: "Reject" }).click();
  await expect(page.getByText("Rejected.")).toBeVisible();

  // Both pending absences are now resolved (one approved, one rejected) —
  // the queue should be empty.
  await expect(page.locator(".absence-row")).toHaveCount(0);
  await expect(page.getByText("No pending requests")).toBeVisible();
});
