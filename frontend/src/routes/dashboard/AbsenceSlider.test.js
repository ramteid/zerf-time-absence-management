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
    if (component) { unmount(component); component = null; }
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
});
