<script>
  // Shared period selector for the Reports page: a month stepper (◀/▶, capped
  // at minMonth/maxMonth — matches the Calendar/Time month-nav pattern) with a
  // toggle into a free "from/to" range for cases the month grid can't express
  // (e.g. a quarter, or a range spanning into the future for planned absences).
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";
  import DateRangeFields from "./DateRangeFields.svelte";
  import { addDays, isoDate } from "../../format.js";
  import {
    monthEnd,
    monthStart,
    REPORT_RANGE_MAX_DAY_DIFFERENCE,
  } from "../domain/dates.js";

  export let mode = "month"; // "month" | "range"
  export let month = ""; // "YYYY-MM"
  export let from = "";
  export let to = "";
  export let minMonth = null;
  export let maxMonth = null;
  export let minDate = null;
  export let maxDate = null;
  // The report backend accepts 366 dates inclusive, which means its ISO
  // endpoints can be at most 365 calendar days apart.
  export let maxRangeDays = REPORT_RANGE_MAX_DAY_DIFFERENCE;
  export let id = "period"; // base id, suffixed per rendered input

  function shiftMonth(value, delta) {
    const [year, monthNum] = value.split("-").map(Number);
    const target = new Date(year, monthNum - 1 + delta, 1);
    return `${target.getFullYear()}-${String(target.getMonth() + 1).padStart(2, "0")}`;
  }

  $: prevDisabled = !!minMonth && month <= minMonth;
  $: nextDisabled = !!maxMonth && month >= maxMonth;

  function goPrev() {
    if (prevDisabled) return;
    month = shiftMonth(month, -1);
  }
  function goNext() {
    if (nextDisabled) return;
    month = shiftMonth(month, 1);
  }

  // Switching modes carries the current selection across so nothing is lost:
  // month → range seeds from/to with that month's bounds; range → month keeps
  // the last month value untouched.
  function switchToRange() {
    from = monthStart(month);
    to = monthEnd(month);
    mode = "range";
  }
  function switchToMonth() {
    mode = "month";
  }

  function shiftIsoDate(value, days) {
    if (!value || isoDate(value) !== value) return "";
    return isoDate(addDays(value, days));
  }

  function earlierIsoDate(first, second) {
    if (!first) return second || "";
    if (!second) return first;
    return first < second ? first : second;
  }

  function laterIsoDate(first, second) {
    if (!first) return second || "";
    if (!second) return first;
    return first > second ? first : second;
  }

  // Keep both date pickers inside the backend's inclusive 366-day window.
  // `from` constrains the latest possible `to`, and `to` constrains the
  // earliest possible `from`; DateRangeFields also preserves chronological
  // and page-level min/max bounds.
  $: rangeMinFrom = laterIsoDate(minDate, shiftIsoDate(to, -maxRangeDays));
  $: rangeMaxTo = earlierIsoDate(maxDate, shiftIsoDate(from, maxRangeDays));
</script>

{#if mode === "month"}
  <div class="period-picker period-picker-month">
    <div class="zf-nav-slider">
      <button
        type="button"
        class="zf-btn zf-btn-ghost"
        aria-label={$t("Previous month")}
        on:click={goPrev}
        disabled={prevDisabled}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <div class="period-month-input">
        <DatePicker
          id="{id}-month"
          mode="month"
          bind:value={month}
          min={minMonth}
          max={maxMonth}
        />
      </div>
      <button
        type="button"
        class="zf-btn zf-btn-ghost"
        aria-label={$t("Next month")}
        on:click={goNext}
        disabled={nextDisabled}
      >
        <Icon name="ChevRight" size={16} />
      </button>
    </div>
    <button
      type="button"
      class="zf-btn zf-btn-ghost period-mode-toggle"
      on:click={switchToRange}
    >
      {$t("Custom range")}
    </button>
  </div>
{:else}
  <div class="period-picker period-picker-range">
    <DateRangeFields
      bind:from
      bind:to
      fromId="{id}-from"
      toId="{id}-to"
      fromLabel={$t("From")}
      toLabel={$t("To")}
      minFrom={rangeMinFrom}
      maxFrom={maxDate}
      minTo={minDate}
      maxTo={rangeMaxTo}
    />
    <button
      type="button"
      class="zf-btn zf-btn-ghost period-mode-toggle"
      on:click={switchToMonth}
    >
      {$t("Month")}
    </button>
  </div>
{/if}

<style>
  .period-picker {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    flex-wrap: wrap;
  }

  /* Keep the month input compact — it only ever shows e.g. "July 2026".
     On mobile the width is reduced so it fits next to the employee selector. */
  .period-month-input {
    width: 150px;
    min-width: 100px;
  }

  /* Keep the toggle button aligned with the date inputs, not their labels. */
  .period-mode-toggle {
    height: 34px;
  }

  /* Mobile: the Reports toolbar puts the employee dropdown and this picker side
     by side, and the picker is used ONLY there. The two picker modes need
     different treatment because the month nav is narrow enough to sit beside
     the dropdown but the From/To pair is not. Verified across 320–412px with a
     headless browser before shipping.

     MONTH mode: `display: contents` dissolves the picker's box so its children —
     the ◀ month ▶ nav and the "Custom range" toggle — become flex items of the
     parent `.reports-filter-pair` directly. The nav then sits next to the
     employee dropdown while the toggle (flex: 1 1 100%) wraps to a full-width
     line beneath both, instead of being trapped in a picker column that forced
     a jumbled 2×2 layout.

     RANGE mode: the picker stays a normal box but is pushed onto its own
     full-width row below the employee dropdown (flex: 1 1 100%), so "Mitarbeitende"
     gets the first row to itself and the From/To fields sit on the row beneath
     it — kept two-up (grid 1fr 1fr) rather than the global mobile single-column
     collapse, since the full-width row has room for both. */
  @media (max-width: 768px) {
    .period-picker-month {
      display: contents;
    }

    .zf-nav-slider {
      flex: 0 1 auto;
      min-width: 0;
    }

    .period-month-input {
      width: 130px;
      min-width: 90px;
      flex: 1 1 auto;
    }

    .period-picker-range {
      flex: 1 1 100%;
    }

    .period-picker-range :global(.field-row) {
      grid-template-columns: 1fr 1fr;
      flex: 1 1 100%;
    }

    .period-mode-toggle {
      flex: 1 1 100%;
      white-space: normal;
      height: auto;
      min-height: 34px;
      line-height: 1.2;
    }
  }
</style>
