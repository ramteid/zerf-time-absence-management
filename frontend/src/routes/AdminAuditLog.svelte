<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel } from "../i18n.js";
  import { fmtDateTime, fmtDateShort } from "../format.js";
  import LogList from "../LogList.svelte";
  import {
    actionClass,
    buildRows,
    extractDetailRows,
  } from "../lib/domain/auditLog.js";

  const PAGE_SIZE = 100;

  let log = [];
  let total = 0;
  let page = 0;
  let usersById = new Map();
  // eslint-disable-next-line no-useless-assignment
  let rows = [];

  // User names are needed to label every row; load them once up front.
  // Swallow failures here (rather than letting them reject `usersLoaded`
  // itself) so a transient error doesn't permanently break every future
  // loadPage() call — rows still render, just with "#id" placeholder labels
  // (see userLabel() in lib/domain/auditLog.js).
  const usersLoaded = api("/users")
    .then((users) => {
      usersById = new Map(
        users.map((user) => [
          user.id,
          `${user.first_name || ""} ${user.last_name || ""}`.trim(),
        ]),
      );
    })
    .catch(() => {});

  async function loadPage(nextPage) {
    // Runs concurrently with the (usually already-resolved) user-name fetch
    // rather than waiting on it first.
    const [data] = await Promise.all([
      api(`/audit-log?limit=${PAGE_SIZE}&offset=${nextPage * PAGE_SIZE}`),
      usersLoaded,
    ]);
    log = data.entries;
    total = data.total;
    page = nextPage;
  }
  loadPage(0);

  // Rebuilds when the page data, user names, or UI language change.
  $: rows = buildRows(log, usersById, $t);
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Audit Log")}</h1>
  </div>
</div>

<div class="content-area">
  <LogList
    {rows}
    {total}
    {page}
    pageSize={PAGE_SIZE}
    onPageChange={loadPage}
    emptyText={$t("No log entries.")}
    rowClass="audit-row"
  >
    <svelte:fragment slot="row" let:row>
      <span class="audit-time">{fmtDateTime(row.occurred_at)}</span>
      <span class="audit-user">{row.user_label}</span>
      {#if row.subject_user_label}
        <span class="audit-subject">→ {row.subject_user_label}</span>
      {/if}
      <span class="zf-badge {actionClass(row.action)}"
        >{auditActionLabel(row.action)}</span
      >
      <span class="audit-table">{auditTableLabel(row.table_name)}</span>
      {#if row.data_summary}
        <span class="audit-data">{row.data_summary}</span>
      {/if}
    </svelte:fragment>

    <svelte:fragment slot="detail-title" let:selected>
      <span class="zf-badge {actionClass(selected.action)} mr-8"
        >{auditActionLabel(selected.action)}</span
      >
      <span class="flex-1 fw-500">{auditTableLabel(selected.table_name)}</span>
    </svelte:fragment>

    <svelte:fragment slot="detail" let:selected>
      <div class="zf-detail-row">
        <span class="zf-detail-label">{$t("Time")}</span>
        <span>{fmtDateTime(selected.occurred_at)}</span>
      </div>
      <div class="zf-detail-row">
        <span class="zf-detail-label">{$t("User")}</span>
        <span>{selected.user_label}</span>
      </div>
      {#if selected.subject_user_label}
        <div class="zf-detail-row">
          <span class="zf-detail-label">{$t("For")}</span>
          <span>{selected.subject_user_label}</span>
        </div>
      {/if}
      {#if selected.is_time_entry_week}
        <div class="zf-detail-row">
          <span class="zf-detail-label">{$t("Week")}</span>
          <span
            >{$t("Week {week}: {from} - {to}", {
              week: selected.week_number,
              from: fmtDateShort(selected.week_start),
              to: fmtDateShort(selected.week_end),
            })}</span
          >
        </div>
        <div class="zf-detail-row">
          <span class="zf-detail-label">{$t("Days")}</span>
          <span>{selected.group_count}</span>
        </div>
      {:else}
        {#each extractDetailRows(selected, usersById, $t) ?? [] as field (field.label)}
          <div class="zf-detail-row">
            <span class="zf-detail-label">{field.label}</span>
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
    </svelte:fragment>
  </LogList>
</div>

<style>
  .audit-time {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .audit-user {
    color: var(--text-secondary);
    font-size: 0.8125rem;
    white-space: nowrap;
  }

  .audit-subject {
    color: var(--text-secondary);
    font-size: 0.8125rem;
    white-space: nowrap;
  }

  .audit-table {
    font-weight: 500;
    white-space: nowrap;
  }

  .audit-data {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 300px;
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
