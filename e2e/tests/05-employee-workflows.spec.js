// File 5: the employee's day-to-day operations — booking, editing, and
// deleting time entries, submitting a week for approval, requesting three
// absences of different kinds, cancelling one while it's still pending, and
// the read-only Reports/Calendar views. This is the longest file in the
// suite because it's also the one that most directly maps to what the user
// guide calls the regular "weekly process" for a non-admin user.
//
// State this file leaves behind for later files to build on:
//   - one submitted (not yet approved) week, reviewed in 06
//   - "E2E vacation" (Vacation) and "E2E day off" (the custom no-cost
//     category from 03) left pending for the team lead to review in 06
//   - "E2E training" cancelled immediately (never reaches anyone's queue)

import { test, expect } from "@playwright/test";
import { EMPLOYEE, NO_COST_ABSENCE_CATEGORY } from "./users.js";
import {
  changeTempPassword,
  isoOffset,
  readCredentials,
  setDate,
  setTime,
  signIn,
} from "./helpers.js";

const EMPLOYEE_PASSWORD = "EmployeePass456!";

// Manual context/page: this file performs Eve's first login (using her
// temporary password from credentials.json) and needs the same authenticated
// session across all of its tests — see the identical reasoning in
// 04-team-lead-onboarding.spec.js.
let context;
let page;

test.beforeAll(async ({ browser }) => {
  context = await browser.newContext();
  page = await context.newPage();
  const { employee } = readCredentials();
  await signIn(page, employee.email, employee.password);
});

test.afterAll(async () => {
  await context?.close();
});

test("employee: sign in and change the temporary password", async () => {
  await expect(page.getByText("Please change your password.")).toBeVisible();
  await changeTempPassword(
    page,
    context,
    "employee",
    EMPLOYEE.email,
    EMPLOYEE_PASSWORD,
  );
});

