import { describe, expect, it } from "vitest";
import {
  csvEncode,
  csvSafe,
  flextimeBounds,
  buildTimesheetCsv,
  safeFileNamePart,
} from "./timesheetCsv.js";

const translate = (key) => key;

describe("csvEncode", () => {
  it("quotes fields containing a comma, quote, or line break", () => {
    expect(csvEncode(["normal"])).toBe("normal");
    expect(csvEncode(["with,comma"])).toBe('"with,comma"');
    expect(csvEncode(['with"quote'])).toBe('"with""quote"');
    expect(csvEncode(["line\nbreak"])).toBe('"line\nbreak"');
    expect(csvEncode(["carriage\rreturn"])).toBe('"carriage\rreturn"');
  });

  it("renders null/undefined fields as an empty string", () => {
    expect(csvEncode([null, undefined, "x"])).toBe(",,x");
  });
});

describe("csvSafe", () => {
  it("prefixes formula-triggering leading characters with a single quote", () => {
    expect(csvSafe("=SUM(A1)")).toBe("'=SUM(A1)");
    expect(csvSafe("+1")).toBe("'+1");
    expect(csvSafe("-1")).toBe("'-1");
    expect(csvSafe("@cmd")).toBe("'@cmd");
  });

  it("leaves ordinary text untouched", () => {
    expect(csvSafe("Vacation")).toBe("Vacation");
  });
});

describe("flextimeBounds", () => {
  it("returns null bounds for an empty series", () => {
    expect(flextimeBounds([])).toEqual({ opening: null, closing: null });
    expect(flextimeBounds(null)).toEqual({ opening: null, closing: null });
  });

  it("derives the opening balance from the first day's cumulative minus its diff", () => {
    const rows = [
      { cumulative_min: 100, diff_min: 20 },
      { cumulative_min: 150, diff_min: 50 },
    ];
    expect(flextimeBounds(rows)).toEqual({ opening: 80, closing: 150 });
  });
});

