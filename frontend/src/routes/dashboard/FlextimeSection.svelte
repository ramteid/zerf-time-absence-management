<script>
  import { t } from "../../i18n.js";
  import { fmtDate } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import FlextimeChart from "../../FlextimeChart.svelte";
  import FlextimeRangeControls from "./FlextimeRangeControls.svelte";

  export let chartFrom;
  export let chartTo;
  export let todayIso;
  export let chartData = [];
  export let chartLoading = false;
  /// End of the last fully approved week — everything after it is flat in the
  /// chart, so the date is spelled out above it.
  export let balanceAsOf = null;
  export let activeHelp = null;
  export let onHelpToggle = () => {};
  export let onSetRange = () => {};
  export let onLoadChart = () => {};
</script>

<div class="zf-card flextime-section">
  <div class="flextime-header">
    <div class="flextime-title-group">
      <Icon name="TrendingUp" size={15} sw={1.5} />
      <span class="flextime-title">{$t("Flextime balance")}</span>
      <button
        class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
        title={$t("help_flextime_chart")}
        on:click={() => onHelpToggle("flextime")}
      >
        <Icon name="Info" size={14} />
      </button>
    </div>

    <FlextimeRangeControls
      bind:from={chartFrom}
      bind:to={chartTo}
      {todayIso}
      {onSetRange}
      onLoad={onLoadChart}
    />
  </div>
  {#if balanceAsOf}
    <div class="flextime-as-of">
      {$t("As of {date}", { date: fmtDate(balanceAsOf) })}
    </div>
  {/if}
  {#if activeHelp === "flextime"}
    <div class="flextime-help">
      {$t("help_flextime_chart")}
    </div>
  {/if}
  {#if chartLoading && chartData.length === 0}
    <div class="flextime-loading">
      {$t("Loading...")}
    </div>
  {:else}
    <div class:flextime-chart-busy={chartLoading} class="flextime-chart-wrap">
      <FlextimeChart data={chartData} asOf={balanceAsOf} />
      {#if chartLoading}
        <div class="flextime-loading-inline">
          {$t("Loading...")}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .flextime-section {
    padding: 16px 20px;
    margin-top: 16px;
  }

  .flextime-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px 16px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }

  .flextime-title-group {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1 1 190px;
  }

  .flextime-title {
    font-size: 0.9375rem;
    font-weight: 400;
    min-width: 0;
  }

  .flextime-as-of {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin-bottom: 10px;
  }

  .flextime-help {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .flextime-loading {
    text-align: center;
    padding: 40px 0;
    font-size: 0.875rem;
    color: var(--text-tertiary);
  }

  .flextime-chart-wrap {
    position: relative;
    min-height: 230px;
  }

  .flextime-chart-busy {
    cursor: progress;
  }

  .flextime-loading-inline {
    position: absolute;
    top: 8px;
    right: 8px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-tertiary);
    font-size: 0.8125rem;
    box-shadow: var(--shadow-sm);
    pointer-events: none;
  }

  @media (max-width: 640px) {
    .flextime-section {
      padding: 14px;
    }

    .flextime-title-group {
      width: 100%;
    }
  }
</style>
