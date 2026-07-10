<script>
  // Shared paginated log list used by the Audit Log and System Log pages.
  // The parent owns data fetching: it passes the current page of `rows`
  // (each needing a unique `id`) plus `total`, and reacts to `onPageChange`
  // by fetching the requested page — no full page reload.
  //
  // Slots:
  //   row          (let:row)      — content of one list row
  //   detail-title (let:selected) — dialog title for the clicked row
  //   detail       (let:selected) — dialog body for the clicked row
  import Dialog from "./Dialog.svelte";
  import { t } from "./i18n.js";

  export let rows = [];
  export let total = 0;
  // Zero-based page index owned by the parent.
  export let page = 0;
  export let pageSize = 100;
  export let onPageChange = () => {};
  export let emptyText = "";
  // Extra class on row buttons so each page keeps its own styling/selectors.
  export let rowClass = "";

  let selected = null;

  $: pageCount = Math.max(1, Math.ceil(total / pageSize));
</script>

<div class="zf-card log-list">
  {#each rows as row (row.id)}
    <button class="log-row {rowClass}" on:click={() => (selected = row)}>
      <slot name="row" {row} />
    </button>
  {:else}
    <div class="log-empty">{emptyText}</div>
  {/each}
</div>

{#if pageCount > 1}
  <div class="log-pager">
    <button
      class="zf-btn"
      disabled={page === 0}
      on:click={() => onPageChange(page - 1)}
    >
      {$t("Previous")}
    </button>
    <span class="log-pager-status">
      {$t("Page {page} of {count}", { page: page + 1, count: pageCount })}
    </span>
    <button
      class="zf-btn"
      disabled={page >= pageCount - 1}
      on:click={() => onPageChange(page + 1)}
    >
      {$t("Next")}
    </button>
  </div>
{/if}

{#if selected}
  <Dialog onClose={() => (selected = null)} style="max-width: 560px">
    <svelte:fragment slot="title">
      <slot name="detail-title" {selected} />
    </svelte:fragment>
    <slot name="detail" {selected} />
  </Dialog>
{/if}

<style>
  .log-list {
    display: flex;
    flex-direction: column;
  }

  .log-row {
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

  .log-row:first-child {
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
  }

  .log-row:last-child {
    border-bottom: none;
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
  }

  .log-row:only-child {
    border-radius: var(--radius-lg);
  }

  .log-row:hover {
    background: var(--bg-subtle);
  }

  .log-empty {
    padding: 16px;
    font-size: 13px;
    color: var(--text-tertiary);
  }

  .log-pager {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
    margin-top: 14px;
  }

  .log-pager-status {
    font-size: 13px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  @media (max-width: 768px) {
    .log-list {
      overflow-x: auto;
    }

    .log-row {
      flex-wrap: nowrap;
      min-width: max-content;
    }
  }
</style>