describe("buildTimesheetCsv", () => {
  const baseReport = {
    days: [
      {
        date: "2026-05-04",
        weekday: "Monday",
        absence: null,
        holiday: null,
        entries: [
          {
            start_time: "08:00",
            end_time: "16:00",
            category: "Development",
            minutes: 480,
            status: "approved",
            counts_as_work: true,
            comment: "",
          },
        ],
      },
      {
        date: "2026-05-05",
        weekday: "Tuesday",
        absence: null,
        holiday: null,
        entries: [],
      },
    ],
  };

  it("emits a header row, one row per entry, an empty-day row, and a total", () => {
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [],
      translate,
    });
    const rows = csv.split("\r\n");
    expect(rows[0]).toBe(
      "Date,Weekday,Start,End,Category,Duration,Status,Comment,Absence,Holiday",
    );
    expect(rows[1]).toContain("2026-05-04");
    expect(rows[1]).toContain("08:00");
    expect(rows[2]).toContain("2026-05-05");
    expect(rows[2]).toContain("0:00"); // empty day
    expect(rows[3]).toContain("Total");
    expect(rows[3]).toContain("8:00"); // only the approved entry counts
  });

  it("excludes non-approved and non-crediting entries from the total", () => {
    const report = {
      days: [
        {
          date: "2026-05-04",
          weekday: "Monday",
          absence: null,
          holiday: null,
          entries: [
            {
              start_time: "08:00",
              end_time: "12:00",
              category: "Development",
              minutes: 240,
              status: "submitted", // not yet approved
              counts_as_work: true,
            },
            {
              start_time: "13:00",
              end_time: "14:00",
              category: "Lunch",
              minutes: 60,
              status: "approved",
              counts_as_work: false, // non-crediting
            },
          ],
        },
      ],
    };
    const csv = buildTimesheetCsv({ report, flextimeData: [], translate });
    const totalRow = csv.split("\r\n").at(-1);
    expect(totalRow).toContain("0:00");
  });

  it("appends opening/closing flextime balance rows when flextime data is present", () => {
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [
        { cumulative_min: 100, diff_min: 20 },
        { cumulative_min: 150, diff_min: 50 },
      ],
      translate,
    });
    expect(csv).toContain("Flextime opening balance");
    expect(csv).toContain("+1:20");
    expect(csv).toContain("Flextime closing balance");
    expect(csv).toContain("+2:30");
  });

  it("appends a flextime adjustments row when an admin booking landed in the period", () => {
    // Without this row, opening + worked hours would not reconcile against
    // closing, and the reader would have no way to see why: the difference
    // is an admin booking, not a data error.
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [
        // Day 1: diff -20 (no adjustment), day 2: diff +50, of which +90 is
        // an admin booking and -40 is worked time. Opening 100 -> closing 180.
        { cumulative_min: 80, diff_min: -20, adjustment_min: 0 },
        { cumulative_min: 130, diff_min: 50, adjustment_min: 90 },
      ],
      translate,
    });
    expect(csv).toContain("Flextime opening balance");
    expect(csv).toContain("Flextime adjustments");
    expect(csv).toContain("+1:30"); // 90 minutes
    expect(csv).toContain("Flextime closing balance");

    // opening (100) + sum(diff_min) (-20 + 50) == closing (130): the
    // reconciliation the "Flextime adjustments" row exists to explain.
    const { opening, closing } = flextimeBounds([
      { cumulative_min: 80, diff_min: -20, adjustment_min: 0 },
      { cumulative_min: 130, diff_min: 50, adjustment_min: 90 },
    ]);
    expect(opening).toBe(100);
    expect(opening - 20 + 50).toBe(closing);
  });

  it("omits the adjustments row when no booking landed in the period", () => {
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [
        { cumulative_min: 100, diff_min: 20, adjustment_min: 0 },
        { cumulative_min: 150, diff_min: 50, adjustment_min: 0 },
      ],
      translate,
    });
    expect(csv).not.toContain("Flextime adjustments");
  });

  it("adds the balance cutoff row, capped at the last ledger day", () => {
    // The closing balance stops at the last fully approved week; the export
    // must say so instead of letting the number read as "end of range".
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [
        { date: "2026-05-04", cumulative_min: 100, diff_min: 20 },
        { date: "2026-05-05", cumulative_min: 150, diff_min: 50 },
      ],
      balanceAsOf: "2026-05-03",
      translate,
    });
    expect(csv).toContain("Flextime balance as of");
    expect(csv).toContain("2026-05-03");
  });

  it("caps the balance cutoff row at the exported range's last day", () => {
    // A cutoff after the exported range would otherwise name a date the
    // document says nothing about.
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [
        { date: "2026-05-04", cumulative_min: 100, diff_min: 20 },
        { date: "2026-05-05", cumulative_min: 150, diff_min: 50 },
      ],
      balanceAsOf: "2026-06-30",
      translate,
    });
    expect(csv).toContain("Flextime balance as of");
    expect(csv).toContain("2026-05-05");
    expect(csv).not.toContain("2026-06-30");
  });

  it("omits the cutoff row when there is no flextime data", () => {
    const csv = buildTimesheetCsv({
      report: baseReport,
      flextimeData: [],
      balanceAsOf: "2026-05-03",
      translate,
    });
    expect(csv).not.toContain("Flextime balance as of");
  });

  it("guards absence cells against formula injection", () => {
    // absenceKindLabel falls back to the raw slug when it isn't a known
    // absence category — "=cmd" round-trips unchanged and must be csvSafe'd.
    const report = {
      days: [
        {
          date: "2026-05-04",
          weekday: "Monday",
          absence: "=cmd",
          holiday: null,
          entries: [],
        },
      ],
    };
    const csv = buildTimesheetCsv({ report, flextimeData: [], translate });
    expect(csv).toContain("'=cmd");
  });
});

describe("safeFileNamePart", () => {
  it("replaces unsafe characters with a hyphen", () => {
    expect(safeFileNamePart("Ada Lövström!")).toBe("Ada-L-vstr-m");
  });

  it("falls back when the cleaned result is empty", () => {
    expect(safeFileNamePart("###", "report")).toBe("report");
    expect(safeFileNamePart("", "report")).toBe("report");
  });
});
