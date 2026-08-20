// File 11b: the dashboard's "Who is absent" tile (AbsenceSlider.svelte) —
// week-by-week navigation and the sick-leave rows it's meant to surface.
//
// Reported production bugs this file reproduces:
//   1. "Switching weeks with the prev/next arrows isn't reliable, and the
//      displayed date doesn't update even when it did switch correctly."
//   2. "Not every employee with a sick-leave entry actually shows up in the
//      tile" (observed with several sick-leave entries across a month).
//
// Root cause (frontend/src/routes/dashboard/AbsenceSlider.svelte): clicking
// prev/next fires a new fetch without waiting for the previous one to
// finish, and the result was applied unconditionally regardless of which
// request it belonged to. Two requests can be in flight at once; if a
// slower response for an *earlier* click resolves after a faster response
// for a *later* click, it silently overwrites the correct, newer data with
// stale (or empty) data — the header (bound to a separate `week` variable)
// already shows the right range, but the row list underneath reverts. With
// several sick-leave weeks in a row, that looks exactly like "some people's
// sick leave never shows up", depending on which responses happen to race.
//
// Two dedicated people (not EMPLOYEE/ASSISTANT/TEAM_LEAD from users.js) are
// onboarded so the exact contents of four specific past weeks are known and
// under this file's sole control — no other spec places absences far enough
// back to collide with them (see pastWeekWorkday in helpers.js).
//
// Runs after 11a (all approvals settled) and before 12 (which archives the
// shared employee — irrelevant here, but keeps the numbering consistent).

import { test, expect } from "@playwright/test";
import {
  changeTempPassword,
  createUserViaAdminUi,
  pastWeekWorkday,
  readCredentials,
  setDate,
  signIn,
  storageStatePath,
  writeCredential,
} from "./helpers.js";
import {
  ABSENCE_SLIDER_EMPLOYEE_ONE,
  ABSENCE_SLIDER_EMPLOYEE_TWO,
  ABSENCE_SLIDER_START_OFFSET_DAYS,
  TEAM_LEAD,
} from "./users.js";

const ONE_NAME = `${ABSENCE_SLIDER_EMPLOYEE_ONE.firstName} ${ABSENCE_SLIDER_EMPLOYEE_ONE.lastName}`;
const TWO_NAME = `${ABSENCE_SLIDER_EMPLOYEE_TWO.firstName} ${ABSENCE_SLIDER_EMPLOYEE_TWO.lastName}`;
const ONE_PASSWORD = "SliderOnePass123!";
const TWO_PASSWORD = "SliderTwoPass123!";

// Populated by "admin onboards two people" below, read by every later
// describe block in this file.
let weekOneDate; // 1 week ago  — only Sina is sick
let weekTwoDate; // 2 weeks ago — both Sina and Theo are sick
let weekFourDate; // 4 weeks ago — only Theo is sick
// Week 3 (in between) is deliberately left with no absences at all, to
// cover the tile's empty-week state during navigation.

test.describe("admin onboards two people for absence-slider testing", () => {
  test.use({ storageState: storageStatePath("admin") });

  test("admin: create Sina and Theo, reporting to the team lead", async ({
    page,
  }) => {
    // Resolve the target dates once, up front, from an authenticated
    // request context (any signed-in user can read /holidays).
    weekOneDate = await pastWeekWorkday(page.request, 1);
    weekTwoDate = await pastWeekWorkday(page.request, 2);
    weekFourDate = await pastWeekWorkday(page.request, 4);

    const onePassword = await createUserViaAdminUi(page, {
      ...ABSENCE_SLIDER_EMPLOYEE_ONE,
      role: "employee",
      approverEmail: TEAM_LEAD.email,
      startDateOffsetDays: ABSENCE_SLIDER_START_OFFSET_DAYS,
    });
    writeCredential(
      "absence_slider_one",
      ABSENCE_SLIDER_EMPLOYEE_ONE.email,
      onePassword,
    );

    const twoPassword = await createUserViaAdminUi(page, {
      ...ABSENCE_SLIDER_EMPLOYEE_TWO,
      role: "employee",
      approverEmail: TEAM_LEAD.email,
      startDateOffsetDays: ABSENCE_SLIDER_START_OFFSET_DAYS,
    });
    writeCredential(
      "absence_slider_two",
      ABSENCE_SLIDER_EMPLOYEE_TWO.email,
      twoPassword,
    );
  });
});

