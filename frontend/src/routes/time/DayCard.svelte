<script>
  import { createEventDispatcher } from "svelte";
  import Icon from "../../Icons.svelte";
  import { absenceKindLabel, formatHours, t } from "../../i18n.js";
  import { fmtDateShort } from "../../format.js";
  import EntryBlock from "./EntryBlock.svelte";
  import {
    absenceColor,
    canAddEntryForDay,
    categoryById,
    creditedEntryMinutes,
  } from "../../lib/domain/time.js";

  export let day;
  export let dayIndex = 0;
  export let currentUser = null;
  export let categories = [];
  export let weekStatus = "draft";
  export let drafts = [];
  export let timeFormat = "24h";
  export let today = "";
  export let isAssistant = false;
  export let weekend = false;

  const dispatch = createEventDispatcher();

  $: dailyTargetHours =
    dayIndex < (currentUser?.workdays_per_week || 5)
      ? (currentUser?.weekly_hours || 0) / (currentUser?.workdays_per_week || 5)
      : 0;
  $: dailyTotalMinutes = (day?.items || []).reduce(
    (totalMinutes, entry) =>
      totalMinutes + creditedEntryMinutes(entry, categories),
    0,
  );
  $: dailyTotalHours = dailyTotalMinutes / 60;
  $: canAdd = canAddEntryForDay(day, currentUser, today);
</script>

<div
  class="zf-card day-card"
  class:day-card--locked={weekStatus === "submitted" ||
    weekStatus === "approved"}
  class:day-card--absent={day.absent}
  class:day-card--before-start={currentUser?.start_date &&
    day.ds < currentUser.start_date}
>
  <div class="day-header">
    <div>
      <div class="day-name">{$t(day.dayName)}</div>
      <div class="day-date tab-num">{fmtDateShort(day.d)}</div>
    </div>
    {#if !weekend}
      <div
        class="day-total tab-num"
        style="color: {!isAssistant &&
        dailyTotalMinutes / 60 >= dailyTargetHours
          ? 'var(--accent)'
          : 'var(--text-primary)'}"
      >
        {formatHours(dailyTotalHours)}
      </div>
    {/if}
  </div>

  <div class="day-entries">
    {#if day.absenceKind || day.holiday}
      {@const statusColor = day.absenceKind
        ? absenceColor(day.absenceKind)
        : "var(--warning-text)"}
      <div class="day-status-indicator" style={`--status-color:${statusColor}`}>
        <span class="day-status-dot" aria-hidden="true"></span>
        <span class="day-status-text">
          {day.absenceKind
            ? absenceKindLabel(day.absenceKind)
            : day.holidayName || $t("Public holiday")}
        </span>
      </div>
    {/if}

    {#each day.items as entry}
      {@const category = categoryById(entry.category_id, categories)}
      <EntryBlock
        {entry}
        {category}
        {timeFormat}
        editable={entry.status === "draft"}
        showDuration={!weekend}
        on:edit={() => dispatch("edit", entry)}
      />
    {/each}
  </div>

  {#if !weekend && (weekStatus === "draft" || drafts.length > 0)}
    <div class="day-add-btn">
      <button
        class="zf-btn zf-btn-ghost zf-btn-sm"
        style="width:100%;justify-content:center;border-style:dashed;border-color:var(--border)"
        disabled={!canAdd}
        on:click={() => dispatch("add", { entry_date: day.ds })}
      >
        <Icon name="Plus" size={13} />{$t("Add")}
      </button>
    </div>
  {/if}
</div>

<style>
  .day-card--before-start {
    opacity: 0.4;
  }

  .day-status-indicator {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    align-self: center;
    gap: 8px;
    margin: auto;
    max-width: 100%;
    padding: 6px 10px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--status-color) 28%, transparent);
    background: color-mix(in srgb, var(--status-color) 12%, transparent);
    color: var(--status-color);
    font-size: 12px;
    font-weight: 600;
    text-align: center;
  }

  .day-status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--status-color);
  }

  .day-status-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
