// Tests for the payroll report detail dialog. The section that matters most
// here is "Booked later": days from an already-reported month reach the reader
// with the date they were worked, which is the whole reason they are printed
// separately from this month's hours.

import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import PayrollContentDialog from "./PayrollContentDialog.svelte";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

function content(rows) {
  return {
    enabled: true,
    period: "2026-08",
    period_label: "August 2026",
    from: "2026-08-01",
    to: "2026-08-31",
    sent: false,
    day_of_month: 5,
    in_progress: false,
    absence_count: 0,
    people_with_hours: 1,
    minutes: 240,
    rows,
  };
}

let component;

function render(props) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  component = mount(PayrollContentDialog, { target, props });
  return target;
}

afterEach(() => {
  if (component) unmount(component);
  component = null;
  document.body.innerHTML = "";
  setLanguage("en");
});

describe("PayrollContentDialog", () => {
  it("lists a day booked after its own month was reported under that day", () => {
    const target = render({
      content: content([
        {
          name: "Assist, Alex",
          kind: "late_hours",
          category: null,
          from: "2026-07-14",
          to: "2026-07-14",
          days: 1,
          minutes: 240,
          medical_certificate_required: null,
        },
      ]),
      onClose: () => {},
    });

    const text = target.textContent;
    expect(text).toContain("Booked later");
    expect(text).toContain("Assist, Alex");
    // July, while the report itself covers August.
    expect(text).toMatch(/7\/14\/2026|Jul/);
    expect(text).toContain("4:00");
    expect(text).not.toContain("Nothing to report for this month.");
  });

  it("says nothing at all when the month has no content of any kind", () => {
    const target = render({ content: content([]), onClose: () => {} });
    expect(target.textContent).toContain("Nothing to report for this month.");
    expect(target.textContent).not.toContain("Booked later");
  });

  it("keeps the catch-up section out of a report that has none", () => {
    const target = render({
      content: content([
        {
          name: "Assist, Alex",
          kind: "hours",
          category: null,
          from: null,
          to: null,
          days: 4,
          minutes: 930,
          medical_certificate_required: null,
        },
      ]),
      onClose: () => {},
    });
    expect(target.textContent).toContain("Working days and hours");
    expect(target.textContent).not.toContain("Booked later");
  });
});
