// File 11a: the Employee report, viewed by someone who is looking at *another*
// person — the path a team lead or admin uses to review an individual.
//
// Every earlier spec views a report as the person it belongs to, and every
// absence the suite books lies in the future. That combination left the report's
// flextime chart untested for the one case that actually broke in production:
// a chart whose period contains an absence day. Colouring such a day is the only
// branch that reads the absence-category list, and it threw, which aborted the
// render of the whole report — the employee could be picked from the dropdown
// but their report stayed empty. Reports for people without absences kept
// working, so it looked like only certain employees were affected.
//
// Runs after 11 (all approvals are final) and before 12 (which archives the
// employee and would remove her from the report dropdown).

import { test, expect } from "@playwright/test";
import {
  collectPageErrors,
  pastBookableDateOffset,
  setDate,
  storageStatePath,
} from "./helpers.js";
import { ASSISTANT, EMPLOYEE, TEAM_LEAD } from "./users.js";

// The past workday the employee calls in sick on. Chosen in the first test and
// read by the ones after it — the suite runs in a single worker, so a
// module-level value is shared safely across the describe blocks below.
let sickDate;

test.describe("employee books a past absence that lands in the report period", () => {
  test.use({ storageState: storageStatePath("employee") });

  test("employee: a past sick day is approved without review", async ({
    page,
  }) => {
    sickDate = await pastBookableDateOffset(page.request);

    await page.goto("/absences");
    await page.getByRole("button", { name: "Request Absence" }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();

    // "Sick" is seeded with auto_approve_past, so a sick day in the past needs
    // no approver — which is what puts an *approved* absence inside the report
    // period without another round-trip through the team lead.
    await dialog.locator("#absence-kind").selectOption({ label: "Sick" });
    await setDate(page, "absence-start-date", sickDate);
    await setDate(page, "absence-end-date", sickDate);
    await dialog.locator("#absence-comment").fill("E2E past sick day");
    await dialog.getByRole("button", { name: "Submit Request" }).click();
    await expect(dialog).toBeHidden();

    await expect(
      page.locator(".absence-entry", { hasText: "E2E past sick day" }),
    ).toContainText("Approved");
  });
});

test.describe("team lead reads another person's employee report", () => {
  test.use({ storageState: storageStatePath("team_lead") });

  test("team lead: the employee's report renders with a banded flextime chart", async ({
    page,
  }) => {
    const pageErrors = collectPageErrors(page);

    await page.goto("/reports");
    const userSelect = page.locator("#reports-user-select");
    await expect(userSelect).toBeVisible();

    // Pick the employee by the label the dropdown actually shows, so the test
    // doesn't depend on user ids or on the roster's ordering.
    await userSelect.selectOption({
      label: `${EMPLOYEE.firstName} ${EMPLOYEE.lastName}`,
    });

    // Cover the sick day with an explicit custom range rather than relying on
    // the default month: near the start of a month the current month may hold
    // no past workday at all, which would silently drop the absence out of the
    // chart and make this test pass for the wrong reason.
    await page.getByRole("button", { name: "Custom range" }).click();
    await setDate(page, "reports-period-from", sickDate);
    await setDate(page, "reports-period-to", sickDate);

    // The chart aborted before drawing anything, so assert the absence band
    // itself is there — "the flextime section is visible" would still have
    // passed, because the section's own card renders before the chart inside it.
    const chart = page.locator(".chart-root svg");
    await expect(chart).toBeVisible();
    await expect(chart.getByTestId("flextime-band").first()).toBeVisible();

    expect(pageErrors).toEqual([]);
  });

  test("team lead: every person in the dropdown renders a report", async ({
    page,
  }) => {
    const pageErrors = collectPageErrors(page);

    await page.goto("/reports");
    const userSelect = page.locator("#reports-user-select");
    await expect(userSelect).toBeVisible();

    // The dropdown lists exactly the people this lead may report on. Walking
    // all of them catches a report that only breaks for someone with a
    // particular data shape, instead of trusting that one sample is
    // representative.
    const labels = await userSelect.locator("option").allInnerTexts();
    expect(labels.length).toBeGreaterThan(1);

    for (const label of labels) {
      await userSelect.selectOption({ label });
      // Every report opens with a row of stat cards, whatever the person's
      // role — their absence is the symptom the production bug produced.
      await expect(page.locator(".stat-card").first()).toBeVisible();
      expect(pageErrors, `report failed for ${label}`).toEqual([]);
    }

    // The lead's own team is what the roster is scoped to, so both the
    // employee and the assistant they approve for must be selectable here.
    expect(labels.join(" ")).toContain(EMPLOYEE.lastName);
    expect(labels.join(" ")).toContain(ASSISTANT.lastName);
    expect(labels.join(" ")).toContain(TEAM_LEAD.lastName);
  });
});
