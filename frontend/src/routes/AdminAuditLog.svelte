<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel } from "../i18n.js";
  import { fmtDateTime, fmtDateShort } from "../format.js";
  import Dialog from "../Dialog.svelte";
  import {
    actionClass,
    buildRows,
    extractDetailRows,
  } from "../lib/domain/auditLog.js";

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

  $: rows = buildRows(log, usersById, $t);
  $: selectedDetails = selected ? extractDetailRows(selected, usersById, $t) : null;

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
