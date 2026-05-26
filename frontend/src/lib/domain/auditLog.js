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

export function weekInfoFromEntry(entry) {
  if (entry.table_name !== "time_entries") return null;
  const payload = relevantPayload(entry);
  const entryDate = payload?.entry_date;
  if (!entryDate) return null;

  const weekStartDate = monday(parseDate(entryDate));
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
    const fullName = `${payload.first_name || ""} ${payload.last_name || ""}`.trim();
    if (fullName && payload.email) return `${fullName} (${payload.email})`;
    if (fullName) return fullName;
    if (payload.email) return payload.email;
    return "";
  }

  if (entry.table_name === "absences") {
    const kind = payload.kind ? absenceKindLabel(payload.kind) : null;
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

export function fmtFieldVal(key, val, userMap, translate) {
  if (val == null) return null;
  if (key === "user_id") return userMap.get(val) || `#${val}`;
  if (DATE_FIELDS.has(key)) {
    try {
      return fmtDate(val);
    } catch {
      return String(val);
    }
  }
  if (key === "kind") return absenceKindLabel(val);
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
    const bFmt = fmtFieldVal(key, before?.[key] ?? null, userMap, translate);
    const aFmt = fmtFieldVal(key, after?.[key] ?? null, userMap, translate);
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
  if (action === "created" || action === "approved" || action === "reopened")
    return "action-success";
  if (action === "deleted" || action === "rejected" || action === "deactivated")
    return "action-danger";
  if (action === "updated" || action === "status_changed") return "action-info";
  return "action-muted";
}

export function buildRows(entries, userMap, translate) {
  const result = [];
  // Maps "(user_id):(action):(week_start)" -> index in result
  const weekGroupIndex = new Map();

  for (const entry of entries) {
    const weekInfo =
      entry.table_name === "time_entries" ? weekInfoFromEntry(entry) : null;

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

    const groupKey = `${entry.user_id ?? ""}:${entry.action}:${weekInfo.week_start}`;
    const existingIdx = weekGroupIndex.get(groupKey);

    if (existingIdx !== undefined) {
      const group = result[existingIdx];
      group.group_count += 1;
      group.data_summary = translate("audit_time_entries_week_summary", {
        week: group.week_number,
        from: fmtDateShort(group.week_start),
        to: fmtDateShort(group.week_end),
        count: group.group_count,
      });
    } else {
      weekGroupIndex.set(groupKey, result.length);
      result.push({
        ...entry,
        user_label: userLabel(entry.user_id, userMap, translate),
        subject_user_label: subjectUserLabel(entry, userMap),
        is_time_entry_week: true,
        week_start: weekInfo.week_start,
        week_end: weekInfo.week_end,
        week_number: weekInfo.week_number,
        group_count: 1,
        data_summary: translate("audit_time_entries_week_summary", {
          week: weekInfo.week_number,
          from: fmtDateShort(weekInfo.week_start),
          to: fmtDateShort(weekInfo.week_end),
          count: 1,
        }),
      });
    }
  }

  return result;
}
