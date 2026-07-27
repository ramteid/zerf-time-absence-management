import {
  addDays,
  fmtDate,
  fmtDateShort,
  isoDate,
  isoWeek,
  monday,
  parseDate,
} from "../../format.js";
import { absenceKindLabel } from "../../i18n.js";

// Fields to show in the detail popup, per table.
export const TABLE_FIELDS = {
  time_entries: ["entry_date", "start_time", "end_time", "status", "note"],
  users: ["first_name", "last_name", "email", "role", "active"],
  absences: ["kind", "start_date", "end_date", "status", "note"],
  categories: ["name", "color", "description", "counts_as_work", "active"],
  holidays: ["name", "holiday_date"],
  app_settings: ["key", "value"],
  reopen_requests: ["week_start_date", "status"],
};

export const FIELD_LABEL_KEYS = {
  entry_date: "Date",
  start_time: "Start",
  end_time: "End",
  status: "Status",
  note: "Note",
  first_name: "First name",
  last_name: "Last name",
  email: "Email",
  role: "Role",
  active: "Active",
  kind: "Type",
  start_date: "From",
  end_date: "To",
  name: "Name",
  color: "Color",
  description: "Description",
  counts_as_work: "Counts as work",
  holiday_date: "Date",
  key: "Setting",
  value: "Value",
  week_start_date: "Week start",
};

const DATE_FIELDS = new Set([
  "entry_date",
  "holiday_date",
  "start_date",
  "end_date",
  "week_start_date",
]);

export function safeParseJson(raw) {
  if (!raw) return null;
  try {
    return typeof raw === "string" ? JSON.parse(raw) : raw;
  } catch {
    return null;
  }
}

export function relevantPayload(entry) {
  const payload =
    entry.action === "deleted" ? entry.before_data : entry.after_data;
  return safeParseJson(payload);
}

// Logical table of the backend's week-level audit rows: one row per employee
// week for submit, approve, reject, and silent auto-approval.
// Mirrors services::time_entries::TIME_ENTRY_WEEK_AUDIT_TABLE.
export const TIME_ENTRY_WEEK_TABLE = "time_entry_weeks";

// Date that anchors an audit row to a calendar week.
// Week rows carry their Monday directly. Per-entry rows are anchored by their
// entry_date; rows written before week-level auditing only put the new status
// into after_data, so the before snapshot — which still holds the full entry —
// serves as the fallback.
function weekAnchorDate(entry) {
  const payload = relevantPayload(entry);
  if (entry.table_name === TIME_ENTRY_WEEK_TABLE) {
    return payload?.week_start_date ?? null;
  }
  if (entry.table_name !== "time_entries") return null;
  return (
    payload?.entry_date ?? safeParseJson(entry.before_data)?.entry_date ?? null
  );
}

export function weekInfoFromEntry(entry) {
  const anchorDate = weekAnchorDate(entry);
  if (!anchorDate) return null;

  const weekStartDate = monday(parseDate(anchorDate));
  const weekEndDate = addDays(weekStartDate, 6);
  return {
    week_start: isoDate(weekStartDate),
    week_end: isoDate(weekEndDate),
    week_number: isoWeek(weekStartDate),
  };
}

export function summarize(entry, translate) {
  const payload = relevantPayload(entry);
  if (!payload) return "";

  if (entry.table_name === "users") {
    const fullName =
      `${payload.first_name || ""} ${payload.last_name || ""}`.trim();
    if (fullName && payload.email) return `${fullName} (${payload.email})`;
    if (fullName) return fullName;
    if (payload.email) return payload.email;
    return "";
  }

  if (entry.table_name === "absences") {
    // The audit log preserves the absence row as it was at action time,
    // including `category_name` from the joined category. Pass it through
    // as the fallback so labels still localize even when the category was
    // later deactivated and dropped from the active-only store cache.
    const kind = payload.kind
      ? absenceKindLabel(payload.kind, payload.category_name)
      : null;
    if (payload.start_date && payload.end_date) {
      const range = `${fmtDateShort(payload.start_date)} - ${fmtDateShort(payload.end_date)}`;
      return kind ? `${kind}, ${range}` : range;
    }
    if (kind) return kind;
    return "";
  }

  if (entry.table_name === "categories") {
    return payload.name || "";
  }

  if (entry.table_name === "holidays") {
    if (payload.holiday_date && payload.name) {
      return `${fmtDate(payload.holiday_date)}, ${payload.name}`;
    }
    return payload.name || "";
  }

  if (entry.table_name === "app_settings") {
    return payload.key || "";
  }

  if (entry.table_name === "reopen_requests") {
    if (payload.week_start_date) {
      const start = parseDate(payload.week_start_date);
      const end = addDays(start, 6);
      return translate("Week {week}: {from} - {to}", {
        week: isoWeek(start),
        from: fmtDateShort(start),
        to: fmtDateShort(end),
      });
    }
    return "";
  }

  return "";
}

export function userLabel(userId, userMap, translate) {
  return (
    userMap.get(userId) ||
    (userId == null ? translate("audit_system_user") : `#${userId}`)
  );
}

