// Tests for the payroll report dashboard tile and its detail dialog. They
// cover the states the tile switches between (pending vs. already sent), the
// "X of Y done" summary, the traffic-light breakdown, and — most importantly —
// that a team lead sees anonymized rows for people they may not see while the
// counts still cover everyone.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import PayrollStatus from "./PayrollStatus.svelte";
import PayrollStatusDialog from "../../dialogs/PayrollStatusDialog.svelte";
import { setLanguage } from "../../i18n.js";

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
    sent: false,
    day_of_month: 5,
    total: 3,
    ready: 1,
    awaiting_approval: 1,
    not_submitted: 1,
    members: [
      {
        user_id: 2,
        name: "Erin Employee",
        status: "ready",
        reason_key: null,
      },
      {
        user_id: 3,
        name: "Alex Assistant",
        status: "awaiting_approval",
        reason_key: "payroll_report_reason_unapproved_entries",
      },
      // Somebody outside this team lead's scope: counted, never named.
      {
        user_id: null,
        name: null,
        status: "not_submitted",
        reason_key: null,
      },
    ],
    ...overrides,
  };
}

describe("PayrollStatus tile", () => {
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
    component = mount(PayrollStatus, { target, props: { status: status() } });
    await settle();

    expect(target.textContent).toContain("1 of 3 done");
    expect(target.textContent).toContain("July 2026");
    // One coloured arc for each non-zero state.
    expect(target.querySelectorAll(".donut-segment").length).toBe(3);
  });

  it("omits arcs for states nobody is in", async () => {
    component = mount(PayrollStatus, {
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

  it("dims the tile and drops the donut once the month has been sent", async () => {
    component = mount(PayrollStatus, {
      target,
      props: { status: status({ sent: true }) },
    });
    await settle();

    expect(target.textContent).toContain("July 2026 sent");
    expect(target.querySelector(".is-dimmed")).toBeTruthy();
    expect(target.querySelector(".donut-segment")).toBeNull();
    // Nothing to drill into any more.
    expect(target.querySelector(".payroll-card-button")).toBeNull();
  });

  it("names the send day in the help text", async () => {
    component = mount(PayrollStatus, {
      target,
      props: { status: status({ day_of_month: 9 }), activeHelp: "payroll" },
    });
    await settle();

    expect(target.querySelector(".dashboard-help").textContent).toContain(
      "day 9 of the month",
    );
  });

  it("opens the detail view when the tile is clicked", async () => {
    const onOpen = vi.fn();
    component = mount(PayrollStatus, {
      target,
      props: { status: status(), onOpen },
    });
    await settle();

    target.querySelector(".payroll-card-button").click();
    expect(onOpen).toHaveBeenCalled();
  });
});

describe("PayrollStatusDialog", () => {
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

  it("anonymizes people the viewer may not see but still lists them", async () => {
    component = mount(PayrollStatusDialog, {
      target,
      props: { status: status(), onClose: () => {} },
    });
    await settle();

    const rows = document.querySelectorAll(".payroll-row");
    // All three people are represented, so the counts add up for the viewer.
    expect(rows.length).toBe(3);
    expect(document.body.textContent).toContain("Erin Employee");
    expect(document.body.textContent).toContain("Not visible to you");
    // The hidden person's row must not be a link to their report.
    const hidden = [...rows].find((row) =>
      row.textContent.includes("Not visible to you"),
    );
    expect(hidden.classList.contains("payroll-row-link")).toBe(false);
  });

  it("deep-links a named row into that person's report for the month", async () => {
    component = mount(PayrollStatusDialog, {
      target,
      props: { status: status(), onClose: () => {} },
    });
    await settle();

    const row = [...document.querySelectorAll(".payroll-row-link")].find((el) =>
      el.textContent.includes("Erin Employee"),
    );
    row.click();
    await settle();

    expect(goMock).toHaveBeenCalledWith(
      "/reports?user=2&from=2026-07-01&to=2026-07-31",
    );
  });

  it("lists the people still missing before the ones already done", async () => {
    component = mount(PayrollStatusDialog, {
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
});
