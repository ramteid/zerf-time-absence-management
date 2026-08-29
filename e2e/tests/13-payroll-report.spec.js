// File 13: the monthly payroll report — its "who is included" configuration
// and the dashboard card that replaced the old "technical error" email.
//
// Runs last because it onboards one more person (Pia, whose contract is
// backdated into the previous month so the card always has somebody to report
// on) and switches the payroll report on, both of which would otherwise change
// what earlier files see in user lists and on the dashboard.
//
// Deliberately *not* asserted here: the exact traffic-light split. Which
// people fall into which colour depends on the calendar day the suite runs on,
// so the counts are checked for internal consistency only; the per-state
// classification is covered by the backend integration tests
// (payroll_status_counts_everyone_but_anonymizes_outside_a_leads_team).

import { test, expect } from "@playwright/test";
import {
  changeTempPassword,
  createUserViaAdminUi,
  enableSmtpForE2E,
  readCredentials,
  signIn,
  storageStatePath,
  writeCredential,
} from "./helpers.js";
import {
  ADMIN,
  EMPLOYEE,
  PAYROLL_EMPLOYEE,
  PAYROLL_START_OFFSET_DAYS,
  TEAM_LEAD,
} from "./users.js";

// Full "First Last" names, not just the surname: the exclusion list prints
// each person's role after their name, so filtering on "Employee" alone would
// also match every row whose role label reads "(Employee)".
const EMPLOYEE_NAME = `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`;
const TEAM_LEAD_NAME = `${TEAM_LEAD.firstName} ${TEAM_LEAD.lastName}`;
const ADMIN_NAME = `${ADMIN.firstName} ${ADMIN.lastName}`;
const PAYROLL_EMPLOYEE_NAME = `${PAYROLL_EMPLOYEE.firstName} ${PAYROLL_EMPLOYEE.lastName}`;
const PAYROLL_EMPLOYEE_PASSWORD = "PayrollPass123!";

test.describe("payroll report configuration", () => {
  test.use({ storageState: storageStatePath("admin") });

  test("admin: onboard someone whose month the report covers", async ({
    page,
  }) => {
    const password = await createUserViaAdminUi(page, {
      ...PAYROLL_EMPLOYEE,
      role: "employee",
      approverEmail: ADMIN.email,
      startDateOffsetDays: PAYROLL_START_OFFSET_DAYS,
    });
    writeCredential("payroll_employee", PAYROLL_EMPLOYEE.email, password);
  });

  // The payroll report is delivered by email and nothing else, so it refuses
  // to be switched on until email works. Everything before this file runs with
  // email off (see 03-admin-config), which is why this switch lives here, in
  // the file that already runs last.
  test("admin: switch email on so the report can be enabled", async ({
    page,
  }) => {
    await enableSmtpForE2E(page);
  });

  test("admin: turn the payroll report on", async ({ page }) => {
    await page.goto("/settings/payroll-report");

    await page
      .getByLabel("Send the payroll report automatically")
      .check({ force: true });
    await page.locator("#payroll-recipients").fill("payroll@e2e.test");
    await page.locator("#payroll-day").fill("5");
    await page.getByRole("button", { name: "Save" }).click();

    await expect(page.getByText("Settings saved.")).toBeVisible();
  });

  test("admin: only active non-admins are offered for exclusion", async ({
    page,
  }) => {
    await page.goto("/settings/payroll-report");

    const list = page.locator(".check-list");
    await expect(list).toBeVisible();
    // Everyone who can appear in the report is offered...
    await expect(
      list.locator("label", { hasText: EMPLOYEE_NAME }),
    ).toBeVisible();
    await expect(
      list.locator("label", { hasText: TEAM_LEAD_NAME }),
    ).toBeVisible();
    // ...but never the admin, who is excluded from the report unconditionally.
    await expect(list.locator("label", { hasText: ADMIN_NAME })).toHaveCount(0);
    await expect(page.getByText("Administrators never appear")).toBeVisible();
  });

  test("admin: excluding someone survives a reload", async ({ page }) => {
    await page.goto("/settings/payroll-report");

    const employeeBox = () =>
      page
        .locator(".check-list label", { hasText: EMPLOYEE_NAME })
        .locator('input[type="checkbox"]');

    await employeeBox().check();
    await page.getByRole("button", { name: "Save" }).click();
    await expect(page.getByText("Settings saved.")).toBeVisible();

    await page.reload();
    await expect(employeeBox()).toBeChecked();

    // Put them back so the card below reports on the whole team again.
    await employeeBox().uncheck();
    await page.getByRole("button", { name: "Save" }).click();
    await expect(page.getByText("Settings saved.")).toBeVisible();
    await page.reload();
    await expect(employeeBox()).not.toBeChecked();
  });
});

