import { describe, expect, it } from "vitest";
import {
  categoryColumnsFromTeamReport,
  dedupeAbsences,
  filterCategories,
  filterTeamCategoryColumns,
  absenceKindTotals,
  summarizeAbsences,
  teamCategoryRowTotal,
} from "./reports.js";

describe("reports domain helpers", () => {
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
});
