// File 11c: the flextime account — the admin surface that replaced the old
// editable "overtime start balance" field on the user profile.
//
// The rule this file pins down is the reason the feature exists: a balance
// correction is a *dated* entry. It moves the balance from its date onwards
// and leaves everything before it exactly as it was, so an admin can no
// longer rewrite years of reported balances by retyping one number.
//
// Runs after 11b (all balances the earlier specs assert on are already
// checked) and before 12, which archives the employee and removes her from
// both the active roster and the report dropdown.

import { test, expect } from "@playwright/test";
import {
  collectPageErrors,
  createUserViaAdminUi,
  pastBookableDateOffset,
  setDate,
  storageStatePath,
} from "./helpers.js";
import { ASSISTANT, EMPLOYEE, TEAM_LEAD } from "./users.js";

// The past workday the correction takes effect on, and the balance the
// account showed before it. Chosen in the first test and read by the ones
// after it — the suite runs in a single worker, so module-level state is
// shared safely across describe blocks.
let correctionDate;
let balanceBefore;

// AdminUsers renders each roster entry as a direct child <div> of the
// `.zf-card` list container.
function userRow(page, fullName) {
  return page.locator(".zf-card > div", { hasText: fullName });
}

// The signed HH:MM balance shown at the top right of the dialog.
async function readBalance(dialog) {
  return (await dialog.locator(".account-balance").innerText()).trim();
}

test.describe("admin enters a carry-in balance while onboarding a new employee", () => {
  test.use({ storageState: storageStatePath("admin") });

  // This is the journey the whole feature exists for: the carry-in balance is
  // still asked for at account creation (per the user guide), but typing a
  // number into that field must now produce a dated ledger booking — not the
  // old mutable profile setting — so that editing it later can never again
  // rewrite the person's whole reported history.
  test("admin: the value typed at creation becomes the account's opening entry", async ({
    page,
  }) => {
    const pageErrors = collectPageErrors(page);
    const startDateOffsetDays = -5;

    await createUserViaAdminUi(page, {
      firstName: "Ossi",
      lastName: "Opening",
      email: "opening.e2e@example.com",
      role: "employee",
      approverEmail: TEAM_LEAD.email,
      startDateOffsetDays,
      openingBalanceHours: "12.5",
    });

    await page.goto("/settings/users");
    const row = userRow(page, "Ossi Opening");
    await row.getByRole("button", { name: "Flextime account" }).click();

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();

    // Exactly one entry: the carry-in, booked as "Hours brought along" rather
    // than a correction, for the exact amount that was typed in.
    const entries = dialog.locator(".adjustment-row");
    await expect(entries).toHaveCount(1);
    await expect(entries.first()).toContainText("Hours brought along");
    await expect(entries.first()).toContainText("+12:30");

    // Nothing has been worked yet, so the balance is exactly the carry-in.
    // ("h" with no space is the English hour-unit formatting — see
    // formatHours/hoursUnit in i18n.js.)
    expect(await readBalance(dialog)).toBe("+12:30h");

    expect(pageErrors).toEqual([]);
  });

  test("admin: assistants are never offered a flextime account", async ({
    page,
  }) => {
    // Assistants have no work target and no flextime account at all, so the
    // button that opens one must not appear for their row — offering it would
    // only lead to the "no flextime account" dead end the dialog itself shows.
    await page.goto("/settings/users");
    const row = userRow(page, `${ASSISTANT.firstName} ${ASSISTANT.lastName}`);
    await expect(row).toBeVisible();
    await expect(row.getByTitle("Flextime account")).toHaveCount(0);
  });
});

