<script>
  import { createEventDispatcher } from "svelte";
  import Icon from "../../Icons.svelte";
  import { absenceKindLabel, formatHours, t } from "../../i18n.js";
  import { fmtDateShort } from "../../format.js";
  import EntryBlock from "./EntryBlock.svelte";
  import {
    absenceColor,
    buildBreakRules,
    canAddEntryForDay,
    categoryById,
    computeDayBreakDeduction,
    creditedEntryMinutes,
    entryCountsAsWork,
  } from "../../lib/domain/time.js";
  import { settings } from "../../stores.js";

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
  $: breakRules = buildBreakRules($settings);
  // Automatic break deduction for this day (0 when the feature is off).
  $: dailyBreakMinutes = breakRules.length
    ? computeDayBreakDeduction(day?.items, categories, breakRules)
    : 0;
  // Daily total: sum of credited entry minutes minus the automatic break deduction,
  // matching the value the backend uses in the flextime account.
  $: dailyTotalMinutes = Math.max(
    0,
    (day?.items || []).reduce(
      (totalMinutes, entry) =>
        totalMinutes + creditedEntryMinutes(entry, categories),
      0,
    ) - dailyBreakMinutes,
  );
  $: dailyTotalHours = dailyTotalMinutes / 60;
  $: canAdd = canAddEntryForDay(day, currentUser, today);

  function parseHHMM(s) {
    if (!s) return 0;
    const parts = s.split(":");
    return parseInt(parts[0], 10) * 60 + parseInt(parts[1] || "0", 10);
  }

  /** Computes break marker positions for all entries in a day.
   *  Adjacent entries (end == start of next) count as one continuous block.
   *  For each block the highest applicable rule is used (not cumulative), placing
   *  exactly one marker per qualifying block at the rule's threshold time point.
   *  Returns a map from entry.id to { positionFraction, deductionFraction }. */
  function computeBreakMarkers(items, cats, rules) {
    if (!items?.length || !rules?.length) return {};

    // Only non-rejected entries that count as work — mirrors computeDayBreakDeduction exactly.
    const eligible = items
      .filter((e) => e.status !== "rejected" && entryCountsAsWork(e, cats))
      .map((e) => ({
        id: e.id,
        _start: parseHHMM(e.start_time),
        _end: parseHHMM(e.end_time),
      }))
      .sort((a, b) => a._start - b._start);

    const markers = {};
    let i = 0;
    while (i < eligible.length) {
      let blockStart = eligible[i]._start;
      let blockEnd = eligible[i]._end;
      const blockEntries = [eligible[i]];
      i++;

      // Extend the block while entries are directly adjacent or overlapping.
      while (i < eligible.length && eligible[i]._start <= blockEnd) {
        blockEnd = Math.max(blockEnd, eligible[i]._end);
        blockEntries.push(eligible[i]);
        i++;
      }

      const blockDuration = blockEnd - blockStart;

      // Find the highest applicable rule for this block.
      let applicableRule = null;
      for (const rule of rules) {
        if (blockDuration >= rule.thresholdHours * 60) {
          applicableRule = rule; // last (highest threshold met) wins
        }
      }
      if (!applicableRule) continue;

      // Wall-clock time at which the break marker is placed.
      const breakTime = blockStart + applicableRule.thresholdHours * 60;

      for (const entry of blockEntries) {
        // Use <= so that when breakTime lands exactly on an entry boundary the
        // marker still appears rather than being silently omitted.
        if (breakTime >= entry._start && breakTime <= entry._end) {
          const entryDuration = entry._end - entry._start;
          markers[entry.id] = {
            positionFraction: Math.min((breakTime - entry._start) / entryDuration, 1),
            deductionFraction: applicableRule.deductionMinutes / entryDuration,
          };
          break;
        }
      }
    }
    return markers;
  }

  $: breakMarkers = breakRules.length
    ? computeBreakMarkers(day?.items, categories, breakRules)
    : {};
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

    {#each day.items as entry (entry.id)}
      {@const category = categoryById(entry.category_id, categories)}
      <EntryBlock
        {entry}
        {category}
        {timeFormat}
        editable={entry.status === "draft"}
        showDuration={!weekend}
        breakMarker={breakMarkers[entry.id] ?? null}
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
    border: 1px solid var(--border);
    background: var(--bg-subtle);
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
