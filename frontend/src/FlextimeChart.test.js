// FlextimeChart renders coloured bands behind absence, holiday and weekend
// days. The absence colours come from the admin-configured category list, so
// these tests seed `absenceCategories` the way the app does at boot.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import FlextimeChart from "./FlextimeChart.svelte";
import { absenceCategories, settings } from "./stores.js";
import { setLanguage, setAbsenceCategoryCache } from "./i18n.js";
import {
  HOLIDAY_COLOR,
  WEEKEND_COLOR,
  MASKED_ABSENCE_COLOR,
} from "./colors.js";
import { fmtDateShort } from "./format.js";

vi.mock("svelte", async () => {
  return await import("../node_modules/svelte/src/index-client.js");
});

// Freeze "today" past the fixture dates so every point counts as actual and
// the bands are actually drawn (the chart only bands days up to today).
vi.mock("./format.js", async () => {
  const actual = await vi.importActual("./format.js");
  return { ...actual, appTodayIsoDate: vi.fn(() => "2030-01-31") };
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function day(date, overrides = {}) {
  return {
    date,
    actual_min: 480,
    target_min: 480,
    diff_min: 0,
    cumulative_min: 0,
    absence: null,
    holiday: null,
    ...overrides,
  };
}

// Absence categories reach the chart through two channels: the store supplies
// the band colours, the i18n cache the legend's display names. The app seeds
// both together at boot, so the tests do too.
function setCategories(categories) {
  absenceCategories.set(categories);
  setAbsenceCategoryCache(categories);
}

function bandColors(target) {
  return [...target.querySelectorAll('rect[data-testid="flextime-band"]')].map(
    (rect) => rect.getAttribute("fill"),
  );
}

describe("FlextimeChart", () => {
  let target;
  let component;
  let originalResizeObserver;

  beforeEach(() => {
    component = null;
    target = document.createElement("div");
    document.body.appendChild(target);
    // The chart measures its container via bind:clientWidth, which compiles to
    // a ResizeObserver — jsdom doesn't implement one.
    originalResizeObserver = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    setLanguage("en");
    settings.set({ timezone: "Europe/Berlin" });
    setCategories([
      { slug: "vacation", name: "Vacation", color: "#0017c7" },
      { slug: "sick", name: "Sick", color: "#ef4444" },
    ]);
  });

  afterEach(() => {
    if (component) unmount(component);
    target.remove();
    globalThis.ResizeObserver = originalResizeObserver;
    setCategories([]);
    settings.set({});
  });

  // Regression: `bandRuns` calls `dayBandColor`, which reads the absence
  // colour lookup. Because that call goes through a plain function, Svelte
  // cannot see the dependency and used to schedule `bandRuns` before the
  // lookup existed — mounting then threw "absColor is not a function" and took
  // the whole employee report down with it. Only days that actually carry an
  // absence reach that code path, which is why reports for people without
  // absences kept working.
  it("mounts without throwing when a day carries an absence", async () => {
    expect(() => {
      component = mount(FlextimeChart, {
        target,
        props: {
          data: [
            day("2030-01-07"),
            day("2030-01-08", { absence: "vacation" }),
            day("2030-01-09"),
          ],
        },
      });
    }).not.toThrow();
    await settle();

    expect(target.querySelector("svg")).toBeTruthy();
  });

  it("bands an absence day with its configured category colour", async () => {
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [
          day("2030-01-07"),
          day("2030-01-08", { absence: "vacation" }),
          day("2030-01-09", { absence: "sick" }),
        ],
      },
    });
    await settle();

    const colors = bandColors(target);
    expect(colors).toContain("#0017c7");
    expect(colors).toContain("#ef4444");
  });

  it("falls back to the masked colour for a category it does not know", async () => {
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [
          day("2030-01-07"),
          day("2030-01-08", { absence: "retired_kind" }),
        ],
      },
    });
    await settle();

    expect(bandColors(target)).toContain(MASKED_ABSENCE_COLOR);
  });

  // The category list is loaded asynchronously at boot, so the chart can mount
  // before it arrives. The bands must repaint once it does instead of keeping
  // the masked fallback forever.
  it("repaints absence bands when the category list arrives after mount", async () => {
    setCategories([]);
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [day("2030-01-07"), day("2030-01-08", { absence: "vacation" })],
      },
    });
    await settle();
    expect(bandColors(target)).toContain(MASKED_ABSENCE_COLOR);

    setCategories([{ slug: "vacation", name: "Vacation", color: "#0017c7" }]);
    await settle();

    expect(bandColors(target)).toContain("#0017c7");
    expect(bandColors(target)).not.toContain(MASKED_ABSENCE_COLOR);
  });

  it("bands holidays and weekends without needing an absence category", async () => {
    setCategories([]);
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [
          day("2030-01-04"),
          day("2030-01-05"), // Saturday
          day("2030-01-06"), // Sunday
          day("2030-01-07", { holiday: "Epiphany" }),
        ],
      },
    });
    await settle();

    const colors = bandColors(target);
    expect(colors).toContain(WEEKEND_COLOR);
    expect(colors).toContain(HOLIDAY_COLOR);
  });

  it("lists every distinct band kind once in the legend", async () => {
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [
          day("2030-01-07", { absence: "vacation" }),
          day("2030-01-08", { absence: "vacation" }),
          day("2030-01-09", { absence: "sick" }),
          day("2030-01-12"), // Saturday
        ],
      },
    });
    await settle();

    const legend = target.textContent;
    expect(legend).toContain("Vacation");
    expect(legend).toContain("Sick");
    expect(legend).toContain("Weekends");
  });

  // `asOf` is the same cutoff shown above the chart as "As of {date}" (end of
  // the last fully approved week). Regression: an earlier version filtered
  // the *entire* dataset down to `asOf`, which also erased bands and x-axis
  // labels for any day past the cutoff — breaking a team lead's report for a
  // colleague whose absence fell in a week that hadn't closed the ledger yet
  // (see the "team lead reads another person's report" e2e spec). Only the
  // balance line/area may stop at the cutoff; everything else must still
  // cover the full data range, since it reflects real approved facts rather
  // than a ledger contribution.
  it("keeps x-axis labels and bands for days after asOf", async () => {
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [
          day("2030-01-07"),
          day("2030-01-08"),
          day("2030-01-09"),
          day("2030-01-10", { absence: "sick" }),
          day("2030-01-11"),
        ],
        asOf: "2030-01-09",
      },
    });
    await settle();

    const labels = target.textContent;
    expect(labels).toContain(fmtDateShort("2030-01-10"));
    expect(labels).toContain(fmtDateShort("2030-01-11"));
    expect(bandColors(target)).toContain("#ef4444"); // the sick day past the cutoff
  });

  it("stops the balance line at asOf even though bands continue", async () => {
    component = mount(FlextimeChart, {
      target,
      props: {
        data: [
          day("2030-01-07"),
          day("2030-01-08"),
          day("2030-01-09"),
          day("2030-01-10"),
          day("2030-01-11"),
        ],
        asOf: "2030-01-09",
      },
    });
    await settle();

    // The line/area path is built from 3 in-range points (through asOf), so
    // it emits 2 "L" segments per point after the first — 4 total. Left
    // uncapped, 5 points would emit 8.
    const linePathD = target
      .querySelector('path[fill="none"]')
      .getAttribute("d");
    expect((linePathD.match(/L/g) || []).length).toBe(4);
  });

  it("renders an empty state instead of a chart without data", async () => {
    component = mount(FlextimeChart, { target, props: { data: [] } });
    await settle();

    expect(target.querySelector("svg")).toBeFalsy();
  });
});