test.describe("month dashboard cards", () => {
  test.use({ storageState: storageStatePath("admin") });

  test("admin: the submissions card summarizes how far the month is", async ({
    page,
  }) => {
    await page.goto("/dashboard");

    const card = page.locator(".submissions-card");
    await expect(card).toBeVisible();
    // "X of Y done" — the actual numbers depend on the calendar day.
    await expect(card).toContainText(/\d+ of \d+ done/);
    // Pia's contract started in the previous month, so somebody is always
    // covered and the ring is never empty.
    await expect(page.locator(".donut-segment").first()).toBeVisible();
  });

  test("admin: the payroll card names what the report holds", async ({
    page,
  }) => {
    await page.goto("/dashboard");

    // Assert the source before the rendering: the card is hidden whenever this
    // endpoint fails or reports the feature as off, and "element not found"
    // alone cannot tell those apart.
    const response = await page.request.get("/api/v1/reports/payroll-content");
    const payload = await response.text();
    expect(
      response.status(),
      `payroll-content responded ${response.status()}: ${payload}`,
    ).toBe(200);
    expect(
      JSON.parse(payload).enabled,
      `the report is switched on, so the card must be live: ${payload}`,
    ).toBe(true);

    const card = page.locator(".payroll-card");
    await expect(card).toBeVisible();
    await expect(card).toContainText(/\d+ absences · \d+ people with hours/);
  });

  test("admin: the help text names the send day and the retry rule", async ({
    page,
  }) => {
    await page.goto("/dashboard");

    await page
      .locator(".payroll-card")
      .getByTitle(/payroll report/i)
      .click();

    const help = page.locator(".dashboard-help");
    await expect(help).toContainText("day 5 of the month");
    await expect(help).toContainText("checked again every night");
  });

  test("admin: the card opens a per-person list that links into the report", async ({
    page,
  }) => {
    await page.goto("/dashboard");
    await page.locator(".submissions-card-button").click();

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    const row = dialog.locator(".payroll-row-link", {
      hasText: PAYROLL_EMPLOYEE_NAME,
    });
    await expect(row).toBeVisible();

    await row.click();
    // Lands on that person's report for the reported month.
    await expect(page).toHaveURL(/\/reports\?user=\d+&from=[\d-]+&to=[\d-]+/);
  });
});

test.describe("month card visibility", () => {
  test("team lead: sees the card", async ({ browser }) => {
    const context = await browser.newContext({
      storageState: storageStatePath("team_lead"),
    });
    const page = await context.newPage();
    await page.goto("/dashboard");

    await expect(page.locator(".submissions-card")).toBeVisible();
    await expect(page.locator(".payroll-card")).toBeVisible();
    await context.close();
  });

  // Pia signs in herself rather than reusing Eve's stored session: 12's
  // password reset invalidated Eve's, and a payroll-specific account keeps
  // this file independent of what earlier files did to the shared employee.
  test("employee: signs in and does not see the card", async ({ browser }) => {
    const context = await browser.newContext();
    const page = await context.newPage();
    const { payroll_employee: credentials } = readCredentials();
    await signIn(page, credentials.email, credentials.password);
    await changeTempPassword(
      page,
      context,
      "payroll_employee",
      PAYROLL_EMPLOYEE.email,
      PAYROLL_EMPLOYEE_PASSWORD,
    );

    await page.goto("/dashboard");
    // Wait for the dashboard to actually render before asserting an absence,
    // so this cannot pass simply because the page had not loaded yet.
    await expect(
      page.getByRole("heading", { name: "Dashboard" }),
    ).toBeVisible();
    await expect(page.locator(".submissions-card")).toHaveCount(0);
    await expect(page.locator(".payroll-card")).toHaveCount(0);
    await context.close();
  });
});
