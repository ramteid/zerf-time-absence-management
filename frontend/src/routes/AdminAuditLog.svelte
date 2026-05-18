<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel, absenceKindLabel } from "../i18n.js";
  import { fmtDateTime, fmtDate, fmtDateShort, parseDate, monday, addDays, isoDate, isoWeek } from "../format.js";
  import Dialog from "../Dialog.svelte";

  let log = [];
  let usersById = new Map();
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

  const TIME_ENTRY_GROUP_WINDOW_MS = 45 * 1000;

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

  function formatJson(raw) {
    if (!raw) return null;
    try {
      const parsed = typeof raw === "string" ? JSON.parse(raw) : raw;
      return JSON.stringify(parsed, null, 2);
    } catch {
      return String(raw);
    }
  }

  function actionClass(action) {
    if (action === "created" || action === "approved" || action === "reopened") return "action-success";
    if (action === "deleted" || action === "rejected" || action === "deactivated") return "action-danger";
    if (action === "updated" || action === "status_changed") return "action-info";
    return "action-muted";
  }

  function canMergeTimeEntryRows(previous, current, currentWeek) {
    if (!previous || !currentWeek || !previous.is_time_entry_week) return false;
    if (previous.table_name !== "time_entries" || current.table_name !== "time_entries") return false;
    if (previous.action !== current.action || previous.user_id !== current.user_id) return false;
    if (previous.week_start !== currentWeek.week_start) return false;

    const previousTs = Date.parse(previous.occurred_at);
    const currentTs = Date.parse(current.occurred_at);
    if (Number.isNaN(previousTs) || Number.isNaN(currentTs)) return false;
    return Math.abs(previousTs - currentTs) <= TIME_ENTRY_GROUP_WINDOW_MS;
  }

  function buildRows(entries, userMap, translate) {
    const nextRows = [];

    for (const entry of entries) {
      const baseRow = {
        ...entry,
        user_label: userLabel(entry.user_id, userMap, translate),
        data_summary: summarize(entry, translate),
        is_time_entry_week: false,
      };

      const weekInfo = weekInfoFromEntry(entry);
      if (!weekInfo) {
        nextRows.push(baseRow);
        continue;
      }

      const previous = nextRows[nextRows.length - 1];
      if (canMergeTimeEntryRows(previous, entry, weekInfo)) {
        previous.group_count += 1;
        previous.data_summary = translate("audit_time_entries_week_summary", {
          week: previous.week_number,
          from: fmtDateShort(previous.week_start),
          to: fmtDateShort(previous.week_end),
          count: previous.group_count,
        });
        continue;
      }

      nextRows.push({
        ...baseRow,
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

    return nextRows;
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
    {#each rows as entry}
      <button class="audit-row" on:click={() => openDetail(entry)}>
        <span class="audit-time">{fmtDateTime(entry.occurred_at)}</span>
        <span class="audit-user">{entry.user_label}</span>
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
      {#if selected.before_data}
        <div class="detail-section">
          <span class="detail-label">{$t("Before")}</span>
          <pre class="detail-json">{formatJson(selected.before_data)}</pre>
        </div>
      {/if}
      {#if selected.after_data}
        <div class="detail-section">
          <span class="detail-label">{$t("After")}</span>
          <pre class="detail-json">{formatJson(selected.after_data)}</pre>
        </div>
      {/if}
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


  .detail-row {
    display: flex;
    gap: 12px;
    align-items: baseline;
    font-size: 13px;
  }

  .detail-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
  }

  .detail-label {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    min-width: 48px;
  }

  .detail-json {
    background: var(--bg-subtle);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 10px 12px;
    font-size: 12px;
    font-family: ui-monospace, monospace;
    overflow-x: auto;
    white-space: pre;
    margin: 0;
    color: var(--text-secondary);
    max-height: 300px;
    overflow-y: auto;
  }
</style>
