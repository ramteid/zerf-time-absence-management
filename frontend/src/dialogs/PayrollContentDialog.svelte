<script>
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import { fmtDate, minToHM } from "../format.js";

  // Payload of GET /reports/payroll-content.
  export let content;
  export let onClose;

  // Absences first, then the hours tables, then the catch-up days — the order
  // the report itself prints.
  $: absences = (content?.rows ?? []).filter((row) => row.kind === "absence");
  $: lateAbsences = (content?.rows ?? []).filter(
    (row) => row.kind === "late_absence",
  );
  $: hours = (content?.rows ?? []).filter((row) => row.kind === "hours");
  $: lateHours = (content?.rows ?? []).filter(
    (row) => row.kind === "late_hours",
  );

  function signedMinutes(minutes) {
    const formatted = minToHM(minutes ?? 0);
    return (minutes ?? 0) > 0 ? `+${formatted}` : formatted;
  }
</script>

<Dialog title={$t("Payroll Report")} {onClose}>
  <svelte:fragment slot="title">
    <span class="flex-1">
      {$t("Payroll Report")} · {content.period_label}
    </span>
  </svelte:fragment>

  <div class="fs-13 text-secondary mb-12">
    {#if content.in_progress}
      {$t("What this month is shaping up to report.")}
    {:else if content.sent}
      {$t("What this month's report contained.")}
    {:else}
      {$t("What this month's report will contain.")}
    {/if}
  </div>

  {#if absences.length === 0 && hours.length === 0 && lateHours.length === 0 && lateAbsences.length === 0}
    <div class="payroll-empty">{$t("Nothing to report for this month.")}</div>
  {:else}
    {#if absences.length > 0}
      <div class="report-subheading">{$t("Absences")}</div>
      <div class="payroll-rows">
        {#each absences as row, index (`absence-${index}`)}
          <div class="payroll-row">
            <span class="payroll-name" class:payroll-hidden={!row.name}>
              {row.name ?? $t("Not visible to you")}
            </span>
            <span class="payroll-detail">
              {row.category} · {fmtDate(row.from)} – {fmtDate(row.to)}
            </span>
            <span class="payroll-amount">
              {$t("{days} days").replace("{days}", row.days)}
            </span>
          </div>
        {/each}
      </div>
    {/if}

    {#if hours.length > 0}
      <div class="report-subheading">{$t("Working days and hours")}</div>
      <div class="payroll-rows">
        {#each hours as row, index (`hours-${index}`)}
          <div class="payroll-row">
            <span class="payroll-name" class:payroll-hidden={!row.name}>
              {row.name ?? $t("Not visible to you")}
            </span>
            <span class="payroll-detail">
              {$t("{days} days").replace("{days}", row.days)}
            </span>
            <span class="payroll-amount">{minToHM(row.minutes ?? 0)}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if lateAbsences.length > 0}
      <div class="report-subheading">{$t("Reported later")}</div>
      <div class="fs-13 text-tertiary mb-12">
        {$t(
          "Absences recorded after the report for their own month had already been sent. They go into this month's report with the days they actually cover.",
        )}
      </div>
      <div class="payroll-rows">
        {#each lateAbsences as row, index (`late-absence-${index}`)}
          <div class="payroll-row">
            <span class="payroll-name" class:payroll-hidden={!row.name}>
              {row.name ?? $t("Not visible to you")}
            </span>
            <span class="payroll-detail">
              {row.category} · {fmtDate(row.from)} – {fmtDate(row.to)}
            </span>
            <span class="payroll-amount">
              {$t("{days} days").replace("{days}", row.days)}
            </span>
          </div>
        {/each}
      </div>
    {/if}

    {#if lateHours.length > 0}
      <div class="report-subheading">{$t("Corrections to earlier months")}</div>
      <div class="fs-13 text-tertiary mb-12">
        {$t(
          "Working time that changed after the report for its month was sent. Positive hours add time and negative hours reduce it. This report lists each correction under the day it belongs to.",
        )}
      </div>
      <div class="payroll-rows">
        {#each lateHours as row, index (`late-${index}`)}
          <div class="payroll-row">
            <span class="payroll-name" class:payroll-hidden={!row.name}>
              {row.name ?? $t("Not visible to you")}
            </span>
            <span class="payroll-detail">{fmtDate(row.from)}</span>
            <span class="payroll-amount">{signedMinutes(row.minutes)}</span>
          </div>
        {/each}
      </div>
    {/if}
  {/if}

  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose}>{$t("Close")}</button>
  </svelte:fragment>
</Dialog>

<style>
  .report-subheading {
    font-size: 0.8125rem;
    font-weight: 400;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 12px 0 6px;
  }

  .payroll-rows {
    display: flex;
    flex-direction: column;
    max-height: 50vh;
    overflow-y: auto;
  }

  .payroll-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 4px;
    border-bottom: 1px solid var(--border);
  }

  .payroll-name {
    /* Long names truncate instead of pushing the figures out of the dialog. */
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .payroll-hidden {
    color: var(--text-tertiary);
    font-style: italic;
  }

  .payroll-detail {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
  }

  .payroll-amount {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .payroll-empty {
    padding: 24px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 0.875rem;
  }
</style>