test.describe("admin corrects an employee's flextime balance", () => {
  test.use({ storageState: storageStatePath("admin") });

  test("admin: the flextime account opens from the user roster", async ({
    page,
  }) => {
    const pageErrors = collectPageErrors(page);
    correctionDate = await pastBookableDateOffset(page.request);

    await page.goto("/settings/users");
    const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
    await row.getByRole("button", { name: "Flextime account" }).click();

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByText(`${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`),
    ).toBeVisible();

    // No carry-in balance was entered when the employee was created (see
    // createUserViaAdminUi), so the ledger starts out empty and whatever
    // balance is shown comes purely from the hours booked in earlier specs.
    await expect(
      dialog.locator(".field-hint", { hasText: "No entries yet" }),
    ).toBeVisible();
    balanceBefore = await readBalance(dialog);
    expect(balanceBefore).toMatch(/^[+-]\d+:\d{2}/);

    expect(pageErrors).toEqual([]);
  });

  test("admin: a correction is booked and listed with its date and note", async ({
    page,
  }) => {
    await page.goto("/settings/users");
    const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
    await row.getByRole("button", { name: "Flextime account" }).click();

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();

    await setDate(page, "flextime-adjustment-date", correctionDate);
    await dialog.locator("#flextime-adjustment-hours").fill("-2.5");
    await dialog
      .locator("#flextime-adjustment-reason")
      .fill("E2E overtime payout");

    const created = page.waitForResponse(
      (r) =>
        /\/api\/v1\/users\/\d+\/flextime-adjustments$/.test(
          new URL(r.url()).pathname,
        ) && r.request().method() === "POST",
    );
    await dialog.getByRole("button", { name: "Add entry" }).click();
    expect((await created).ok()).toBe(true);

    // The entry, its note, and the balance that now includes it.
    const entry = dialog.locator(".adjustment-row", {
      hasText: "E2E overtime payout",
    });
    await expect(entry).toBeVisible();
    await expect(entry).toContainText("-2:30");

    const balanceAfter = await readBalance(dialog);
    expect(balanceAfter).not.toEqual(balanceBefore);
  });

  test("admin: the entry survives a reload", async ({ page }) => {
    // Re-opening the dialog re-fetches from the API, so this proves the entry
    // was persisted rather than only rendered optimistically.
    await page.goto("/settings/users");
    const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
    await row.getByRole("button", { name: "Flextime account" }).click();

    const dialog = page.getByRole("dialog");
    await expect(
      dialog.locator(".adjustment-row", { hasText: "E2E overtime payout" }),
    ).toBeVisible();
  });

  test("admin: an entry is cancelled, never deleted", async ({ page }) => {
    // Uses a throwaway entry of its own so the correction the employee test
    // below looks at stays in force.
    await page.goto("/settings/users");
    const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
    await row.getByRole("button", { name: "Flextime account" }).click();

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    const balanceBeforeMistake = await readBalance(dialog);

    await setDate(page, "flextime-adjustment-date", correctionDate);
    await dialog.locator("#flextime-adjustment-hours").fill("4");
    await dialog.locator("#flextime-adjustment-reason").fill("E2E typo");
    await dialog.getByRole("button", { name: "Add entry" }).click();

    const mistake = dialog.locator(".adjustment-row", { hasText: "E2E typo" });
    await expect(mistake).toBeVisible();
    expect(await readBalance(dialog)).not.toEqual(balanceBeforeMistake);

    // Cancelling books the opposite amount rather than removing the row.
    await mistake.getByRole("button", { name: "Cancel entry" }).click();
    const confirm = page.getByRole("dialog").last();
    await confirm.getByRole("button", { name: "Cancel entry" }).click();

    // The balance is back, the mistake is still on the record and marked, and
    // the cancellation stands next to it.
    await expect(dialog.locator(".adjustment-row", { hasText: "E2E typo" }))
      .toContainText("Cancelled");
    await expect(
      dialog.locator(".adjustment-row", { hasText: "Cancellation" }),
    ).toBeVisible();
    await expect
      .poll(async () => await readBalance(dialog))
      .toBe(balanceBeforeMistake);
  });

  test("admin: the user dialog no longer offers the balance as a setting", async ({
    page,
  }) => {
    // The whole point of the change: the number cannot be edited on the
    // profile any more, because doing so used to move every balance ever
    // reported for this person at once.
    await page.goto("/settings/users");
    const row = userRow(page, `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`);
    // Edit is the first action button in the row and carries no title, so it
    // is addressed by position rather than by accessible name.
    await row.locator("button").first().click();

    const dialog = page.getByRole("dialog");
    await expect(dialog.getByText("Edit User")).toBeVisible();
    await expect(dialog.locator("#user-opening-balance")).toHaveCount(0);
    await expect(
      dialog.locator(".field-hint", { hasText: "flextime account" }),
    ).toBeVisible();
  });
});

test.describe("the employee's own views are unaffected by the correction", () => {
  test.use({ storageState: storageStatePath("employee") });

  test("employee: the report still renders for the corrected day", async ({
    page,
  }) => {
    // The switch to a dated ledger is meant to be invisible in the employee's
    // own views: no new section, no new controls, nothing to acknowledge. What
    // must hold is that a period containing an admin booking still renders —
    // the balance simply reflects it, and hovering that day in the chart names
    // it. A render error here would show up as a silently missing chart.
    const pageErrors = collectPageErrors(page);

    await page.goto("/reports");

    // Pin the report to the correction's own day. Relying on the default
    // month would make the test depend on which day of the month it runs on.
    await page.getByRole("button", { name: "Custom range" }).click();
    await setDate(page, "reports-period-from", correctionDate);

    const flextimeLoaded = page.waitForResponse((r) => {
      const url = new URL(r.url());
      return (
        url.pathname === "/api/v1/reports/flextime" &&
        url.searchParams.get("from") === correctionDate &&
        url.searchParams.get("to") === correctionDate
      );
    });
    await setDate(page, "reports-period-to", correctionDate);
    const flextimeResponse = await flextimeLoaded;
    expect(flextimeResponse.ok()).toBe(true);

    // The booking reaches the employee's own ledger — the number is theirs,
    // even though nothing in their UI announces where it came from.
    const ledger = await flextimeResponse.json();
    expect(ledger.days[0].adjustment_min).toBe(-150);

    await expect(page.locator(".chart-root svg")).toBeVisible();
    // No section was added to the employee's report for this.
    await expect(page.locator(".flextime-adjustments")).toHaveCount(0);

    expect(pageErrors).toEqual([]);
  });
});
