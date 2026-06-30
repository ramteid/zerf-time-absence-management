// File 9: the employee triggers the two "needs approver review" paths that
// 05's straightforward cancel-while-pending didn't cover — requesting to
// reopen an already-approved week (to fix a mistake after the fact) and
// requesting cancellation of an already-approved absence. Per
// docs/user-guide.md, both differ from their "still pending" counterparts
// specifically because the data has already been approved: a reopen
// unlocks entries that were already submitted/approved/rejected, and an
// approved absence's cancellation is deferred into a
// "cancellation_pending" status rather than applied immediately. Both are
// left pending here for 10-team-lead-final-reviews.spec.js to act on.

import { test, expect } from "@playwright/test";
import { storageStatePath } from "./helpers.js";

// Resumes Eve's session saved in 05-employee-workflows.spec.js.
test.use({ storageState: storageStatePath("employee") });

test("employee: request to reopen the approved week", async ({ page }) => {
  await page.goto("/time");
  // The same prior week 05 booked entries into and 06 approved — its
  // entries are all status=approved now, which is exactly the precondition
  // TimeWeekHeader's canRequestReopen check needs (some entry in
  // submitted/approved/rejected, and no reopen already pending).
  await page.locator(".time-week-picker button").first().click();
  await expect(page.locator(".week-grid")).toBeVisible();

  // Persistent UI-state proof — not the transient toast 06 saw — that the
  // team lead's approval actually changed the data: the employee's own week
  // grid now renders its entries with the "Approved" status chip (an
  // approved entry is locked, so EntryBlock shows the status label rather
  // than a draft/editable block).
  await expect(
    page.locator(".week-grid").getByText("Approved").first(),
  ).toBeVisible();

  // Reopening always requires a reason — Time.svelte's requestReopen() shows
  // a Confirm.svelte dialog with `reason: true`, which renders the
  // #confirm-reason textarea and blocks the confirm button until it's
  // filled in.
  await page.getByRole("button", { name: "Request edit" }).click();
  const confirm = page.getByRole("dialog");
  await confirm.locator("#confirm-reason").fill("E2E: need to fix an entry");
  await confirm.getByRole("button", { name: "Request edit" }).click();
  await expect(page.getByText("Edit request sent.")).toBeVisible();
});

test("employee: request cancellation of the approved vacation", async ({ page }) => {
  await page.goto("/absences");
  // Persistent UI-state proof of 06's decisions, read from the employee's
  // own absence history: the status chip on each row reflects what the team
  // lead did — the vacation is "Approved", the day-off request is "Rejected"
  // — rather than relying on the toasts that flashed on the team lead's
  // screen back in 06.
  await expect(
    page.locator(".absence-entry", { hasText: "E2E vacation" }),
  ).toContainText("Approved");
  await expect(
    page.locator(".absence-entry", { hasText: "E2E day off" }),
  ).toContainText("Rejected");

  // "E2E vacation" was approved by the team lead in 06; cancelling it now
  // exercises AbsenceDetailDialog's "cancellable" branch for an *approved*
  // absence specifically, which renders a different button label
  // ("Request cancellation") and a different confirmation flow than the
  // still-"requested" cancellation 05 already covered ("Cancel absence").
  await page.locator(".absence-entry", { hasText: "E2E vacation" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Request cancellation" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Yes, request cancellation" })
    .click();
  // The toast wording itself confirms the deferred (not immediate) outcome:
  // the absence's status becomes "cancellation_pending", not "cancelled",
  // until the team lead reviews it in 10.
  await expect(
    page.getByText("Cancellation requested. Your team lead will review it."),
  ).toBeVisible();
});
