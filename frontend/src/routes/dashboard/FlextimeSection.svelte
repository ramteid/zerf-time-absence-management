<script>
  import { currentUser } from "../../stores.js";
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import FlextimeChart from "../../FlextimeChart.svelte";
  import DatePicker from "../../DatePicker.svelte";

  export let chartFrom;
  export let chartTo;
  export let todayIso;
  export let chartData = [];
  export let chartLoading = false;
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

    <div class="flextime-controls">
      <div class="flextime-ranges">
        <button class="zf-btn zf-btn-sm" on:click={() => onSetRange(30)}
          >{$t("Last 30 days")}</button
        >
        <button class="zf-btn zf-btn-sm" on:click={() => onSetRange(90)}
          >{$t("Last 90 days")}</button
        >
        <button class="zf-btn zf-btn-sm" on:click={() => onSetRange(182)}
          >{$t("Last 6 months")}</button
        >
        <button class="zf-btn zf-btn-sm" on:click={() => onSetRange(365)}
          >{$t("Last year")}</button
        >
      </div>
      <div class="flextime-date-range">
        <span class="flextime-date-picker">
          <DatePicker
            bind:value={chartFrom}
            min={$currentUser?.start_date}
            max={chartTo}
            class="range-select"
          />
        </span>
        <span class="flextime-date-separator">-</span>
        <span class="flextime-date-picker">
          <DatePicker
            bind:value={chartTo}
            min={chartFrom}
            max={todayIso}
            class="range-select"
          />
        </span>
        <button
          class="zf-btn zf-btn-sm"
          on:click={onLoadChart}
          aria-label={$t("Show")}
        >
          <Icon name="Search" size={13} />
        </button>
      </div>
    </div>
  </div>
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
      <FlextimeChart data={chartData} />
      {#if chartLoading}
        <div class="flextime-loading-inline">
          {$t("Loading...")}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Compact select for choosing the chart range. */
  .flextime-date-picker :global(.range-select) {
    font-size: 0.8125rem;
    padding: 3px 28px 3px 6px;
    height: 28px;
  }

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

  .flextime-controls {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    flex: 0 1 auto;
    flex-wrap: wrap;
    min-width: 0;
  }

  .flextime-ranges,
  .flextime-date-range {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
    flex-wrap: nowrap;
  }

  .flextime-date-picker {
    display: block;
    flex: 0 0 126px;
    width: 126px;
  }

  .flextime-date-separator {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
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

  @media (max-width: 1024px) {
    .flextime-controls {
      width: 100%;
      justify-content: flex-start;
    }

    .flextime-ranges,
    .flextime-date-range {
      flex-wrap: wrap;
    }
  }

  @media (max-width: 640px) {
    .flextime-section {
      padding: 14px;
    }

    .flextime-title-group,
    .flextime-controls,
    .flextime-ranges,
    .flextime-date-range {
      width: 100%;
    }

    .flextime-date-picker {
      flex: 1 1 126px;
      width: auto;
    }
  }
</style>
