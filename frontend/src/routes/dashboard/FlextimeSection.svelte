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
    <Icon name="TrendingUp" size={15} sw={1.5} />
    <span class="flextime-title">{$t("Flextime balance")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost"
      title={$t("help_flextime_chart")}
      on:click={() => onHelpToggle("flextime")}
      style="color:var(--text-tertiary);font-size:14px;cursor:help"
    >
      <Icon name="Info" size={14} />
    </button>
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
      <DatePicker
        bind:value={chartFrom}
        min={$currentUser?.start_date}
        max={chartTo}
        style="font-size:12px;padding:3px 6px;height:28px"
      />
      <span class="flextime-date-separator">-</span>
      <DatePicker
        bind:value={chartTo}
        min={chartFrom}
        max={todayIso}
        style="font-size:12px;padding:3px 6px;height:28px"
      />
      <button class="zf-btn zf-btn-sm" on:click={onLoadChart} aria-label={$t("Show")}>
        <Icon name="Search" size={13} />
      </button>
    </div>
  </div>
  {#if activeHelp === "flextime"}
    <div class="flextime-help">
      {$t("help_flextime_chart")}
    </div>
  {/if}
  {#if chartLoading}
    <div class="flextime-loading">
      {$t("Loading...")}
    </div>
  {:else}
    <FlextimeChart data={chartData} />
  {/if}
</div>

<style>
  .flextime-section {
    padding: 16px 20px;
    margin-top: 16px;
  }

  .flextime-header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }

  .flextime-title {
    font-size: 14px;
    font-weight: 400;
    flex: 1;
  }

  .flextime-ranges,
  .flextime-date-range {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }

  .flextime-date-range {
    align-items: center;
  }

  .flextime-date-separator {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .flextime-help {
    font-size: 12px;
    color: var(--text-tertiary);
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .flextime-loading {
    text-align: center;
    padding: 40px 0;
    font-size: 13px;
    color: var(--text-tertiary);
  }
</style>