// Requests a single-day "Sick" absence for whichever user `page` is signed
// in as. Sick is seeded with auto_approve_past (see 11a), so a past date
// needs no separate approval step — it is immediately visible to the team
// lead's team view, exactly the path the reported bug affects.
async function requestPastSickDay(page, dateIso, comment) {
  await page.goto("/absences");
  await page.getByRole("button", { name: "Request Absence" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.locator("#absence-kind").selectOption({ label: "Sick" });
  await setDate(page, "absence-start-date", dateIso);
  await setDate(page, "absence-end-date", dateIso);
  await dialog.locator("#absence-comment").fill(comment);
  await dialog.getByRole("button", { name: "Submit Request" }).click();
  await expect(dialog).toBeHidden();
  await expect(
    page.locator(".absence-entry", { hasText: comment }),
  ).toContainText("Approved");
}

test.describe("Sina reports in sick twice", () => {
  let context;
  let page;

  test.beforeAll(async ({ browser }) => {
    context = await browser.newContext();
    page = await context.newPage();
    const { absence_slider_one: credentials } = readCredentials();
    await signIn(page, credentials.email, credentials.password);
    await changeTempPassword(
      page,
      context,
      "absence_slider_one",
      ABSENCE_SLIDER_EMPLOYEE_ONE.email,
      ONE_PASSWORD,
    );
  });

  test.afterAll(async () => {
    await context?.close();
  });

  test("Sina: reports sick one week ago and two weeks ago", async () => {
    await requestPastSickDay(page, weekOneDate, "E2E slider Sina week 1");
    await requestPastSickDay(page, weekTwoDate, "E2E slider Sina week 2");
  });
});

test.describe("Theo reports in sick twice", () => {
  let context;
  let page;

  test.beforeAll(async ({ browser }) => {
    context = await browser.newContext();
    page = await context.newPage();
    const { absence_slider_two: credentials } = readCredentials();
    await signIn(page, credentials.email, credentials.password);
    await changeTempPassword(
      page,
      context,
      "absence_slider_two",
      ABSENCE_SLIDER_EMPLOYEE_TWO.email,
      TWO_PASSWORD,
    );
  });

  test.afterAll(async () => {
    await context?.close();
  });

  test("Theo: reports sick two weeks ago and four weeks ago", async () => {
    await requestPastSickDay(page, weekTwoDate, "E2E slider Theo week 2");
    await requestPastSickDay(page, weekFourDate, "E2E slider Theo week 4");
  });
});

test.describe("team lead browses the Who is absent tile", () => {
  test.use({ storageState: storageStatePath("team_lead") });

  const rangeButton = (page) => page.locator(".absence-week-range");
  const prevButton = (page) => page.locator('[aria-label="Previous week"]');
  const nextButton = (page) => page.locator('[aria-label="Next week"]');
  const sliderCard = (page) =>
    page.locator(".zf-card", { hasText: "Who is absent" });

  test("tile shows the current week on load and Today returns to it", async ({
    page,
  }) => {
    await page.goto("/dashboard");
    await expect(sliderCard(page)).toBeVisible();

    const initialLabel = (await rangeButton(page).textContent()).trim();
    expect(initialLabel.length).toBeGreaterThan(0);

    // Move away, then use the range button itself (title="Today") to jump
    // straight back — this is the exact control the reported bug affects.
    await nextButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .not.toBe(initialLabel);

    await rangeButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .toBe(initialLabel);
  });

  test("stepping back one week at a time reveals each week's sick leave and nobody else's", async ({
    page,
  }) => {
    await page.goto("/dashboard");
    await expect(sliderCard(page)).toBeVisible();
    const initialLabel = (await rangeButton(page).textContent()).trim();

    // Week -1: only Sina.
    await prevButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .not.toBe(initialLabel);
    await expect(sliderCard(page)).toContainText(ONE_NAME);
    await expect(sliderCard(page)).not.toContainText(TWO_NAME);
    const week1Label = (await rangeButton(page).textContent()).trim();

    // Week -2: both Sina and Theo — the direct reproduction of "not every
    // employee with sick leave is shown".
    await prevButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .not.toBe(week1Label);
    await expect(sliderCard(page)).toContainText(ONE_NAME);
    await expect(sliderCard(page)).toContainText(TWO_NAME);
    const week2Label = (await rangeButton(page).textContent()).trim();

    // Week -3: deliberately empty.
    await prevButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .not.toBe(week2Label);
    await expect(sliderCard(page)).toContainText("No absences this week.");
    await expect(sliderCard(page)).not.toContainText(ONE_NAME);
    await expect(sliderCard(page)).not.toContainText(TWO_NAME);
    const week3Label = (await rangeButton(page).textContent()).trim();

    // Week -4: only Theo.
    await prevButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .not.toBe(week3Label);
    await expect(sliderCard(page)).toContainText(TWO_NAME);
    await expect(sliderCard(page)).not.toContainText(ONE_NAME);

    // Walking back forward four times must land exactly back on the week
    // the tile opened on — proving the label itself (not just the data)
    // reliably tracks every click, in both directions.
    for (let step = 0; step < 4; step++) await nextButton(page).click();
    await expect
      .poll(async () => (await rangeButton(page).textContent()).trim())
      .toBe(initialLabel);
  });

  test("a slow response for an earlier click cannot overwrite a faster response for a later click", async ({
    page,
  }) => {
    // Full-stack reproduction of the race condition: real browser, real
    // backend, real network requests — only the *timing* of one response is
    // controlled, via route interception, to force the exact out-of-order
    // resolution an impatient double-click can trigger for real. Without
    // the `loadSeq` guard in AbsenceSlider.svelte, this test fails because
    // the delayed first response for week -1 lands after the immediate
    // second response for week -2, and overwrites the display with the
    // wrong (or emptier) week's data even though the header already reads
    // week -2's range.
    // Wait for the tile's own *initial* load (status=approved) to fully
    // settle before installing the delayed route below — Playwright only
    // intercepts requests issued after the route is registered, but an
    // explicit wait here removes any doubt that the interceptor could catch
    // the mount-time request instead of a click-triggered one (the
    // dashboard also fires an unrelated status=pending_review request for
    // the approval queue on the same mount, which this must not be
    // confused with).
    const initialLoad = page.waitForResponse(
      (r) =>
        new URL(r.url()).pathname === "/api/v1/absences/all" &&
        new URL(r.url()).searchParams.get("status") === "approved",
    );
    await page.goto("/dashboard");
    await expect(sliderCard(page)).toBeVisible();
    await initialLoad;

    let delayedOne = false;
    await page.route("**/api/v1/absences/all**", async (route) => {
      if (!delayedOne) {
        delayedOne = true;
        await new Promise((resolve) => setTimeout(resolve, 800));
      }
      await route.continue();
    });

    // Two "previous week" clicks fired back-to-back, not awaiting the first
    // request's response — exactly what a real user's fast double-click
    // produces. The first click's request is the one artificially delayed
    // above; the second's is not, so it resolves first.
    await prevButton(page).click();
    await prevButton(page).click();

    // Let both in-flight requests settle (the delayed one included), then
    // assert the final, stable state matches week -2 (both people) — the
    // week actually landed on — and never reverts to week -1's or an empty
    // result once the stale, slow response finally arrives.
    await page.waitForTimeout(1200);
    await expect(sliderCard(page)).toContainText(ONE_NAME);
    await expect(sliderCard(page)).toContainText(TWO_NAME);

    // Give any further microtask-queued (mis-)updates one more beat, then
    // re-assert — catches a fix that happens to look right immediately
    // after the race but still lets the stale response through a tick
    // later.
    await page.waitForTimeout(300);
    await expect(sliderCard(page)).toContainText(ONE_NAME);
    await expect(sliderCard(page)).toContainText(TWO_NAME);

    await page.unroute("**/api/v1/absences/all**");
  });
});
