// File 8: the team lead reviews the assistant's submissions from 07 — the
// exact same Dashboard approval surface used for the employee in 06, just
// for a user whose approver relationship was implicit (set automatically
// when Tom created Amy via /team-users in 04, rather than picked from an
// approver checklist as Eve's was in 02). Approving here proves that
// implicit relationship actually grants Tom approval rights over Amy.

import { test, expect } from "@playwright/test";
import { storageStatePath } from "./helpers.js";
import { ASSISTANT } from "./users.js";

test.use({ storageState: storageStatePath("team_lead") });

test("team lead: approve the assistant's week and absence", async ({ page }) => {
  await page.goto("/dashboard");

  // Only Amy's week is pending at this point — Eve's was already resolved
  // in 06, and nobody else has submitted anything since.
  const weekRow = page
    .locator(".dashboard-click-row")
    .filter({ hasText: `${ASSISTANT.firstName} ${ASSISTANT.lastName}` });
  await expect(weekRow).toHaveCount(1);
  await weekRow.locator("button").first().click();
  await expect(page.getByText("Approved.")).toBeVisible();
  // UI-state confirmation: the approved week leaves the pending queue.
  await expect(weekRow).toHaveCount(0);

  const absenceRow = page.locator(".absence-row", {
    hasText: "E2E assistant absence",
  });
  await expect(absenceRow).toHaveCount(1);
  await absenceRow.locator("button").first().click();
  await expect(page.getByText("Approved.")).toBeVisible();
  // UI-state confirmation: the approved absence leaves the pending queue.
  await expect(absenceRow).toHaveCount(0);
});
