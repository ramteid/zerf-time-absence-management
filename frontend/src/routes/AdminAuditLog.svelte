<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel } from "../i18n.js";
  import { fmtDateTime } from "../format.js";
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

  function userLabel(userId, userMap) {
    return userMap.get(userId) || (userId == null ? "System" : `#${userId}`);
  }

  function dataSummary(entry) {
    const raw =
      entry.action === "deleted" ? entry.before_data : entry.after_data;
    if (!raw) return "";
    try {
      const parsedData = typeof raw === "string" ? JSON.parse(raw) : raw;
      const keys = [
        "name", "email", "kind", "status", "entry_date",
        "start_date", "end_date", "start_time", "end_time",
        "role", "key", "value",
      ];
      const parts = [];
      for (const fieldKey of keys) {
        if (parsedData[fieldKey] != null) parts.push(`${fieldKey}: ${parsedData[fieldKey]}`);
      }
      return parts.join(", ");
    } catch {
      return "";
    }
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

  $: rows = log.map((entry) => ({
    ...entry,
    user_label: userLabel(entry.user_id, usersById),
    data_summary: dataSummary(entry),
  }));

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
