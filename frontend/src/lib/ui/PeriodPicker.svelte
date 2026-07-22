<script>
  // Shared period selector for the Reports page: a month stepper (◀/▶, capped
  // at minMonth/maxMonth — matches the Calendar/Time month-nav pattern) with a
  // toggle into a free "from/to" range for cases the month grid can't express
  // (e.g. a quarter, or a range spanning into the future for planned absences).
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";
  import DateRangeFields from "./DateRangeFields.svelte";
  import { monthEnd, monthStart } from "../domain/dates.js";

  export let mode = "month"; // "month" | "range"
  export let month = ""; // "YYYY-MM"
  export let from = "";
  export let to = "";
  export let minMonth = null;
  export let maxMonth = null;
  export let minDate = null;
  export let maxDate = null;
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
</script>

{#if mode === "month"}
  <div class="period-picker">
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
  <div class="period-picker">
    <DateRangeFields
      bind:from
      bind:to
      fromId="{id}-from"
      toId="{id}-to"
      fromLabel={$t("From")}
      toLabel={$t("To")}
      minFrom={minDate}
      maxFrom={maxDate}
      maxTo={maxDate}
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

  /* Keep the month input compact — it only ever shows e.g. "July 2026". */
  .period-month-input {
    width: 150px;
  }

  /* Keep the toggle button aligned with the date inputs, not their labels. */
  .period-mode-toggle {
    height: 34px;
  }
</style>
