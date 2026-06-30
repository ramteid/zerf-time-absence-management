// File 7: the assistant's first login, a permission-boundary check (no
// Dashboard access), and their own time/absence booking — the same kinds of
// operations 05 covers for an employee, but for the "assistant" role, which
// behaves differently in a few specific ways per docs/user-guide.md and
// rolePolicy.js: no weekly-hours contract, no flextime/overtime account, and
// — most importantly for this file's permission test — no Dashboard.

import { test, expect } from "@playwright/test";
import { ASSISTANT } from "./users.js";
import {
  changeTempPassword,
  readCredentials,
  setTime,
  signIn,
} from "./helpers.js";

const ASSISTANT_PASSWORD = "AssistantPass321!";

// Manual context/page for Amy's first login, same reasoning as 04 and 05.
let context;
let page;

test.beforeAll(async ({ browser }) => {
  context = await browser.newContext();
  page = await context.newPage();
  const { assistant } = readCredentials();
  await signIn(page, assistant.email, assistant.password);
});

test.afterAll(async () => {
  await context?.close();
});

test("assistant: sign in and change the temporary password", async () => {
  await expect(page.getByText("Please change your password.")).toBeVisible();
  await changeTempPassword(
    page,
    context,
    "assistant",
    ASSISTANT.email,
    ASSISTANT_PASSWORD,
  );
});

test("assistant: has no dashboard access and lands on /time", async () => {
  // The backend's permission set omits can_view_dashboard for the assistant
  // role (see handlers/auth.rs: "can_view_dashboard": !is_assistant_role),
  // and App.svelte's router redirects any inaccessible route to the user's
  // preferred home — which for an assistant is always /time (there's no
  // Dashboard nav link to fall back to, and assistants don't track
  // home="/dashboard" the way employees with dashboard access might).
  // Directly requesting /dashboard is the most direct way to prove this
  // server-enforced boundary, rather than just checking the nav doesn't
  // show a Dashboard link (which could pass for the wrong reason, e.g. a
  // rendering bug, without proving the route itself is actually blocked).
  await page.goto("/dashboard");
  await page.waitForURL("**/time");
  await expect(page.locator(".week-grid")).toBeVisible();
});

test("assistant: book a time entry and submit the week", async () => {
  await page.goto("/time");
  await page.locator(".time-week-picker button").first().click();
  await expect(page.locator(".week-grid")).toBeVisible();

  // Same booking flow as the employee in 05, just a single entry — the
  // point here is proving an assistant *can* track time and submit a week
  // at all (despite having no weekly-hours contract), not re-testing every
  // edit/delete edge case already covered for the employee role.
  const addButtons = page.locator(".day-add-btn button:not([disabled])");
  await addButtons.first().click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator("#entry-comment")).toBeVisible();
  await setTime(page, "entry-end-time", "12:00");
  await setTime(page, "entry-start-time", "09:00");
  await dialog.locator("#entry-comment").fill("E2E assistant shift");
  await dialog.getByRole("button", { name: "Add Entry" }).click();
  await expect(dialog).toBeHidden();

  await page.getByRole("button", { name: "Submit Week" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Submit Week" })
    .click();
  await expect(page.getByText("Week submitted.")).toBeVisible();
});

test("assistant: request an absence", async () => {
  await page.goto("/absences");
  await page.getByRole("button", { name: "Request Absence" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  // Leave the absence kind at whatever category is selected by default
  // (the first active absence category) — only the comment matters for
  // locating this specific request in 08's approval review.
  await dialog.locator("#absence-comment").fill("E2E assistant absence");
  await dialog.getByRole("button", { name: "Submit Request" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByText("E2E assistant absence")).toBeVisible();
});
