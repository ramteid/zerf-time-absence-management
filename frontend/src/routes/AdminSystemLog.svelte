<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import { fmtDateTime } from "../format.js";
  import LogList from "../LogList.svelte";

  const PAGE_SIZE = 100;
  // Hard cut for the one-line row preview; the dialog shows the full text.
  const PREVIEW_LENGTH = 200;

  let entries = [];
  let total = 0;
  let page = 0;

  async function loadPage(nextPage) {
    const data = await api(
      `/logs?limit=${PAGE_SIZE}&offset=${nextPage * PAGE_SIZE}`,
    );
    entries = data.entries;
    total = data.total;
    page = nextPage;
  }
  loadPage(0);

  function preview(message) {
    // Split into Unicode code points (not UTF-16 code units) so a surrogate
    // pair (e.g. an emoji) straddling the cut-off never gets split in half.
    const chars = Array.from(message);
    return chars.length > PREVIEW_LENGTH
      ? `${chars.slice(0, PREVIEW_LENGTH).join("")}…`
      : message;
  }

  function levelClass(level) {
    return level === "error" ? "action-danger" : "action-warning";
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("System Log")}</h1>
  </div>
</div>

<div class="content-area">
  <LogList
    rows={entries}
    {total}
    {page}
    pageSize={PAGE_SIZE}
    onPageChange={loadPage}
    emptyText={$t("No log entries.")}
    rowClass="syslog-row"
  >
    <svelte:fragment slot="row" let:row>
      <span class="syslog-time">{fmtDateTime(row.occurred_at)}</span>
      <span class="zf-badge {levelClass(row.level)}">
        {row.level === "error" ? $t("Error") : $t("Warning")}
      </span>
      <span class="syslog-message">{preview(row.message)}</span>
    </svelte:fragment>

    <svelte:fragment slot="detail-title" let:selected>
      <span class="zf-badge {levelClass(selected.level)} mr-8">
        {selected.level === "error" ? $t("Error") : $t("Warning")}
      </span>
      <span class="flex-1 fw-500">{$t("Log entry")}</span>
    </svelte:fragment>

    <svelte:fragment slot="detail" let:selected>
      <div class="zf-detail-row">
        <span class="zf-detail-label">{$t("Time")}</span>
        <span>{fmtDateTime(selected.occurred_at)}</span>
      </div>
      <div class="zf-detail-row">
        <span class="zf-detail-label">{$t("Source")}</span>
        <span class="syslog-source">{selected.target}</span>
      </div>
      <!-- Structured tracing fields captured with the event, if any. -->
      {#each Object.entries(selected.fields ?? {}) as [key, value] (key)}
        <div class="zf-detail-row">
          <span class="zf-detail-label">{key}</span>
          <span class="syslog-field-value">{value}</span>
        </div>
      {/each}
      <div class="syslog-full-message">{selected.message}</div>
    </svelte:fragment>
  </LogList>
</div>

<style>
  .syslog-time {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .syslog-message {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
  }

  .syslog-source {
    font-family: ui-monospace, monospace;
    font-size: 0.8125rem;
    word-break: break-all;
  }

  .syslog-field-value {
    word-break: break-word;
  }

  .syslog-full-message {
    margin-top: 6px;
    padding: 10px 12px;
    background: var(--bg-subtle);
    border-radius: var(--radius-md);
    font-family: ui-monospace, monospace;
    font-size: 0.84375rem;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow-y: auto;
  }
</style>