test("employee: book, edit, and delete time entries, then submit the week", async () => {
  await page.goto("/time");
  // /time defaults to the current week, but the user guide is explicit that
  // an entry's date can never be in the future — move back one week so every
  // weekday in view is safely in the past before adding anything.
  await page.locator(".time-week-picker button").first().click();
  await expect(page.locator(".week-grid")).toBeVisible();

  // Every day card has an "Add" button that's disabled when the day isn't
  // bookable (future, before the user's start date, a holiday, or an
  // absence day) — per DayCard.svelte's canAddEntryForDay. All three entries
  // below land on the same (first) weekday of the chosen week.
  const addButtons = page.locator(".day-add-btn button:not([disabled])");
  await expect(addButtons.first()).toBeVisible();

  // Entry 1: keep the dialog's own defaults (08:00–12:00) — no need to
  // exercise the time picker for the very first entry.
  await addButtons.first().click();
  let dialog = page.getByRole("dialog");
  await expect(dialog.locator("#entry-comment")).toBeVisible();
  await dialog.locator("#entry-comment").fill("E2E morning shift");
  await dialog.getByRole("button", { name: "Add Entry" }).click();
  await expect(dialog).toBeHidden();

  // Entry 2: a different, non-overlapping range (13:00–16:30) to actually
  // exercise the custom TimePicker widget instead of relying on defaults.
  await addButtons.first().click();
  dialog = page.getByRole("dialog");
  await expect(dialog.locator("#entry-comment")).toBeVisible();
  // Set the end time before the start time: setting the start first (to
  // 13:00) would briefly leave start >= the still-default end (12:00),
  // tripping EntryDialog's "bump end to stay after start" reactive guard and
  // re-rendering the end-time picker out from under us mid-interaction.
  await setTime(page, "entry-end-time", "16:30");
  await setTime(page, "entry-start-time", "13:00");
  // Deliberately named "(draft)" for now — renamed once edited below, so the
  // two phases of this entry's lifecycle are distinguishable while debugging
  // a failed run from a screenshot/trace.
  await dialog.locator("#entry-comment").fill("E2E afternoon shift (draft)");
  await dialog.getByRole("button", { name: "Add Entry" }).click();
  await expect(dialog).toBeHidden();

  // Edit entry 2's comment before submitting — proving a draft entry can be
  // reopened and changed (per the user guide, only `draft` entries are
  // editable; these all still are, since nothing has been submitted yet).
  // EntryBlock.svelte never renders the comment text in the week grid (only
  // the category and the time range), so the entry must be located by its
  // visible time-range text ("13:00 - 16:30") rather than by comment.
  await page.getByText("13:00 - 16:30").click();
  dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Edit Entry")).toBeVisible();
  await dialog.locator("#entry-comment").fill("E2E afternoon shift");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toBeHidden();

  // Entry 3: a throwaway entry (17:00–18:00), added purely to be deleted —
  // proving the delete path (and its confirmation prompt) works on a draft
  // entry before the week is ever submitted.
  await addButtons.first().click();
  dialog = page.getByRole("dialog");
  await expect(dialog.locator("#entry-comment")).toBeVisible();
  await setTime(page, "entry-end-time", "18:00");
  await setTime(page, "entry-start-time", "17:00");
  await dialog.locator("#entry-comment").fill("E2E throwaway entry");
  await dialog.getByRole("button", { name: "Add Entry" }).click();
  await expect(dialog).toBeHidden();

  await page.getByText("17:00 - 18:00").click();
  dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Edit Entry")).toBeVisible();
  await dialog.getByRole("button", { name: "Delete" }).click();
  // EntryDialog's own remove() opens a separate Confirm.svelte dialog
  // *without* closing itself first, so at this point there are two open
  // native <dialog> elements stacked on top of each other — EntryDialog
  // underneath, Confirm on top — and both have a "Delete" button. A plain
  // page-scoped getByRole("dialog") would match both and throw a strict-mode
  // violation; `.last()` is the most-recently-opened (topmost) one, i.e. the
  // Confirm prompt.
  await page
    .getByRole("dialog")
    .last()
    .getByRole("button", { name: "Delete" })
    .click();
  await expect(page.getByText("17:00 - 18:00")).toBeHidden();

  // Submit the week for approval. Only the two surviving entries (morning
  // and afternoon shifts) move from draft to submitted; per the user guide
  // this is the point at which an approver's review queue picks them up —
  // 06-team-lead-approves-employee.spec.js handles that.
  await page.getByRole("button", { name: "Submit Week" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Submit Week" })
    .click();
  await expect(page.getByText("Week submitted.")).toBeVisible();
});

test("employee: request three absences of different kinds", async () => {
  await page.goto("/absences");

  // All three requests use widely-spaced future date ranges (4, 5, and 6
  // weeks out) purely so they can never overlap each other or trip the
  // backend's "Overlap with existing absence" guard — the exact dates don't
  // matter to what's being tested.
  async function requestAbsence(kindLabel, startOffset, endOffset, comment) {
    await page.getByRole("button", { name: "Request Absence" }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    await dialog.locator("#absence-kind").selectOption({ label: kindLabel });
    await setDate(page, "absence-start-date", isoOffset(startOffset));
    await setDate(page, "absence-end-date", isoOffset(endOffset));
    await dialog.locator("#absence-comment").fill(comment);
    await dialog.getByRole("button", { name: "Submit Request" }).click();
    await expect(dialog).toBeHidden();
  }

  // Three different absence categories, exercising two different cost_type
  // values per the user guide: "Vacation" counts against the annual leave
  // balance; "Training" and the custom NO_COST_ABSENCE_CATEGORY (created in
  // 03-admin-config.spec.js) are both cost_type="none", a free day with no
  // balance impact — deliberately so, since Eve is too new a hire to have
  // banked enough overtime for a flextime-cost absence. Requesting the
  // custom category here is also what proves it's genuinely usable
  // end-to-end, not just creatable.
  await requestAbsence("Vacation", 28, 29, "E2E vacation");
  await requestAbsence("Training", 35, 35, "E2E training");
  await requestAbsence(NO_COST_ABSENCE_CATEGORY, 42, 42, "E2E day off");

  // Scoped to the absence-entry row, not a bare page-wide text search:
  // Playwright's getByText does a case-insensitive substring match, and the
  // category named "E2E Day Off" would otherwise also match the comment
  // text "E2E day off", causing a strict-mode violation between the two.
  await expect(page.locator(".absence-entry", { hasText: "E2E vacation" })).toBeVisible();
  await expect(page.locator(".absence-entry", { hasText: "E2E training" })).toBeVisible();
  await expect(page.locator(".absence-entry", { hasText: "E2E day off" })).toBeVisible();
});

test("employee: cancel a still-pending absence request", async () => {
  await page.goto("/absences");
  await page
    .locator(".absence-entry", { hasText: "E2E training" })
    .click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  // Per the user guide, cancelling a "requested" (not-yet-approved) absence
  // is immediate — no approver review needed, unlike cancelling an already
  // *approved* absence (see 09/10, which exercise that deferred path).
  await dialog.getByRole("button", { name: "Cancel absence" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Yes, cancel absence" })
    .click();
  await expect(page.getByText("Absence cancelled.")).toBeVisible();
  // Only "E2E vacation" and "E2E day off" remain pending after this —
  // 06-team-lead-approves-employee.spec.js expects exactly those two.
});

test("employee: view reports and calendar", async () => {
  await page.goto("/reports");
  // Eve can't see team-wide data (no can_view_team_reports permission), so
  // Reports.svelte falls back to "Your hours overview" rather than the
  // team-lead/admin "Team hours overview" subtitle.
  await expect(page.getByText("Your hours overview")).toBeVisible();

  await page.goto("/calendar");
  // Same self-only framing applies to the calendar heading.
  await expect(page.getByText("My Calendar")).toBeVisible();
});
