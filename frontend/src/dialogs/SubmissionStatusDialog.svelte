<script>
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import { go } from "../stores.js";

  // Payload of GET /reports/submission-status.
  export let status;
  export let onClose;

  // Opens the person's report in month mode, so it judges the month exactly
  // as this list did.
  function openReport(member) {
    go(`/reports?user=${member.user_id}&month=${status.period}`);
    onClose();
  }

  const CHIP_CLASS = {
    ready: "zf-chip-approved",
    awaiting_approval: "zf-chip-pending",
    not_submitted: "zf-chip-rejected",
  };

  // Nobody is done with a month that is still running — only up to date.
  $: statusLabel = {
    ready: status?.in_progress ? "Up to date" : "Done",
    awaiting_approval: "Waiting for approval",
    not_submitted: "Not submitted",
  };

  // Missing people first — that is what the reader opened this list for.
  const ORDER = { not_submitted: 0, awaiting_approval: 1, ready: 2 };
  $: rows = [...(status?.members ?? [])].sort(
    (a, b) => ORDER[a.status] - ORDER[b.status] || a.name.localeCompare(b.name),
  );
</script>

<Dialog title={$t("Submissions")} {onClose}>
  <svelte:fragment slot="title">
    <span class="flex-1">
      {$t("Submissions")} · {status.period_label}
      {#if status.in_progress}
        · {$t("still running")}
      {/if}
    </span>
  </svelte:fragment>

  {#if status.total > 0}
    <div class="fs-13 text-secondary mb-12">
      {$t(
        status.in_progress
          ? "{ready} of {total} up to date"
          : "{ready} of {total} done",
      )
        .replace("{ready}", status.ready)
        .replace("{total}", status.total)}
    </div>
  {/if}

  {#if rows.length === 0}
    <div class="payroll-empty">{$t("No people in this month.")}</div>
  {:else}
    <div class="payroll-rows">
      {#each rows as member (member.user_id)}
        <button
          class="payroll-row payroll-row-link"
          on:click={() => openReport(member)}
        >
          <span class="payroll-name">{member.name}</span>
          <span class="zf-chip zf-chip-sm {CHIP_CLASS[member.status]}">
            {$t(statusLabel[member.status])}
          </span>
        </button>
      {/each}
    </div>
  {/if}

  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose}>{$t("Close")}</button>
  </svelte:fragment>
</Dialog>

<style>
  .payroll-rows {
    display: flex;
    flex-direction: column;
    max-height: 60vh;
    overflow-y: auto;
  }

  .payroll-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    padding: 10px 4px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: none;
    text-align: left;
    font: inherit;
    color: inherit;
  }

  .payroll-row-link {
    cursor: pointer;
  }

  .payroll-row-link:hover {
    background: var(--bg-subtle);
  }

  .payroll-name {
    /* Long names truncate instead of pushing the chip out of the dialog. */
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .payroll-empty {
    padding: 24px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 0.875rem;
  }
</style>
