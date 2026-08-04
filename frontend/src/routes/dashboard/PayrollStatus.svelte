<script>
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import StatusDonut from "../../lib/ui/StatusDonut.svelte";

  // Payload of GET /reports/payroll-status, or null while it loads.
  export let status = null;
  export let activeHelp = null;
  export let onHelpToggle = () => {};
  export let onOpen = () => {};

  // Once the month's report has gone out there is nothing left to chase, so
  // the tile steps back: dimmed, no donut, and not clickable.
  $: done = !!status?.sent;
</script>

<div class="zf-card payroll-card" class:is-dimmed={done}>
  <div class="card-header">
    <Icon name="FileText" size={15} sw={1.5} />
    <span class="card-header-title">{$t("Payroll Report")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
      title={$t("help_payroll_report")}
      on:click={() => onHelpToggle("payroll")}
    >
      <Icon name="Info" size={14} />
    </button>
  </div>
  {#if activeHelp === "payroll"}
    <div class="dashboard-help payroll-help">
      {$t("help_payroll_report").replace("{day}", status?.day_of_month ?? 5)}
    </div>
  {/if}

  {#if done}
    <div class="payroll-body">
      <Icon name="Check" size={20} />
      <div>
        <div class="payroll-headline">
          {$t("{month} sent").replace("{month}", status?.period_label ?? "")}
        </div>
        <div class="payroll-sub">{$t("Nothing left to do this month.")}</div>
      </div>
    </div>
  {:else}
    <button
      class="payroll-body payroll-card-button"
      on:click={onOpen}
      disabled={!status}
    >
      <StatusDonut
        ready={status?.ready ?? 0}
        awaitingApproval={status?.awaiting_approval ?? 0}
        notSubmitted={status?.not_submitted ?? 0}
      />
      <div>
        <div class="payroll-headline">
          {$t("{ready} of {total} done")
            .replace("{ready}", status?.ready ?? 0)
            .replace("{total}", status?.total ?? 0)}
        </div>
        <div class="payroll-sub">{status?.period_label ?? ""}</div>
      </div>
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
</style>
