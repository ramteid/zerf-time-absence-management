// File 4: the team lead's first login (forced password change), their own
// Team Settings page (where they configure auto-approval for their direct
// reports), and onboarding an assistant they manage directly.
//
// Per docs/user-guide.md, "team lead" is a distinct role from "admin": a
// team lead can only approve/manage users explicitly assigned to them (here,
// the employee and the assistant created below), never the whole org. They
// can also be granted a scoped ability to create/manage "assistant"-role
// users — gated by an admin setting (allow_team_lead_manage_assistants,
// default enabled) that this suite never touches, so it's exercised here in
// its default-on state via the /settings/team-users page.

import { test, expect } from "@playwright/test";
import {
  changeTempPassword,
  isoOffset,
  readCredentials,
  setDate,
  signIn,
  storageStatePath,
  writeCredential,
} from "./helpers.js";
import { ASSISTANT, EMPLOYEE, TEAM_LEAD } from "./users.js";

const TEAM_LEAD_PASSWORD = "TeamLeadPass789!";

// A manually-managed context/page is needed here (rather than the `page`
// fixture) because this file performs Tom's very first login — using his
// temporary password read from credentials.json — and then needs to keep
// using that same authenticated context across all four tests below (the
// password change must happen before any of the other interactions, and we
// don't want to re-trigger login/password-change in every test() block).
// `changeTempPassword` snapshots this context's storageState once the
// password is set, so *other* files can resume the session without this
// dance — but within this file, we keep using the same context throughout.
let context;
let page;

test.beforeAll(async ({ browser }) => {
  context = await browser.newContext();
  page = await context.newPage();
  const { team_lead } = readCredentials();
  await signIn(page, team_lead.email, team_lead.password);
});

test.afterAll(async () => {
  await context?.close();
});

test("team lead: sign in and change the temporary password", async () => {
  // Same forced-password-change gate every newly created user hits, team
  // leads included — there's nothing role-specific about it.
  await expect(page.getByText("Please change your password.")).toBeVisible();
  await changeTempPassword(
    page,
    context,
    "team_lead",
    TEAM_LEAD.email,
    TEAM_LEAD_PASSWORD,
  );
});

test("team lead: toggle and revert an auto-approval setting", async () => {
  await page.goto("/settings/team");
  // TeamSettings.svelte lists every user this team lead approves, twice over
  // — once in a "Time Submissions" section, once in "Edit Requests" — both
  // rendered with the same `.team-setting-row` class. `.first()` after the
  // email filter picks the Submissions-section row specifically (it's first
  // in the template), giving us its "Auto-approve submissions" checkbox.
  const employeeRow = page
    .locator(".team-setting-row")
    .filter({ hasText: EMPLOYEE.email })
    .first();
  const checkbox = employeeRow.locator('input[type="checkbox"]');

  // Toggled on then immediately back off: this only exercises the
  // PUT /team-settings/{user_id} endpoint and its "Settings saved." toast.
  // Leaving it permanently on would auto-approve Eve's week submission in
  // 05-employee-workflows.spec.js, which would silently remove the manual
  // approval coverage that 06-team-lead-approves-employee.spec.js is for —
  // so the round-trip here is deliberate, not an oversight.
  await checkbox.check();
  await expect(page.getByText("Settings saved.")).toBeVisible();
  await checkbox.uncheck();
  // Toasts stay visible for 3.5s (see lib/app/toast.js), so the first one
  // can still be on screen when the second fires — ".last()" always refers
  // to the most recent toast regardless of whether an older one is still
  // fading out, avoiding a strict-mode violation from matching both.
  await expect(page.getByText("Settings saved.").last()).toBeVisible();
});

