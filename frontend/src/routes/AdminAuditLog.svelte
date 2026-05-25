<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel, absenceKindLabel } from "../i18n.js";
  import { fmtDateTime, fmtDate, fmtDateShort, parseDate, monday, addDays, isoDate, isoWeek } from "../format.js";
  import Dialog from "../Dialog.svelte";

  let log = [];
  let usersById = new Map();
  // eslint-disable-next-line no-useless-assignment
  let rows = [];
  let selected = null;

  async function load() {
    const [entries, users] = await Promise.all([
      api("/audit-log"),
      api("/users"),
    ]);
    log = entries;
    usersById = new Map(
      users.map((user) => [
        user.id,
        `${user.first_name || ""} ${user.last_name || ""}`.trim(),
      ]),
    );
  }
  load();

  function safeParseJson(raw) {
    if (!raw) return null;
    try {
      return typeof raw === "string" ? JSON.parse(raw) : raw;
    } catch {
      return null;
    }
  }

  function relevantPayload(entry) {
    const payload = entry.action === "deleted" ? entry.before_data : entry.after_data;
    return safeParseJson(payload);
  }

  function weekInfoFromEntry(entry) {
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

  function summarize(entry, translate) {
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

  function userLabel(userId, userMap, translate) {
    return userMap.get(userId) || (userId == null ? translate("audit_system_user") : `#${userId}`);
  }

  // Returns the ID of the user whose data is being acted on (may differ from the acting user).
  // For "users" table: the record itself is the subject. For other tables: look in the payload.
  function subjectUserId(entry) {
    if (entry.table_name === "users") return entry.record_id ?? null;
    const payload = relevantPayload(entry);
    return payload?.user_id ?? null;
  }

  function subjectUserLabel(entry, userMap) {
    const subjectId = subjectUserId(entry);
    if (subjectId == null || subjectId === entry.user_id) return null;
    return userMap.get(subjectId) || `#${subjectId}`;
  }

  // Which fields to show in the detail popup, per table
  const TABLE_FIELDS = {
    time_entries:    ["entry_date", "start_time", "end_time", "status", "note"],
    users:           ["first_name", "last_name", "email", "role", "active"],
    absences:        ["kind", "start_date", "end_date", "status", "note"],
    categories:      ["name", "color", "description", "counts_as_work", "active"],
    holidays:        ["name", "holiday_date"],
    app_settings:    ["key", "value"],
    reopen_requests: ["week_start_date", "status"],
  };

  const FIELD_LABEL_KEYS = {
    entry_date:     "Date",
    start_time:     "Start",
    end_time:       "End",
    status:         "Status",
    note:           "Note",
    first_name:     "First name",
    last_name:      "Last name",
    email:          "Email",
    role:           "Role",
    active:         "Active",
    kind:           "Type",
    start_date:     "From",
    end_date:       "To",
    name:           "Name",
    color:          "Color",
    description:    "Description",
    counts_as_work: "Counts as work",
    holiday_date:   "Date",
    key:            "Setting",
    value:          "Value",
    week_start_date: "Week start",
  };

  const DATE_FIELDS = new Set([
    "entry_date", "holiday_date", "start_date", "end_date", "week_start_date",
  ]);

  function fmtFieldVal(key, val, userMap, translate) {
    if (val == null) return null;
    if (key === "user_id") return userMap.get(val) || `#${val}`;
    if (DATE_FIELDS.has(key)) { try { return fmtDate(val); } catch { return String(val); } }
    if (key === "kind") return absenceKindLabel(val);
    if (typeof val === "boolean") return val ? translate("Yes") : translate("No");
    return String(val);
  }

  function extractDetailRows(entry, userMap, translate) {
    const fields = TABLE_FIELDS[entry.table_name];
    if (!fields) return null;

    const before = safeParseJson(entry.before_data);
    const after  = safeParseJson(entry.after_data);
    const hasBoth = before != null && after != null;
    const result = [];

    for (const key of fields) {
      const bFmt = fmtFieldVal(key, before?.[key] ?? null, userMap, translate);
      const aFmt = fmtFieldVal(key, after?.[key]  ?? null, userMap, translate);
      if (bFmt == null && aFmt == null) continue;   // nothing to show
      if (hasBoth && bFmt === aFmt) continue;        // unchanged in an update
      result.push({
        label: translate(FIELD_LABEL_KEYS[key] ?? key),
        before: bFmt,
        after: aFmt,
      });
    }

    return result.length > 0 ? result : null;
  }

  $: selectedDetails = selected ? extractDetailRows(selected, usersById, $t) : null;

  function actionClass(action) {
    if (action === "created" || action === "approved" || action === "reopened") return "action-success";
    if (action === "deleted" || action === "rejected" || action === "deactivated") return "action-danger";
    if (action === "updated" || action === "status_changed") return "action-info";
    return "action-muted";
  }

  function buildRows(entries, userMap, translate) {
    const result = [];
    // Maps "(user_id):(action):(week_start)" -> index in result
    const weekGroupIndex = new Map();

    for (const entry of entries) {
      const weekInfo = entry.table_name === "time_entries"
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

  $: rows = buildRows(log, usersById, $t);

  function openDetail(entry) {
    selected = entry;
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Audit Log")}</h1>
  </div>
</div>

<div class="content-area">
  <div class="zf-card audit-list">
    {#each rows as entry (entry.id)}
      <button class="audit-row" on:click={() => openDetail(entry)}>
        <span class="audit-time">{fmtDateTime(entry.occurred_at)}</span>
        <span class="audit-user">{entry.user_label}</span>
        {#if entry.subject_user_label}
          <span class="audit-subject">→ {entry.subject_user_label}</span>
        {/if}
        <span class="audit-action {actionClass(entry.action)}">{auditActionLabel(entry.action)}</span>
        <span class="audit-table">{auditTableLabel(entry.table_name)}</span>
        {#if entry.data_summary}
          <span class="audit-data">{entry.data_summary}</span>
        {/if}
      </button>
    {/each}
  </div>
</div>

{#if selected}
  <Dialog
    onClose={() => (selected = null)}
    style="max-width: 560px"
  >
    <svelte:fragment slot="title">
      <span class="audit-action {actionClass(selected.action)}" style="margin-right:8px">{auditActionLabel(selected.action)}</span>
      <span style="flex:1;font-weight:500">{auditTableLabel(selected.table_name)}</span>
    </svelte:fragment>
    <div class="detail-row">
      <span class="detail-label">{$t("Time")}</span>
      <span>{fmtDateTime(selected.occurred_at)}</span>
    </div>
    <div class="detail-row">
      <span class="detail-label">{$t("User")}</span>
      <span>{selected.user_label}</span>
    </div>
    {#if selected.subject_user_label}
      <div class="detail-row">
        <span class="detail-label">{$t("For")}</span>
        <span>{selected.subject_user_label}</span>
      </div>
    {/if}
    {#if selected.is_time_entry_week}
      <div class="detail-row">
        <span class="detail-label">{$t("Week")}</span>
        <span>{$t("Week {week}: {from} - {to}", {
          week: selected.week_number,
          from: fmtDateShort(selected.week_start),
          to: fmtDateShort(selected.week_end),
        })}</span>
      </div>
      <div class="detail-row">
        <span class="detail-label">{$t("Days")}</span>
        <span>{selected.group_count}</span>
      </div>
    {:else}
      {#each selectedDetails ?? [] as field (field.label)}
        <div class="detail-field-row">
          <span class="detail-label">{field.label}</span>
          <span class="detail-value">
            {#if field.before != null && field.after != null}
              <span class="detail-old">{field.before}</span>
              <span class="detail-sep"> → </span>
              <span class="detail-new">{field.after}</span>
            {:else if field.after != null}
              {field.after}
            {:else}
              {field.before}
            {/if}
          </span>
        </div>
      {/each}
    {/if}
  </Dialog>
{/if}

<style>
  .audit-list {
    display: flex;
    flex-direction: column;
  }

  .audit-row {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    padding: 9px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    cursor: pointer;
    background: none;
    border-radius: 0;
    border-left: none;
    border-right: none;
    border-top: none;
    text-align: left;
    width: 100%;
    color: var(--text-primary);
    font-family: inherit;
    transition: background 0.1s;
  }

  .audit-row:first-child {
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
  }

  .audit-row:last-child {
    border-bottom: none;
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
  }

  .audit-row:only-child {
    border-radius: var(--radius-lg);
  }

  .audit-row:hover {
    background: var(--bg-subtle);
  }

  .audit-time {
    color: var(--text-tertiary);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .audit-user {
    color: var(--text-secondary);
    font-size: 12px;
    white-space: nowrap;
  }

  .audit-subject {
    color: var(--text-secondary);
    font-size: 12px;
    white-space: nowrap;
  }

  .audit-action {
    display: inline-block;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  .action-success {
    background: var(--success-soft);
    color: var(--success-text);
  }

  .action-danger {
    background: var(--danger-soft);
    color: var(--danger-text);
  }

  .action-info {
    background: var(--info-soft);
    color: var(--info-text);
  }

  .action-muted {
    background: var(--bg-muted);
    color: var(--text-secondary);
  }

  .audit-table {
    font-weight: 500;
    white-space: nowrap;
  }

  .audit-data {
    color: var(--text-tertiary);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 300px;
  }

  @media (max-width: 768px) {
    .audit-list {
      overflow-x: auto;
    }

    .audit-row {
      flex-wrap: nowrap;
      min-width: max-content;
    }
  }

  .detail-row {
    display: flex;
    gap: 12px;
    align-items: baseline;
    font-size: 13px;
  }

  .detail-label {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    min-width: 80px;
    flex-shrink: 0;
  }

  .detail-field-row {
    display: flex;
    gap: 12px;
    align-items: baseline;
    font-size: 13px;
  }

  .detail-value {
    display: flex;
    align-items: baseline;
    gap: 4px;
    flex-wrap: wrap;
  }

  .detail-old {
    color: var(--text-tertiary);
    text-decoration: line-through;
  }

  .detail-sep {
    color: var(--text-tertiary);
  }

  .detail-new {
    color: var(--text-primary);
  }
</style>
