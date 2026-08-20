// Tests for AbsenceSlider — the weekly team-calendar strip on the dashboard
// that shows who is absent this week. It fetches from /team-absences when the
// current user is a manager (can_approve); employees with no approve permission
// see an empty component. Tests verify rendering, navigation callbacks, and
// the API call guard.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceSlider from "./AbsenceSlider.svelte";
import { currentUser, settings } from "../../stores.js";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

vi.mock("../../lib/api/dashboardApi.js", () => ({
  getTeamAbsences: vi.fn(),
}));

import { getTeamAbsences } from "../../lib/api/dashboardApi.js";

// jsdom does not implement the Web Animations API, but the component's
// week-change block uses Svelte's `fly` transition, which calls
// element.animate() on every re-render. Without this stub, clicking
// prev/next/today below throws "element.animate is not a function" as an
// unhandled exception during the test run. The returned object only needs
// to satisfy what Svelte's transition runtime touches (a settable
// `onfinish` and a callable `cancel()`) — none of the assertions here
// depend on the animation actually completing, since the underlying
// `{#key}` block's content is already mounted/unmounted independently of
// the CSS animation layered on top of it.
if (!Element.prototype.animate) {
  Element.prototype.animate = () => ({
    onfinish: null,
    cancel() {},
  });
}

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("AbsenceSlider", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    vi.clearAllMocks();
    getTeamAbsences.mockResolvedValue([]);
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("calls getTeamAbsences when the current user can approve (lead view)", async () => {
    // Only managers see the team absence slider — employees don't have access
    // to their colleagues' absence data.
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { can_approve: true },
    });
    component = mount(AbsenceSlider, { target, props: { users: [] } });
    await settle();
    expect(getTeamAbsences).toHaveBeenCalled();
  });

  it("does not call getTeamAbsences for employees without approve permission", async () => {
    // Employees only see their own absences elsewhere in the app; the team
    // slider must remain invisible and not make an unauthorized API call.
    currentUser.set({
      id: 2,
      role: "employee",
      permissions: { can_approve: false },
    });
    component = mount(AbsenceSlider, { target, props: { users: [] } });
    await settle();
    expect(getTeamAbsences).not.toHaveBeenCalled();
  });

  it("renders previous-week and next-week navigation buttons", async () => {
    // Managers need to browse back and forward to see absences for past and
    // future weeks, not just the current one.
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { can_approve: true },
    });
    component = mount(AbsenceSlider, { target, props: { users: [] } });
    await settle();
    const buttons = target.querySelectorAll("button");
    expect(buttons.length).toBeGreaterThanOrEqual(2);
  });

  it("renders an absence row for each team member on leave", async () => {
    // The slider must show who is away for this week so managers can plan
    // schedules and coverage without checking another system.
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { can_approve: true },
    });
    getTeamAbsences.mockResolvedValue([
      {
        id: 5,
        user_id: 3,
        kind: "vacation",
        start_date: "2026-07-06",
        end_date: "2026-07-10",
        status: "approved",
      },
    ]);
    const users = [{ id: 3, first_name: "Dave", last_name: "Dev" }];
    component = mount(AbsenceSlider, { target, props: { users } });
    await settle();
    await settle();
    expect(target.textContent).toContain("Dave Dev");
  });

  it("renders both rows when one person has two separate approved absences in the same week", async () => {
    // Regression test: the app only blocks *overlapping* absences for the
    // same person (see assert_no_overlap_tx), so a non-overlapping pair —
    // e.g. sick Mon-Tue, then vacation Thu-Fri — can both land in the same
    // displayed week. The each-block used to be keyed on `absence.user_id`,
    // which collided for this exact case: Svelte's keyed each requires
    // unique keys, so the duplicate key silently dropped the whole block,
    // hiding both absences instead of just the second one.
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { can_approve: true },
    });
    getTeamAbsences.mockResolvedValue([
      {
        id: 5,
        user_id: 3,
        kind: "sick",
        start_date: "2026-07-06",
        end_date: "2026-07-07",
        status: "approved",
      },
      {
        id: 6,
        user_id: 3,
        kind: "vacation",
        start_date: "2026-07-09",
        end_date: "2026-07-10",
        status: "approved",
      },
    ]);
    const users = [{ id: 3, first_name: "Dave", last_name: "Dev" }];
    component = mount(AbsenceSlider, { target, props: { users } });
    await settle();
    await settle();
    const items = target.querySelectorAll(".dropdown-slider-item");
    expect(items.length).toBe(2);
  });

  it("updates the displayed week range when navigating next/previous, and Today returns to it", async () => {
    // Reported symptom: "the displayed date doesn't change even when
    // switched correctly". The header text is bound directly to `week`, so
    // this pins down that clicking really does move it, in both directions.
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { can_approve: true },
    });
    component = mount(AbsenceSlider, { target, props: { users: [] } });
    await settle();

    const rangeButton = target.querySelector(".absence-week-range");
    const initialText = rangeButton.textContent.trim();
    expect(initialText.length).toBeGreaterThan(0);

    target.querySelector('[aria-label="Next week"]').click();
    await settle();
    const afterNext = rangeButton.textContent.trim();
    expect(afterNext).not.toBe(initialText);

    target.querySelector('[aria-label="Previous week"]').click();
    await settle();
    expect(rangeButton.textContent.trim()).toBe(initialText);

    // Move away, then use the range button itself (title="Today") to jump
    // straight back to the current week.
    target.querySelector('[aria-label="Next week"]').click();
    await settle();
    expect(rangeButton.textContent.trim()).not.toBe(initialText);
    rangeButton.click();
    await settle();
    expect(rangeButton.textContent.trim()).toBe(initialText);
  });

  it("keeps the most recently requested week's data even if an earlier request resolves later (race-condition regression)", async () => {
    // Reproduces the reported bug: clicking prev/next fires a new fetch
    // without waiting for the previous one to finish. If a slow response
    // for an *earlier* click arrives after a fast response for a *later*
    // click, it must not be allowed to overwrite the newer, correct data —
    // otherwise the tile can silently revert to the wrong week's absences
    // (or an empty list) even though the header already moved on.
    currentUser.set({
      id: 1,
      role: "team_lead",
      permissions: { can_approve: true },
    });

    let resolveInitialLoad;
    const initialLoadPromise = new Promise((resolve) => {
      resolveInitialLoad = resolve;
    });
    const nextWeekResult = [
      {
        id: 9,
        user_id: 3,
        kind: "sick",
        start_date: "2026-08-10",
        end_date: "2026-08-10",
        status: "approved",
      },
    ];

    let callCount = 0;
    getTeamAbsences.mockImplementation(() => {
      callCount += 1;
      // First call = the initial mount load. Deliberately left pending so
      // it can be resolved *after* the second call, out of order.
      if (callCount === 1) return initialLoadPromise;
      return Promise.resolve(nextWeekResult);
    });

    const users = [{ id: 3, first_name: "Sam", last_name: "Sick" }];
    component = mount(AbsenceSlider, { target, props: { users } });
    await settle(); // initial load still pending

    target.querySelector('[aria-label="Next week"]').click();
    await settle(); // second (faster) request already resolved and rendered
    expect(target.textContent).toContain("Sam Sick");

    // The slow, now-superseded first response finally resolves with a
    // different (empty) result. A correct implementation ignores it.
    resolveInitialLoad([]);
    await settle();
    expect(target.textContent).toContain("Sam Sick");
  });
});
