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

<div class="dashboard-group">
  <div class="dashboard-group-label zf-row">
    {$t("Payroll Report")}
    <button
      class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
      title={$t("help_payroll_report")}
      on:click={() => onHelpToggle("payroll")}
    >
      <Icon name="Info" size={14} />
    </button>
  </div>
  {#if activeHelp === "payroll"}
    <div class="dashboard-help">
      {$t("help_payroll_report").replace("{day}", status?.day_of_month ?? 5)}
    </div>
  {/if}

  {#if done}
    <div class="zf-card payroll-card is-dimmed">
      <div class="payroll-body">
        <Icon name="Check" size={20} />
        <div>
          <div class="payroll-headline">
            {$t("{month} sent").replace("{month}", status?.period_label ?? "")}
          </div>
          <div class="payroll-sub">{$t("Nothing left to do this month.")}</div>
        </div>
      </div>
    </div>
  {:else}
    <button
      class="zf-card payroll-card payroll-card-button"
      on:click={onOpen}
      disabled={!status}
    >
      <div class="payroll-body">
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
      </div>
    </button>
  {/if}
</div>

<style>
  .payroll-card {
    padding: 16px;
    width: 100%;
    text-align: left;
  }

  /* The clickable variant is a <button> so it is keyboard reachable; strip the
     button chrome so it still reads as a card. */
  .payroll-card-button {
    font: inherit;
    color: inherit;
    cursor: pointer;
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
    /* Keeps the text from pushing the card wider than its grid column. */
    min-width: 0;
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
