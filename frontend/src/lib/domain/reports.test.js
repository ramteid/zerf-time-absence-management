import { beforeEach, describe, expect, it } from "vitest";
import {
  categoryColumnsFromTeamReport,
  dedupeAbsences,
  filterCategories,
  filterTeamCategoryColumns,
  absenceKindTotals,
  summarizeAbsences,
  teamCategoryMinutes,
  teamCategoryRowTotal,
  totalAbsenceDays,
  totalCategoryMinutes,
} from "./reports.js";
import { absenceCategories } from "../../stores.js";

// Seed the store so keepsWorkTargetSlugs() picks up flextime_reduction and
// any custom keeps_work_target=true category. Tests reset it as needed.
const BASE_CATEGORIES = [
  { id: 1, slug: "vacation", name: "Vacation", keeps_work_target: false },
  { id: 2, slug: "sick", name: "Sick Leave", keeps_work_target: false },
  { id: 7, slug: "flextime_reduction", name: "Flextime Reduction", keeps_work_target: true },
];

describe("reports domain helpers", () => {
  beforeEach(() => {
    absenceCategories.set(BASE_CATEGORIES);
  });
  it("summarizes absence days by kind", () => {
    expect(
      summarizeAbsences([
        { kind: "vacation", days: 2 },
        { kind: "sick", days: 1 },
        { kind: "vacation", days: 0.5 },
      ]),
    ).toEqual({ vacation: 2.5, sick: 1 });
  });

  it("deduplicates absences by id", () => {
    expect(dedupeAbsences([{ id: 1 }, { id: 1 }, { id: 2 }])).toEqual([
      { id: 1 },
      { id: 2 },
    ]);
  });

  it("sorts team category columns by total minutes", () => {
    const columns = categoryColumnsFromTeamReport([
      {
        categories: [
          { category: "Admin", color: "#111", minutes: 30 },
          { category: "Project", color: "#222", minutes: 120 },
        ],
      },
      { categories: [{ category: "Admin", color: "#111", minutes: 100 }] },
    ]);

    expect(columns.map((column) => column.category)).toEqual([
      "Admin",
      "Project",
    ]);
  });

  it("totals only selected categories when requested", () => {
    expect(
      teamCategoryRowTotal(
        {
          categories: [
            { category: "Admin", minutes: 30 },
            { category: "Project", minutes: 120 },
          ],
        },
        ["Project"],
      ),
    ).toBe(120);
  });

  it("keeps an empty category filter empty", () => {
    expect(
      filterCategories([{ category: "Project", minutes: 60 }], []),
    ).toEqual([]);
    expect(
      filterTeamCategoryColumns([{ category: "Project", color: "#222" }], []),
    ).toEqual([]);
  });

  it("uses unknown for missing absence kinds", () => {
    expect(absenceKindTotals([{ days: 1 }])).toEqual({ unknown: 1 });
  });

  // Bug 6: categoryColumnsFromTeamReport secondary sort
  it("sorts equal-minute categories alphabetically as secondary sort", () => {
    const columns = categoryColumnsFromTeamReport([
      {
        categories: [
          { category: "Zebra", color: "#z", minutes: 60 },
          { category: "Alpha", color: "#a", minutes: 60 },
          { category: "Mango", color: "#m", minutes: 60 },
        ],
      },
    ]);
    expect(columns.map((c) => c.category)).toEqual(["Alpha", "Mango", "Zebra"]);
  });

  it("primary sort by descending minutes takes precedence over name", () => {
    const columns = categoryColumnsFromTeamReport([
      {
        categories: [
          { category: "Alpha", color: "#a", minutes: 10 },
          { category: "Zebra", color: "#z", minutes: 200 },
        ],
      },
    ]);
    expect(columns.map((c) => c.category)).toEqual(["Zebra", "Alpha"]);
  });

  // Bug 7: absenceKindTotals excludes zero-day kinds
  it("excludes absence kinds whose total is zero", () => {
    expect(
      absenceKindTotals([
        { kind: "vacation", days: 2 },
        { kind: "sick", days: 0 },
      ]),
    ).toEqual({ vacation: 2 });
  });

  it("returns empty object when all absence kinds total zero", () => {
    expect(
      absenceKindTotals([
        { kind: "sick", days: 0 },
        { kind: "vacation", days: 0 },
      ]),
    ).toEqual({});
  });

  // totalAbsenceDays and totalCategoryMinutes edge cases
  it("totalAbsenceDays returns 0 for null input", () => {
    expect(totalAbsenceDays(null)).toBe(0);
  });

  it("totalCategoryMinutes returns 0 for empty list", () => {
    expect(totalCategoryMinutes([])).toBe(0);
  });

  // teamCategoryMinutes returns 0 for missing category
  it("teamCategoryMinutes returns 0 when category is not in row", () => {
    expect(
      teamCategoryMinutes({ categories: [{ category: "Admin", minutes: 30 }] }, "Project"),
    ).toBe(0);
  });

  // flextime_reduction must not inflate leave statistics
  it("totalAbsenceDays excludes flextime_reduction absences", () => {
    expect(
      totalAbsenceDays([
        { kind: "vacation", days: 3 },
        { kind: "flextime_reduction", days: 2 },
        { kind: "sick", days: 1 },
      ]),
    ).toBe(4); // vacation + sick only
  });

  it("absenceKindTotals excludes flextime_reduction absences", () => {
    expect(
      absenceKindTotals([
        { kind: "vacation", days: 3 },
        { kind: "flextime_reduction", days: 2 },
        { kind: "sick", days: 1 },
      ]),
    ).toEqual({ vacation: 3, sick: 1 });
  });

  it("summarizeAbsences excludes flextime_reduction absences", () => {
    expect(
      summarizeAbsences([
        { kind: "vacation", days: 5 },
        { kind: "flextime_reduction", days: 2 },
      ]),
    ).toEqual({ vacation: 5 });
  });

  // Bug B1: summarizeAbsences excludes zero-day kinds
  it("summarizeAbsences excludes absence kinds whose total is zero", () => {
    expect(
      summarizeAbsences([
        { kind: "vacation", days: 2 },
        { kind: "sick", days: 0 },
      ]),
    ).toEqual({ vacation: 2 });
  });

  it("summarizeAbsences returns empty object when all kinds total zero", () => {
    expect(
      summarizeAbsences([
        { kind: "sick", days: 0 },
        { kind: "vacation", days: 0 },
      ]),
    ).toEqual({});
  });

  it("summarizeAbsences still sums partial days before filtering", () => {
    expect(
      summarizeAbsences([
        { kind: "vacation", days: 0.5 },
        { kind: "vacation", days: 0.5 },
        { kind: "sick", days: 0 },
      ]),
    ).toEqual({ vacation: 1 });
  });

  // B3: exclusion is flag-based, not slug-based — custom keeps_work_target categories are excluded too
  it("totalAbsenceDays excludes any custom keeps_work_target=true category", () => {
    absenceCategories.set([
      { id: 1, slug: "vacation", name: "Vacation", keeps_work_target: false },
      { id: 8, slug: "comp_time", name: "Comp Time", keeps_work_target: true },
    ]);
    expect(
      totalAbsenceDays([
        { kind: "vacation", days: 3 },
        { kind: "comp_time", days: 2 },
      ]),
    ).toBe(3); // comp_time excluded because keeps_work_target=true
  });

  it("absenceKindTotals excludes any custom keeps_work_target=true category", () => {
    absenceCategories.set([
      { id: 1, slug: "vacation", name: "Vacation", keeps_work_target: false },
      { id: 8, slug: "comp_time", name: "Comp Time", keeps_work_target: true },
    ]);
    expect(
      absenceKindTotals([
        { kind: "vacation", days: 3 },
        { kind: "comp_time", days: 2 },
      ]),
    ).toEqual({ vacation: 3 });
  });
});