// ID of the user whose data is being acted on (may differ from the acting user).
// For "users" table: the record itself is the subject. For other tables: look in the payload.
export function subjectUserId(entry) {
  if (entry.table_name === "users") return entry.record_id ?? null;
  const payload = relevantPayload(entry);
  return payload?.user_id ?? null;
}

export function subjectUserLabel(entry, userMap) {
  const subjectId = subjectUserId(entry);
  if (subjectId == null || subjectId === entry.user_id) return null;
  return userMap.get(subjectId) || `#${subjectId}`;
}

// `rowPayload` is the parent JSON object (before/after_data) that `val`
// came from. We pass it through so kind formatting can pick up the
// sibling `category_name` field as a fallback for inactive categories
// that are missing from the active-only frontend store cache.
export function fmtFieldVal(key, val, userMap, translate, rowPayload) {
  if (val == null) return null;
  if (key === "user_id") return userMap.get(val) || `#${val}`;
  if (DATE_FIELDS.has(key)) {
    try {
      return fmtDate(val);
    } catch {
      return String(val);
    }
  }
  if (key === "kind") return absenceKindLabel(val, rowPayload?.category_name);
  if (typeof val === "boolean") return val ? translate("Yes") : translate("No");
  return String(val);
}

export function extractDetailRows(entry, userMap, translate) {
  const fields = TABLE_FIELDS[entry.table_name];
  if (!fields) return null;

  const before = safeParseJson(entry.before_data);
  const after = safeParseJson(entry.after_data);
  const hasBoth = before != null && after != null;
  const result = [];

  for (const key of fields) {
    const bFmt = fmtFieldVal(
      key,
      before?.[key] ?? null,
      userMap,
      translate,
      before,
    );
    const aFmt = fmtFieldVal(
      key,
      after?.[key] ?? null,
      userMap,
      translate,
      after,
    );
    if (bFmt == null && aFmt == null) continue;
    if (hasBoth && bFmt === aFmt) continue;
    result.push({
      label: translate(FIELD_LABEL_KEYS[key] ?? key),
      before: bFmt,
      after: aFmt,
    });
  }

  return result.length > 0 ? result : null;
}

export function actionClass(action) {
  if (
    action === "created" ||
    action === "approved" ||
    action === "auto_approved" ||
    action === "reopened" ||
    action === "restored"
  )
    return "action-success";
  if (
    action === "deleted" ||
    action === "rejected" ||
    action === "deactivated" ||
    action === "archived"
  )
    return "action-danger";
  if (action === "updated" || action === "status_changed") return "action-info";
  return "action-muted";
}

function weekSummary(weekInfo, dayCount, translate) {
  return translate("audit_time_entries_week_summary", {
    week: weekInfo.week_number,
    from: fmtDateShort(weekInfo.week_start),
    to: fmtDateShort(weekInfo.week_end),
    count: dayCount,
  });
}

function weekRow(entry, weekInfo, dayCount, userMap, translate) {
  return {
    ...entry,
    user_label: userLabel(entry.user_id, userMap, translate),
    subject_user_label: subjectUserLabel(entry, userMap),
    is_time_entry_week: true,
    week_start: weekInfo.week_start,
    week_end: weekInfo.week_end,
    week_number: weekInfo.week_number,
    group_count: dayCount,
    // Rejections carry the approver's reason; every other action leaves it unset.
    week_reason: relevantPayload(entry)?.reason ?? null,
    data_summary: weekSummary(weekInfo, dayCount, translate),
  };
}

export function buildRows(entries, userMap, translate) {
  const result = [];
  // Maps "(user_id):(action):(week_start)" -> index in result
  const weekGroupIndex = new Map();

  for (const entry of entries) {
    const weekInfo =
      entry.table_name === "time_entries" ||
      entry.table_name === TIME_ENTRY_WEEK_TABLE
        ? weekInfoFromEntry(entry)
        : null;

    if (!weekInfo) {
      result.push({
        ...entry,
        user_label: userLabel(entry.user_id, userMap, translate),
        subject_user_label: subjectUserLabel(entry, userMap),
        data_summary: summarize(entry, translate),
        is_time_entry_week: false,
      });
      continue;
    }

    // A week row is already one whole-week decision and stands on its own: two
    // decisions on the same week (approve → reopen → approve again) are
    // separate events, each with its own day count, and must not be merged.
    if (entry.table_name === TIME_ENTRY_WEEK_TABLE) {
      const dayCount = relevantPayload(entry)?.entry_count ?? 1;
      result.push(weekRow(entry, weekInfo, dayCount, userMap, translate));
      continue;
    }

    // Per-entry rows (create/edit/delete of single days) are still written one
    // per entry, so they get collapsed here into one row per user+action+week.
    const groupKey = `${entry.user_id ?? ""}:${entry.action}:${weekInfo.week_start}`;
    const existingIdx = weekGroupIndex.get(groupKey);

    if (existingIdx !== undefined) {
      const group = result[existingIdx];
      group.group_count += 1;
      group.data_summary = weekSummary(group, group.group_count, translate);
    } else {
      weekGroupIndex.set(groupKey, result.length);
      result.push(weekRow(entry, weekInfo, 1, userMap, translate));
    }
  }

  return result;
}
