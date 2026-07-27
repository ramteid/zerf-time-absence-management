// Tests for the auditLog domain module. The audit log shows admins a history
// of every data change: who changed what, when, and what it looked like before
// and after. Key concerns:
//   - Week-level decisions (submit/approve/reject/auto-approve a timesheet
//     week) are their own audit table and always render as a single row,
//     carrying a full snapshot of every day entry in the week
//   - Every other row (including individual time entry create/update/delete)
//     is shown individually, never merged with another row
//   - Summaries are human-readable with entity-specific formatting
//   - Action classes control the colour coding in the UI (green = good, red = bad)

import { describe, expect, it } from "vitest";
import {
  actionClass,
  buildRows,
  extractDetailRows,
  fmtFieldVal,
  relevantPayload,
  safeParseJson,
  subjectUserId,
  subjectUserLabel,
  summarize,
  userLabel,
  weekInfoFromEntry,
} from "./auditLog.js";
import { setLanguage } from "../../i18n.js";
import { fmtDateShort } from "../../format.js";

// Use English translations so label assertions are predictable across locales.
setLanguage("en");

const translate = (key, params) => {
  // Minimal translate stub — returns key + params for assertions.
  if (!params) return key;
  return key.replace(/\{(\w+)\}/g, (_, k) => params[k] ?? `{${k}}`);
};

describe("safeParseJson", () => {
  it("parses a valid JSON string", () => {
    expect(safeParseJson('{"a":1}')).toEqual({ a: 1 });
  });

  it("returns the value as-is when already an object", () => {
    const obj = { x: 2 };
    expect(safeParseJson(obj)).toBe(obj);
  });

  it("returns null for invalid JSON", () => {
    expect(safeParseJson("not json")).toBeNull();
  });

  it("returns null for null/undefined input", () => {
    expect(safeParseJson(null)).toBeNull();
    expect(safeParseJson(undefined)).toBeNull();
  });
});

describe("relevantPayload", () => {
  it("uses before_data for deleted entries (shows what was removed)", () => {
    // Deleted records no longer have after_data; the before snapshot is the
    // only meaningful representation of what was lost.
    const entry = {
      action: "deleted",
      before_data: '{"name":"Old"}',
      after_data: null,
    };
    expect(relevantPayload(entry)).toEqual({ name: "Old" });
  });

  it("uses after_data for any non-deleted action (created, updated, approved)", () => {
    // The after snapshot is what the record looks like now, which is the most
    // useful state for created/updated/approved actions.
    const entry = {
      action: "created",
      before_data: null,
      after_data: '{"name":"New"}',
    };
    expect(relevantPayload(entry)).toEqual({ name: "New" });
  });
});

describe("weekInfoFromEntry", () => {
  it("returns null for tables other than the week-level audit table", () => {
    // A single entry edit is its own event and is never shown as a week.
    expect(weekInfoFromEntry({ table_name: "users" })).toBeNull();
    expect(
      weekInfoFromEntry({
        table_name: "time_entries",
        action: "updated",
        before_data: null,
        after_data: '{"entry_date":"2026-01-07"}',
      }),
    ).toBeNull();
  });

  it("returns null when week_start_date is missing from the payload", () => {
    const entry = {
      table_name: "time_entry_weeks",
      action: "approved",
      before_data: null,
      after_data: "{}",
    };
    expect(weekInfoFromEntry(entry)).toBeNull();
  });

  it("reads the week directly from a week-level row", () => {
    // 2026-01-05 is already the Monday the backend computed.
    const entry = {
      table_name: "time_entry_weeks",
      action: "approved",
      before_data: '{"status":"submitted"}',
      after_data:
        '{"status":"approved","user_id":7,"week_start_date":"2026-01-05","entry_count":5}',
    };
    const info = weekInfoFromEntry(entry);
    expect(info.week_start).toBe("2026-01-05");
    expect(info.week_end).toBe("2026-01-11");
    expect(typeof info.week_number).toBe("number");
  });
});

