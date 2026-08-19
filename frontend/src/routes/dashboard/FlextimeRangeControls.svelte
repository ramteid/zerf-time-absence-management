<script>
  // Range controls for a flextime chart: four quick-range buttons plus a
  // from/to pair with a search button. Used by both the dashboard chart and the
  // employee report chart, which keep their own from/to state and only get
  // told when to reload.
  import { currentUser } from "../../stores.js";
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";

  export let from;
  export let to;
  export let todayIso;
  /// Earliest selectable date. Defaults to the signed-in user's start date;
  /// the report view passes the inspected employee's start date instead.
  export let minDate = undefined;
  export let onSetRange = () => {};
  export let onLoad = () => {};

  $: minSelectable = minDate ?? $currentUser?.start_date;
</script>

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
        bind:value={from}
        min={minSelectable}
        max={to}
        class="zf-input range-select"
      />
    </span>
    <span class="flextime-date-separator">-</span>
    <span class="flextime-date-picker">
      <DatePicker
        bind:value={to}
        min={from}
        max={todayIso}
        class="zf-input range-select"
      />
    </span>
    <button class="zf-btn zf-btn-sm" on:click={onLoad} aria-label={$t("Show")}>
      <Icon name="Search" size={13} />
    </button>
  </div>
</div>

<style>
  /* Compact date inputs for choosing the chart range. They keep the shared
     `.zf-input` styling (border, surface background, light/dark theming and
     width: 100%) and only shrink the height and type scale to align with the
     range/search buttons next to them. The `.zf-input.range-select` compound
     outweighs both the base `.zf-input` rule and DatePicker's
     `.date-picker-wrap :global(.zf-input)`, so these overrides always win. */
  .flextime-date-picker :global(.zf-input.range-select) {
    height: 28px;
    font-size: 0.84375rem;
  }

  .flextime-controls {
    display: flex;
    align-items: center;
    /* Left-aligned: the surrounding header pushes the whole block to the right
       when it shares the title's row, so this only decides how the button and
       date rows line up once they wrap onto two lines. */
    justify-content: flex-start;
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
    /* flex: 0 1 auto allows the row to shrink when the tile is narrow;
       min-width: 0 prevents flex children from overflowing their container. */
    flex: 0 1 auto;
    min-width: 0;
  }

  .flextime-date-picker {
    display: block;
    /* flex-shrink: 1 allows the picker to shrink below 126px when the tile
       is too narrow, preventing overflow beyond the card boundary. */
    flex: 0 1 126px;
    width: 126px;
    min-width: 80px;
  }

  .flextime-date-separator {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
  }

  @media (max-width: 1024px) {
    .flextime-controls {
      width: 100%;
    }

    .flextime-ranges,
    .flextime-date-range {
      flex-wrap: wrap;
    }
  }

  @media (max-width: 640px) {
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
