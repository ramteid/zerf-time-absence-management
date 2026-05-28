// Tests for the auditLog domain module. The audit log shows admins a history
// of every data change: who changed what, when, and what it looked like before
// and after. Key concerns:
//   - Time-entry audit rows for the same user+action+week are grouped into one
//     summary row (e.g. "Week 5: 3 changes") to reduce noise
//   - Non-time-entry rows (users, absences, categories, …) are never grouped
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

// Use English translations so label assertions are predictable across locales.
setLanguage("en");

const translate = (key, params) => {
  // Minimal translate stub — returns key + params for assertions.
  if (!params) return key;
  return key.replace(/\{(\w+)\}/g, (_, k) => (params[k] ?? `{${k}}`));
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
  it("returns null for non-time-entry tables", () => {
    // Only time_entries rows get grouped by week; other tables display individually.
    expect(weekInfoFromEntry({ table_name: "users" })).toBeNull();
  });

  it("returns null when entry_date is missing from the payload", () => {
    const entry = {
      table_name: "time_entries",
      action: "updated",
      before_data: null,
      after_data: '{}',
    };
    expect(weekInfoFromEntry(entry)).toBeNull();
  });

  it("computes the Monday-based week containing the entry date", () => {
    // 2026-01-07 (Wednesday) → week starts Monday 2026-01-05.
    const entry = {
      table_name: "time_entries",
      action: "updated",
      before_data: null,
      after_data: '{"entry_date":"2026-01-07"}',
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
      after_data: '{"first_name":"Alice","last_name":"Admin","email":"a@b.com"}',
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
    const entry = { table_name: "categories", action: "created", before_data: null, after_data: null };
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
    const result = fmtFieldVal("entry_date", "2026-01-05", new Map(), translate);
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
      before_data: '{"first_name":"Bob","last_name":"Smith","email":"b@s.com","role":"employee","active":true}',
      after_data: '{"first_name":"Robert","last_name":"Smith","email":"b@s.com","role":"employee","active":true}',
    };
    const rows = extractDetailRows(entry, new Map(), translate);
    expect(rows).not.toBeNull();
    expect(rows.some((r) => r.before === "Bob")).toBe(true);
    expect(rows.every((r) => r.label !== "Last name")).toBe(true);
  });

  it("returns null when no fields differ (no-op update)", () => {
    const entry = {
      table_name: "categories",
      before_data: '{"name":"Work","color":"#123456","description":null,"counts_as_work":true,"active":true}',
      after_data: '{"name":"Work","color":"#123456","description":null,"counts_as_work":true,"active":true}',
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

describe("buildRows — time-entry grouping", () => {
  const userMap = new Map([[1, "Ada Lead"]]);

  it("groups multiple time_entry changes for the same user+action+week", () => {
    // Individually showing 20 entry edits for the same week would bury other
    // audit entries. Grouping them into one row with a count keeps the log
    // scannable without losing the key facts (who, what week, how many).
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
    expect(rows).toHaveLength(1);
    expect(rows[0].group_count).toBe(2);
    expect(rows[0].is_time_entry_week).toBe(true);
  });

  it("does not group entries across different users or actions", () => {
    // Grouping must be scoped to (user × action × week) — otherwise edits by
    // different users, or creates vs. deletes, would be incorrectly merged.
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
        user_id: 2,
        action: "updated",
        table_name: "time_entries",
        before_data: null,
        after_data: '{"entry_date":"2026-01-06"}',
      },
    ];
    const rows = buildRows(entries, userMap, translate);
    expect(rows).toHaveLength(2);
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