describe("summarize", () => {
  it("returns full name and email for user entries", () => {
    const entry = {
      table_name: "users",
      action: "created",
      before_data: null,
      after_data:
        '{"first_name":"Alice","last_name":"Admin","email":"a@b.com"}',
    };
    expect(summarize(entry, translate)).toBe("Alice Admin (a@b.com)");
  });

  it("returns only name when user has no email in payload", () => {
    const entry = {
      table_name: "users",
      action: "updated",
      before_data: null,
      after_data: '{"first_name":"Bob","last_name":"Smith"}',
    };
    expect(summarize(entry, translate)).toBe("Bob Smith");
  });

  it("returns date and time range for time entry rows", () => {
    const entry = {
      table_name: "time_entries",
      action: "created",
      before_data: null,
      after_data:
        '{"entry_date":"2026-01-07","start_time":"08:00","end_time":"16:00"}',
    };
    expect(summarize(entry, translate)).toBe(
      `${fmtDateShort("2026-01-07")}, 08:00–16:00`,
    );
  });

  it("returns just the date for time entry rows without a time range", () => {
    const entry = {
      table_name: "time_entries",
      action: "deleted",
      before_data: '{"entry_date":"2026-01-07"}',
      after_data: null,
    };
    expect(summarize(entry, translate)).toBe(fmtDateShort("2026-01-07"));
  });

  it("returns the category name for category entries", () => {
    const entry = {
      table_name: "categories",
      action: "created",
      before_data: null,
      after_data: '{"name":"Core Duties"}',
    };
    expect(summarize(entry, translate)).toBe("Core Duties");
  });

  it("returns setting key for app_settings entries", () => {
    const entry = {
      table_name: "app_settings",
      action: "updated",
      before_data: null,
      after_data: '{"key":"smtp_host","value":"mail.example.com"}',
    };
    expect(summarize(entry, translate)).toBe("smtp_host");
  });

  it("returns empty string when payload is null", () => {
    const entry = {
      table_name: "categories",
      action: "created",
      before_data: null,
      after_data: null,
    };
    expect(summarize(entry, translate)).toBe("");
  });
});

describe("userLabel", () => {
  it("returns the cached name from the userMap when available", () => {
    const userMap = new Map([[1, "Alice Admin"]]);
    expect(userLabel(1, userMap, translate)).toBe("Alice Admin");
  });

  it("returns a fallback #id when not in the map", () => {
    expect(userLabel(99, new Map(), translate)).toBe("#99");
  });

  it("returns the system label for null user_id (background tasks)", () => {
    // Some actions (e.g. automated reminders) have no acting user.
    // The label distinguishes them from real actors in the audit trail.
    expect(userLabel(null, new Map(), translate)).toBe("audit_system_user");
  });
});

describe("subjectUserId", () => {
  it("uses record_id for user table (the user record IS the subject)", () => {
    const entry = {
      table_name: "users",
      record_id: 7,
      action: "updated",
      before_data: null,
      after_data: '{"first_name":"Bob"}',
    };
    expect(subjectUserId(entry)).toBe(7);
  });

  it("reads user_id from payload for non-user tables", () => {
    const entry = {
      table_name: "absences",
      record_id: 42,
      action: "created",
      before_data: null,
      after_data: '{"user_id":3,"kind":"vacation"}',
    };
    expect(subjectUserId(entry)).toBe(3);
  });
});

describe("subjectUserLabel", () => {
  it("returns null when the subject is the same as the acting user (self-edit)", () => {
    // If Alice edits her own record the label would be redundant — hide it.
    const entry = {
      table_name: "users",
      record_id: 1,
      user_id: 1,
      action: "updated",
      before_data: null,
      after_data: "{}",
    };
    expect(subjectUserLabel(entry, new Map([[1, "Alice"]]))).toBeNull();
  });

  it("returns the subject name when different from the acting user", () => {
    const entry = {
      table_name: "users",
      record_id: 5,
      user_id: 1,
      action: "updated",
      before_data: null,
      after_data: "{}",
    };
    const userMap = new Map([[5, "Carol"]]);
    expect(subjectUserLabel(entry, userMap)).toBe("Carol");
  });
});

