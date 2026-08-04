<script>
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import { go } from "../stores.js";

  // Payload of GET /reports/payroll-status.
  export let status;
  export let onClose;

  // Rows the requester may not see arrive without an id or name — a team lead
  // still learns that somebody is holding the month up, just not who.
  function openReport(member) {
    if (!member.user_id) return;
    go(`/reports?user=${member.user_id}&from=${status.from}&to=${status.to}`);
    onClose();
  }

  const CHIP_CLASS = {
    ready: "zf-chip-approved",
    awaiting_approval: "zf-chip-pending",
    not_submitted: "zf-chip-rejected",
  };

  const STATUS_LABEL = {
    ready: "Done",
    awaiting_approval: "Waiting for approval",
    not_submitted: "Not submitted",
  };

  // Missing people first — that is what the reader opened this list for.
  const ORDER = { not_submitted: 0, awaiting_approval: 1, ready: 2 };
  $: rows = [...(status?.members ?? [])].sort(
    (a, b) =>
      ORDER[a.status] - ORDER[b.status] ||
      (a.name ?? "").localeCompare(b.name ?? ""),
  );
</script>

<Dialog title={$t("Payroll Report")} {onClose}>
  <svelte:fragment slot="title">
    <span class="flex-1">
      {$t("Payroll Report")} · {status.period_label}
    </span>
  </svelte:fragment>

  <div class="fs-13 text-secondary mb-12">
    {$t("{ready} of {total} done")
      .replace("{ready}", status.ready)
      .replace("{total}", status.total)}
  </div>

  {#if rows.length === 0}
    <div class="payroll-empty">{$t("No people in this month.")}</div>
  {:else}
    <div class="payroll-rows">
      {#each rows as member, index (member.user_id ?? `hidden-${index}`)}
        {#if member.user_id}
          <button
            class="payroll-row payroll-row-link"
            on:click={() => openReport(member)}
          >
            <span class="payroll-name">{member.name}</span>
            <span class="zf-chip zf-chip-sm {CHIP_CLASS[member.status]}">
              {$t(STATUS_LABEL[member.status])}
            </span>
          </button>
        {:else}
          <div class="payroll-row">
            <span class="payroll-name payroll-hidden">
              {$t("Not visible to you")}
            </span>
            <span class="zf-chip zf-chip-sm {CHIP_CLASS[member.status]}">
              {$t(STATUS_LABEL[member.status])}
            </span>
          </div>
        {/if}
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

  .payroll-hidden {
    color: var(--text-tertiary);
    font-style: italic;
  }

  .payroll-empty {
    padding: 24px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 0.875rem;
  }
</style>
