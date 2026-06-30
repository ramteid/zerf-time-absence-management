// File 10: the team lead's final review pass over the two deferred requests
// 09 created — reject the reopen request (so the week stays locked, per the
// user guide's "on rejection the week remains unchanged"), then approve the
// pending vacation cancellation. Rejecting one and approving the other
// (rather than approving both) keeps both outcomes of "review a deferred
// request" covered across the suite, mirroring 06's approve-one/reject-one
// pattern for ordinary absence requests.

import { test, expect } from "@playwright/test";
import { storageStatePath } from "./helpers.js";

test.use({ storageState: storageStatePath("team_lead") });

test("team lead: reject the reopen request", async ({ page }) => {
  await page.goto("/dashboard");

  // Reopen requests render in the same "Week Approvals" card as pending
  // weeks, but with "wants to edit {week}" text instead of a total-hours
  // summary — filtering on that phrase picks the reopen row specifically.
  const reopenRow = page
    .locator(".dashboard-click-row")
    .filter({ hasText: "wants to edit" });
  await expect(reopenRow).toHaveCount(1);
  // Open the detail dialog (ReopenReviewDialog) rather than using the row's
  // inline Approve/Reject icons directly — both paths end up calling the
  // same Dashboard.svelte rejectReopen()/approveReopen() functions, so this
  // exercises the detail-view path specifically for coverage variety.
  await reopenRow.click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Edit Request Details")).toBeVisible();
  await dialog.getByRole("button", { name: "Reject" }).click();

  // Same as any other rejection in this suite: a reason is mandatory.
  const confirm = page.getByRole("dialog");
  await confirm.locator("#confirm-reason").fill("E2E: please resubmit next week");
  await confirm.getByRole("button", { name: "Reject" }).click();
  await expect(page.getByText("Rejected.")).toBeVisible();
  // The week's entries remain in their current (approved) state — rejecting
  // a reopen request does not reset anything, unlike approving one would.
});

test("team lead: approve the absence cancellation request", async ({ page }) => {
  await page.goto("/dashboard");

  // A cancellation_pending absence shows up in the exact same "Absence
  // Requests" queue as an ordinary pending request — ApprovalQueues.svelte
  // just gives it a different chip label ("Cancellation" vs. the regular
  // pending styling). The same Approve button finalizes it either way; on
  // the backend this transitions the absence to "cancelled" rather than
  // "approved" (it was already approved once before being cancelled).
  const cancellationRow = page.locator(".absence-row", {
    hasText: "E2E vacation",
  });
  await expect(cancellationRow).toHaveCount(1);
  await cancellationRow.locator("button").first().click();
  await expect(page.getByText("Approved.")).toBeVisible();
  // UI-state confirmation: the cancellation request leaves the queue; the
  // employee-side outcome (vacation now shows "Cancelled") is verified from
  // the employee's own UI in 11-final-ui-state.spec.js.
  await expect(cancellationRow).toHaveCount(0);
});