describe("fmtFieldVal", () => {
  it("formats boolean true as Yes and false as No", () => {
    expect(fmtFieldVal("active", true, new Map(), translate)).toBe("Yes");
    expect(fmtFieldVal("active", false, new Map(), translate)).toBe("No");
  });

  it("formats date fields as locale date strings", () => {
    // Date fields must be human-readable (e.g. not raw ISO strings) so
    // admins can spot date-based changes without mental parsing.
    const result = fmtFieldVal(
      "entry_date",
      "2026-01-05",
      new Map(),
      translate,
    );
    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });

  it("returns null for null values (omit the row in the detail table)", () => {
    expect(fmtFieldVal("note", null, new Map(), translate)).toBeNull();
  });

  it("resolves user_id fields to names via the userMap", () => {
    const userMap = new Map([[3, "Frank"]]);
    expect(fmtFieldVal("user_id", 3, userMap, translate)).toBe("Frank");
  });
});

describe("extractDetailRows", () => {
  it("returns null for unknown table names", () => {
    // Unknown tables have no field definition, so there's nothing to show.
    const entry = {
      table_name: "unknown_table",
      before_data: '{"x":1}',
      after_data: '{"x":2}',
    };
    expect(extractDetailRows(entry, new Map(), translate)).toBeNull();
  });

  it("shows only changed fields when both before and after snapshots exist", () => {
    // Showing unchanged fields clutters the diff and makes real changes harder
    // to spot. Only fields that differ between before and after are shown.
    const entry = {
      table_name: "users",
      before_data:
        '{"first_name":"Bob","last_name":"Smith","email":"b@s.com","role":"employee","active":true}',
      after_data:
        '{"first_name":"Robert","last_name":"Smith","email":"b@s.com","role":"employee","active":true}',
    };
    const rows = extractDetailRows(entry, new Map(), translate);
    expect(rows).not.toBeNull();
    expect(rows.some((r) => r.before === "Bob")).toBe(true);
    expect(rows.every((r) => r.label !== "Last name")).toBe(true);
  });

  it("returns null when no fields differ (no-op update)", () => {
    const entry = {
      table_name: "categories",
      before_data:
        '{"name":"Work","color":"#123456","description":null,"counts_as_work":true,"active":true}',
      after_data:
        '{"name":"Work","color":"#123456","description":null,"counts_as_work":true,"active":true}',
    };
    expect(extractDetailRows(entry, new Map(), translate)).toBeNull();
  });
});

describe("actionClass", () => {
  it("maps created/approved/reopened to success styling", () => {
    for (const action of ["created", "approved", "reopened"]) {
      expect(actionClass(action)).toBe("action-success");
    }
  });

  it("maps deleted/rejected/deactivated to danger styling", () => {
    for (const action of ["deleted", "rejected", "deactivated"]) {
      expect(actionClass(action)).toBe("action-danger");
    }
  });

  it("maps updated/status_changed to info styling", () => {
    for (const action of ["updated", "status_changed"]) {
      expect(actionClass(action)).toBe("action-info");
    }
  });

  it("maps unknown actions to muted styling", () => {
    expect(actionClass("activated")).toBe("action-muted");
  });
});

