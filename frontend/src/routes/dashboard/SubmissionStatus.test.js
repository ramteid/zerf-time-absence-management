// Tests for the Submissions dashboard tile and its detail dialog. They
// cover the "X of Y done" summary, the traffic-light breakdown, the switch
// between the previous and the running month, and the detail list.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import SubmissionStatus from "./SubmissionStatus.svelte";
import SubmissionStatusDialog from "../../dialogs/SubmissionStatusDialog.svelte";
import { setLanguage } from "../../i18n.js";
import { appTodayDate, fmtMonthName } from "../../format.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

const goMock = vi.hoisted(() => vi.fn());
vi.mock("../../stores.js", async () => {
  const actual = await vi.importActual("../../stores.js");
  return { ...actual, go: goMock };
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function status(overrides = {}) {
  return {
    enabled: true,
    period: "2026-07",
    period_label: "July 2026",
    from: "2026-07-01",
    to: "2026-07-31",
    in_progress: false,
    sent: false,
    day_of_month: 5,
    total: 3,
    ready: 1,
    awaiting_approval: 1,
    not_submitted: 1,
    members: [
      { user_id: 2, name: "Erin Employee", status: "ready" },
      { user_id: 3, name: "Alex Assistant", status: "awaiting_approval" },
      { user_id: 4, name: "Nora Newcomer", status: "not_submitted" },
    ],
    ...overrides,
  };
}

describe("SubmissionStatus tile", () => {
  let component;
  let target;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    goMock.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("summarizes progress and draws one arc per state", async () => {
    component = mount(SubmissionStatus, {
      target,
      props: { status: status() },
    });
    await settle();

    expect(target.textContent).toContain("1 of 3 done");
    expect(target.textContent).toContain("July 2026");
    // One coloured arc for each non-zero state.
    expect(target.querySelectorAll(".donut-segment").length).toBe(3);
    expect(target.querySelector(".seg-awaiting")).toBeTruthy();
  });

  it("spells out who has to act next to the ring", async () => {
    component = mount(SubmissionStatus, {
      target,
      props: { status: status() },
    });
    await settle();

    const breakdown = target.querySelector(".submissions-breakdown");
    expect(breakdown.textContent).toContain("1 not submitted");
    expect(breakdown.textContent).toContain("1 waiting for approval");
  });

  it("omits arcs for states nobody is in", async () => {
    component = mount(SubmissionStatus, {
      target,
      props: {
        status: status({
          total: 2,
          ready: 2,
          awaiting_approval: 0,
          not_submitted: 0,
        }),
      },
    });
    await settle();

    expect(target.querySelectorAll(".donut-segment").length).toBe(1);
    expect(target.textContent).toContain("2 of 2 done");
  });

  it("always offers a peek at the current month", async () => {
    // The submissions tile is never "done" — there is always a running month
    // worth looking at, so the offer does not wait on a delivery any more.
    const onShowCurrentMonth = vi.fn();
    component = mount(SubmissionStatus, {
      target,
      props: { status: status(), onShowCurrentMonth },
    });
    await settle();

    const label = `Show ${fmtMonthName(appTodayDate())}`;
    const peekButton = [...target.querySelectorAll("button")].find(
      (button) => button.textContent.trim() === label,
    );
    expect(peekButton).toBeTruthy();

    peekButton.click();
    expect(onShowCurrentMonth).toHaveBeenCalled();
  });

  it("offers the way back while it shows the running month", async () => {
    const now = appTodayDate();
    const onShowPreviousMonth = vi.fn();
    component = mount(SubmissionStatus, {
      target,
      props: { status: status({ in_progress: true }), onShowPreviousMonth },
    });
    await settle();

    expect(target.textContent).not.toContain(`Show ${fmtMonthName(now)}`);
    expect(target.textContent).toContain("still running");
    const label = `Show ${fmtMonthName(
      new Date(now.getFullYear(), now.getMonth() - 1, 1),
    )}`;
    const backButton = [...target.querySelectorAll("button")].find(
      (button) => button.textContent.trim() === label,
    );
    expect(backButton).toBeTruthy();

    backButton.click();
    expect(onShowPreviousMonth).toHaveBeenCalled();
  });

  it("calls nobody done with a month that is still running", async () => {
    component = mount(SubmissionStatus, {
      target,
      props: { status: status({ in_progress: true }) },
    });
    await settle();

    expect(target.textContent).toContain("1 of 3 up to date");
    expect(target.textContent).not.toContain("done");
  });

  it("says so when nobody is tracked instead of counting 0 of 0", async () => {
    component = mount(SubmissionStatus, {
      target,
      props: {
        status: status({
          total: 0,
          ready: 0,
          awaiting_approval: 0,
          not_submitted: 0,
          members: [],
        }),
      },
    });
    await settle();

    expect(target.textContent).toContain("No people in this month.");
    expect(target.textContent).not.toContain("0 of 0");
  });

  it("explains itself, and says it is not the payroll report", async () => {
    component = mount(SubmissionStatus, {
      target,
      props: { status: status(), activeHelp: "submissions" },
    });
    await settle();

    expect(target.querySelector(".dashboard-help").textContent).toContain(
      "payroll report",
    );
  });

  it("opens the detail view when the tile is clicked", async () => {
    const onOpen = vi.fn();
    component = mount(SubmissionStatus, {
      target,
      props: { status: status(), onOpen },
    });
    await settle();

    target.querySelector(".submissions-card-button").click();
    expect(onOpen).toHaveBeenCalled();
  });
});

describe("SubmissionStatusDialog", () => {
  let component;
  let target;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    goMock.mockClear();
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("lists every person by name, each one linking to their report", async () => {
    component = mount(SubmissionStatusDialog, {
      target,
      props: { status: status(), onClose: () => {} },
    });
    await settle();

    const rows = document.querySelectorAll(".payroll-row");
    expect(rows.length).toBe(3);
    expect(
      [...rows].every((row) => row.classList.contains("payroll-row-link")),
    ).toBe(true);
  });

  it("opens that person's report in month mode for the same month", async () => {
    component = mount(SubmissionStatusDialog, {
      target,
      props: { status: status(), onClose: () => {} },
    });
    await settle();

    const row = [...document.querySelectorAll(".payroll-row-link")].find((el) =>
      el.textContent.includes("Erin Employee"),
    );
    row.click();
    await settle();

    expect(goMock).toHaveBeenCalledWith("/reports?user=2&month=2026-07");
  });

  it("labels the running month up to date rather than done", async () => {
    component = mount(SubmissionStatusDialog, {
      target,
      props: { status: status({ in_progress: true }), onClose: () => {} },
    });
    await settle();

    const readyRow = [...document.querySelectorAll(".payroll-row")].find(
      (row) => row.textContent.includes("Erin Employee"),
    );
    expect(readyRow.textContent).toContain("Up to date");
    expect(document.body.textContent).toContain("1 of 3 up to date");
    expect(document.body.textContent).toContain("still running");
  });

  it("sends somebody waiting for approval to the approval lists", async () => {
    const onShowApprovals = vi.fn(() => true);
    const onClose = vi.fn();
    component = mount(SubmissionStatusDialog, {
      target,
      props: { status: status(), onClose, onShowApprovals },
    });
    await settle();

    const awaitingRow = [...document.querySelectorAll(".payroll-row")].find(
      (row) => row.textContent.includes("Alex Assistant"),
    );
    awaitingRow.click();
    await settle();

    expect(onShowApprovals).toHaveBeenCalledWith(
      expect.objectContaining({ user_id: 3 }),
    );
    expect(onClose).toHaveBeenCalled();
    // The report has no approve button, so it is not where this row leads.
    expect(goMock).not.toHaveBeenCalled();
  });

  it("falls back to the report when no approval list holds the person", async () => {
    component = mount(SubmissionStatusDialog, {
      target,
      props: {
        status: status(),
        onClose: () => {},
        onShowApprovals: () => false,
      },
    });
    await settle();

    const awaitingRow = [...document.querySelectorAll(".payroll-row")].find(
      (row) => row.textContent.includes("Alex Assistant"),
    );
    awaitingRow.click();
    await settle();

    expect(goMock).toHaveBeenCalledWith("/reports?user=3&month=2026-07");
  });

  it("lists the people still missing before the ones already done", async () => {
    component = mount(SubmissionStatusDialog, {
      target,
      props: { status: status(), onClose: () => {} },
    });
    await settle();

    const labels = [...document.querySelectorAll(".payroll-row")].map((row) =>
      row.textContent.replace(/\s+/g, " ").trim(),
    );
    expect(labels[0]).toContain("Not submitted");
    expect(labels[labels.length - 1]).toContain("Done");
  });

  it("marks a booked but unapproved person with the amber status", async () => {
    component = mount(SubmissionStatusDialog, {
      target,
      props: { status: status(), onClose: () => {} },
    });
    await settle();

    const awaitingRow = [...document.querySelectorAll(".payroll-row")].find(
      (row) => row.textContent.includes("Alex Assistant"),
    );
    expect(awaitingRow.querySelector(".zf-chip-pending")).toBeTruthy();
    expect(awaitingRow.textContent).toContain("Waiting for approval");
  });
});