test("team lead: create an assistant", async () => {
  await page.goto("/settings/team-users");
  await page.getByRole("button", { name: "Add User" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.locator("#user-first-name").fill(ASSISTANT.firstName);
  await dialog.locator("#user-last-name").fill(ASSISTANT.lastName);
  await dialog.locator("#user-email").fill(ASSISTANT.email);
  await setDate(page, "user-start-date", isoOffset(-21));
  // This dialog is the same UserDialog component used by the admin's "Add
  // Member" flow, but opened with lockedRole="assistant" and
  // apiBase="/team-users" (TeamUsers.svelte): the role field becomes a
  // disabled, pre-filled "Assistant" display, there's no approver checklist
  // (the team lead is implicitly the sole approver), and the request goes
  // to /team-users instead of /users. Nothing else to fill in before
  // submitting.
  await dialog.getByRole("button", { name: "Add User" }).click();

  const tempDialog = page.getByRole("dialog");
  await expect(tempDialog.getByText("Temporary password:")).toBeVisible();
  const password = (
    await tempDialog.locator("strong").first().innerText()
  ).trim();
  await tempDialog.getByRole("button", { name: "OK" }).click();

  await expect(
    page.getByText(`${ASSISTANT.firstName} ${ASSISTANT.lastName}`),
  ).toBeVisible();
  // 07-assistant-workflows.spec.js needs this to perform Amy's first login.
  writeCredential("assistant", ASSISTANT.email, password);
});

test("team lead: edit the assistant", async () => {
  await page.goto("/settings/team-users");
  // Each user row is a direct child <div> of the .zf-card list; within a
  // row the Edit button comes first (no accessible name), Archive second
  // (has a title attribute) — so Edit is reached positionally.
  const row = page.locator(".zf-card > div", { hasText: ASSISTANT.firstName });
  await row.getByRole("button").first().click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  // UserDialog forces weekly_hours/workdays_per_week/overtime to 0 and
  // disables those inputs for the assistant role (assistants don't accrue
  // contract hours or overtime), but the vacation-day override fields have
  // no such restriction — editing one proves the edit path (and its
  // "User updated." confirmation) works for a role-locked dialog too.
  await dialog.locator("#leave-cur").fill("25");
  await dialog.getByRole("button", { name: "Save" }).click();

  await expect(page.getByText("User updated.")).toBeVisible();
});

// Discovered limitation, worked around here rather than silently ignored:
// UserDialog's "Time Categories"/"Absence Categories" checklists (which
// default a brand-new user to every active category) load their options via
// GET /categories/all and /absence-categories/all — both admin-only
// endpoints. When a *team lead* opens this same dialog to create an
// assistant, that fetch gets a 403, is swallowed by an empty `catch {}`, and
// the dialog silently submits an empty category list. The assistant is
// created successfully, but with zero category access — they can't add a
// time entry at all ("No categories available." in the Add Entry dialog)
// until an admin grants access. There is no UserDialog edit-mode path to fix
// this either (the checklist only renders for `isNew`); the only existing
// admin surface that can grant access after the fact is each category's own
// "Available to employees" list. This block exercises exactly that surface,
// as an admin would have to in practice, before the assistant can do
// anything in 07-assistant-workflows.spec.js.
test.describe(() => {
  test.use({ storageState: storageStatePath("admin") });

  test("admin: grant the assistant access to a time and an absence category", async ({
    page,
  }) => {
    await page.goto("/settings/categories");
    // Each category list is one `.zf-card` whose rows are direct child
    // <div>s (see AdminCategories.svelte) — matching on the row text rather
    // than the whole card targets that one row's Edit button specifically.
    // "Core Duties" is the first seeded work category (see
    // repository/categories.rs's INITIAL_CATEGORIES) — granting just this
    // one is enough for the assistant to book a time entry.
    await page
      .locator(".zf-card > div", { hasText: "Core Duties" })
      .getByRole("button")
      .click();
    let dialog = page.getByRole("dialog");
    await expect(dialog.getByText("Edit Category")).toBeVisible();
    await dialog
      .locator("tr", { hasText: ASSISTANT.firstName })
      .locator('input[type="checkbox"]')
      .check();
    await dialog.getByRole("button", { name: "Save" }).click();
    await expect(dialog).toBeHidden();

    // Same fix on the absence side — "Vacation" is the first seeded absence
    // category (see migrations/017_absence_categories.sql).
    await page
      .locator(".zf-card > div", { hasText: "Vacation" })
      .getByRole("button")
      .click();
    dialog = page.getByRole("dialog");
    await expect(dialog.getByText("Edit Absence Category")).toBeVisible();
    await dialog
      .locator("tr", { hasText: ASSISTANT.firstName })
      .locator('input[type="checkbox"]')
      .check();
    await dialog.getByRole("button", { name: "Save" }).click();
    await expect(dialog).toBeHidden();
  });
});
