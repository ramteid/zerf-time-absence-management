// File 11: verifies the cumulative outcome of every approve / reject /
// cancel / reopen-reject decision *through the UI of the affected user* — the
// persistent status the employee and assistant actually see, not the
// transient toast the reviewer saw at the moment of acting, and explicitly
// not the audit log. This is the "additionally via the UI" confirmation that
// the whole approval chain (specs 06, 08, 10) genuinely changed state.
//
// It must run after 10 (the last review) and before 12 (which archives the
// employee, invalidating her session), hence the file number.

import { test, expect } from "@playwright/test";
import { storageStatePath } from "./helpers.js";

test.describe("employee sees the final state of their items", () => {
  test.use({ storageState: storageStatePath("employee") });

  test("employee: approved week stayed approved after the reopen was rejected", async ({
    page,
  }) => {
    await page.goto("/time");
    await page.locator(".time-week-picker button").first().click();
    await expect(page.locator(".week-grid")).toBeVisible();
    // The reopen request was *rejected* in 10, so per the user guide the
    // week is left untouched: its entries are still "Approved", never reset
    // to draft. Seeing the Approved chips here (rather than editable draft
    // blocks) is the employee-side UI proof of that rejection's effect.
    await expect(
      page.locator(".week-grid").getByText("Approved").first(),
    ).toBeVisible();
  });

  test("employee: the approved vacation now shows as cancelled", async ({
    page,
  }) => {
    await page.goto("/absences");
    // Full lifecycle visible in one row's status chip: requested (05) →
    // approved (06) → cancellation requested (09) → cancellation approved
    // (10) → "Cancelled". The day-off request stayed "Rejected" (06).
    await expect(
      page.locator(".absence-entry", { hasText: "E2E vacation" }),
    ).toContainText("Cancelled");
    await expect(
      page.locator(".absence-entry", { hasText: "E2E day off" }),
    ).toContainText("Rejected");
  });
});

test.describe("assistant sees the final state of their items", () => {
  test.use({ storageState: storageStatePath("assistant") });

  test("assistant: week and absence both show as approved", async ({ page }) => {
    await page.goto("/time");
    await page.locator(".time-week-picker button").first().click();
    await expect(page.locator(".week-grid")).toBeVisible();
    await expect(
      page.locator(".week-grid").getByText("Approved").first(),
    ).toBeVisible();

    await page.goto("/absences");
    await expect(
      page.locator(".absence-entry", { hasText: "E2E assistant absence" }),
    ).toContainText("Approved");
  });
});
