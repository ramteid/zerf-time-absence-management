import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import PayrollContent from "./PayrollContent.svelte";
import { setLanguage } from "../../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

function payrollContent(minutes, rows) {
  return {
    enabled: true,
    period: "2026-08",
    period_label: "August 2026",
    sent: false,
    day_of_month: 5,
    in_progress: false,
    absence_count: 0,
    people_with_hours: 1,
    minutes,
    rows,
  };
}

function correction(minutes, date) {
  return {
    name: "Assist, Alex",
    kind: "late_hours",
    category: null,
    from: date,
    to: date,
    days: 1,
    minutes,
    medical_certificate_required: null,
  };
}

describe("PayrollContent", () => {
  let component;
  let target;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("shows a negative-only correction total", () => {
    component = mount(PayrollContent, {
      target,
      props: {
        content: payrollContent(-90, [correction(-90, "2026-07-14")]),
      },
    });

    expect(target.querySelector(".payroll-sub").textContent).toContain("-1:30");
  });

  it("shows a zero total when signed corrections offset each other", () => {
    component = mount(PayrollContent, {
      target,
      props: {
        content: payrollContent(0, [
          correction(-60, "2026-07-14"),
          correction(60, "2026-07-15"),
        ]),
      },
    });

    expect(target.querySelector(".payroll-sub").textContent).toContain("0:00");
  });
});