describe("buildRows — individual entry rows are never merged", () => {
  const userMap = new Map([[1, "Ada Lead"]]);

  it("shows each time entry change as its own row, even for the same user/action/week", () => {
    // A single entry edit is its own event: showing two edits as one merged
    // row would hide which specific day was touched and when.
    const entries = [
      {
        id: 1,
        user_id: 1,
        action: "updated",
        table_name: "time_entries",
        before_data: null,
        after_data: '{"entry_date":"2026-01-06"}',
      },
      {
        id: 2,
        user_id: 1,
        action: "updated",
        table_name: "time_entries",
        before_data: null,
        after_data: '{"entry_date":"2026-01-07"}',
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows).toHaveLength(2);
    expect(rows.every((r) => !r.is_time_entry_week)).toBe(true);
  });

  it("never groups non-time-entry tables regardless of user and action", () => {
    // User, absence, and category audit rows must always display individually
    // so admins can see exactly which record was affected.
    const entries = [
      {
        id: 1,
        user_id: 1,
        action: "updated",
        table_name: "users",
        before_data: '{"first_name":"Alice"}',
        after_data: '{"first_name":"Alicia"}',
      },
      {
        id: 2,
        user_id: 1,
        action: "updated",
        table_name: "users",
        before_data: '{"first_name":"Bob"}',
        after_data: '{"first_name":"Robert"}',
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows).toHaveLength(2);
    expect(rows.every((r) => !r.is_time_entry_week)).toBe(true);
  });
});

describe("buildRows — week-level rows", () => {
  const userMap = new Map([
    [1, "Ada Lead"],
    [7, "Bob Employee"],
  ]);

  it("renders one row per week decision with the backend's entry count", () => {
    const entries = [
      {
        id: 10,
        user_id: 1,
        action: "approved",
        table_name: "time_entry_weeks",
        record_id: 7,
        before_data: '{"status":"submitted"}',
        after_data:
          '{"status":"approved","user_id":7,"week_start_date":"2026-01-05","entry_count":5,"entry_ids":[1,2,3,4,5]}',
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows).toHaveLength(1);
    expect(rows[0].is_time_entry_week).toBe(true);
    expect(rows[0].group_count).toBe(5);
    // The approver acted for someone else, so the subject is shown separately.
    expect(rows[0].subject_user_label).toBe("Bob Employee");
    expect(rows[0].week_start).toBe("2026-01-05");
  });

  it("keeps repeated decisions on the same week as separate rows", () => {
    // Approve → reopen → approve again are distinct events. Merging them would
    // invent a single approval covering the sum of both day counts.
    const week = (id, count) => ({
      id,
      user_id: 1,
      action: "approved",
      table_name: "time_entry_weeks",
      record_id: 7,
      before_data: '{"status":"submitted"}',
      after_data: `{"status":"approved","user_id":7,"week_start_date":"2026-01-05","entry_count":${count}}`,
    });
    const rows = buildRows([week(11, 5), week(10, 2)], userMap, translate);
    expect(rows).toHaveLength(2);
    expect(rows.map((r) => r.group_count)).toEqual([5, 2]);
  });

  it("exposes the rejection reason of a rejected week", () => {
    const entries = [
      {
        id: 12,
        user_id: 1,
        action: "rejected",
        table_name: "time_entry_weeks",
        record_id: 7,
        before_data: '{"status":"submitted"}',
        after_data:
          '{"status":"rejected","user_id":7,"week_start_date":"2026-01-05","entry_count":3,"reason":"Tuesday is missing"}',
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows[0].week_reason).toBe("Tuesday is missing");
  });

  it("exposes the per-day entry snapshots embedded in the week payload", () => {
    const entries = [
      {
        id: 13,
        user_id: 1,
        action: "approved",
        table_name: "time_entry_weeks",
        record_id: 7,
        before_data: '{"status":"submitted"}',
        after_data: JSON.stringify({
          status: "approved",
          user_id: 7,
          week_start_date: "2026-01-05",
          entry_count: 2,
          entries: [
            {
              id: 101,
              entry_date: "2026-01-05",
              start_time: "08:00",
              end_time: "12:00",
              category_id: 3,
              category_name: "Core Duties",
              comment: "morning shift",
            },
            {
              id: 102,
              entry_date: "2026-01-06",
              start_time: "09:00",
              end_time: "17:00",
              category_id: 3,
              category_name: "Core Duties",
              comment: null,
            },
          ],
        }),
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows[0].week_entries).toHaveLength(2);
    expect(rows[0].week_entries[0].category_name).toBe("Core Duties");
    expect(rows[0].week_entries[1].comment).toBeNull();
  });

  it("falls back to an empty entry list for legacy rows without a snapshot", () => {
    // Rows written before per-entry snapshots existed only carry entry_count.
    const entries = [
      {
        id: 14,
        user_id: 1,
        action: "approved",
        table_name: "time_entry_weeks",
        record_id: 7,
        before_data: '{"status":"submitted"}',
        after_data:
          '{"status":"approved","user_id":7,"week_start_date":"2026-01-05","entry_count":3,"entry_ids":[1,2,3]}',
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows[0].week_entries).toEqual([]);
    expect(rows[0].group_count).toBe(3);
  });
});
