<script>
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import { appTodayDate, fmtMonthName, minToHM } from "../../format.js";

  // Payload of GET /reports/payroll-content, or null while it loads.
  export let content = null;
  export let activeHelp = null;
  export let onHelpToggle = () => {};
  export let onOpen = () => {};
  // Transient peek at the current, in-progress month — see Dashboard.svelte.
  export let onShowCurrentMonth = () => {};

  const currentMonthName = fmtMonthName(appTodayDate());
  // Once the month has been delivered the card steps back: the figures are
  // history, not something anybody still has to act on.
  $: done = !!content?.sent;
  $: hasRows = (content?.rows?.length ?? 0) > 0;
</script>

<div class="zf-card payroll-card" class:is-dimmed={done}>
  <div class="card-header">
    <Icon name="FileText" size={15} sw={1.5} />
    <span class="card-header-title">{$t("Payroll Report")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
      title={$t("help_payroll_report")}
      on:click={() => onHelpToggle("payrollContent")}
    >
      <Icon name="Info" size={14} />
    </button>
  </div>
  {#if activeHelp === "payrollContent"}
    <div class="dashboard-help payroll-help">
      {$t("help_payroll_report").replace("{day}", content?.day_of_month ?? 5)}
    </div>
  {/if}

  <button
    class="payroll-body payroll-card-button"
    on:click={onOpen}
    disabled={!content || !hasRows}
  >
    <div>
      <div class="payroll-headline">
        {$t("{absences} absences · {people} people with hours")
          .replace("{absences}", content?.absence_count ?? 0)
          .replace("{people}", content?.people_with_hours ?? 0)}
      </div>
      <div class="payroll-sub">
        {content?.period_label ?? ""}
        {#if (content?.minutes ?? 0) > 0}
          · {minToHM(content?.minutes ?? 0)}
        {/if}
        {#if content?.in_progress}
          · {$t("still running")}
        {:else if done}
          · {$t("sent")}
        {/if}
      </div>
    </div>
  </button>
  {#if !content?.in_progress}
    <button
      class="zf-btn zf-btn-ghost zf-btn-sm payroll-peek-btn"
      on:click={onShowCurrentMonth}
    >
      {$t("Show {month}").replace("{month}", currentMonthName)}
    </button>
  {/if}
</div>

<style>
  .payroll-card {
    width: 100%;
    text-align: left;
    min-width: 0;
  }

  /* The content is a button so the card remains keyboard reachable while the
     header keeps the same structure as the other dashboard cards. */
  .payroll-card-button {
    font: inherit;
    color: inherit;
    cursor: pointer;
    border: 0;
    background: transparent;
    width: 100%;
    text-align: left;
  }

  .payroll-card-button:hover:not(:disabled) {
    background: var(--bg-subtle);
  }

  .payroll-card-button:disabled {
    cursor: default;
  }

  .is-dimmed {
    opacity: 0.6;
  }

  .payroll-body {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px;
    min-width: 0;
  }

  .payroll-help {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin: 12px 16px 0;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .payroll-headline {
    font-size: 1.125rem;
    font-weight: 600;
  }

  .payroll-sub {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
  }

  .payroll-peek-btn {
    margin: 0 16px 12px;
    padding-left: 0;
    padding-right: 0;
    height: auto;
  }
</style>
