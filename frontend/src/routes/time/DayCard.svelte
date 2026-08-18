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
    computeDayBreakInfo,
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

  function potentialWorkdaysPerWeek(workdaysPerWeek) {
    const configured = Number(workdaysPerWeek || 0);
    if (!Number.isFinite(configured) || configured <= 0) return 0;
    if (configured <= 5) return 5;
    if (configured === 6) return 6;
    return 7;
  }

  $: configuredWorkdays = Number(currentUser?.workdays_per_week || 5);
  $: potentialWorkdays = potentialWorkdaysPerWeek(configuredWorkdays);
  $: isPotentialDay = dayIndex < potentialWorkdays;
  $: dailyTargetHours = isPotentialDay
    ? (currentUser?.weekly_hours || 0) / potentialWorkdays
    : 0;
  $: breakRules = buildBreakRules($settings);
  // Break requirement/coverage for this day (day-total based, ArbZG §4 "insgesamt";
  // see computeDayBreakInfo). Empty breakdown when the feature is off.
  $: breakInfo = breakRules.length
    ? computeDayBreakInfo(day?.items, categories, breakRules)
    : {
        blocks: [],
        requiredMin: 0,
        takenMin: 0,
        deductionMin: 0,
        appliedRule: null,
      };
  $: dailyBreakMinutes = breakInfo.deductionMin;
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

  /** Places the existing in-entry hatch marker, but only for the common single-block
   *  day (one continuous stretch, no gaps) where the deduction is unambiguously "at"
   *  the moment the block crosses its rule's threshold. On multi-block days a
   *  deduction (if any, once real gaps are credited) is no longer a single instant in
   *  time — those days show the day-level shortfall indicator below instead.
   *  Returns a map from entry.id to { positionFraction, deductionFraction }. */
  function computeSingleBlockMarker(items, cats, info) {
    if (info.blocks.length !== 1 || !info.appliedRule) return {};
    const block = info.blocks[0];
    const breakTime = block.start + info.appliedRule.thresholdHours * 60;

    const eligible = (items || []).filter(
      (e) => e.status !== "rejected" && entryCountsAsWork(e, cats),
    );
    for (const entry of eligible) {
      const start = parseHHMM(entry.start_time);
      const end = parseHHMM(entry.end_time);
      // Use <= so that when breakTime lands exactly on an entry boundary the
      // marker still appears rather than being silently omitted.
      if (breakTime >= start && breakTime <= end) {
        const entryDuration = end - start;
        return {
          [entry.id]: {
            positionFraction: Math.min((breakTime - start) / entryDuration, 1),
            deductionFraction:
              info.appliedRule.deductionMinutes / entryDuration,
          },
        };
      }
    }
    return {};
  }

  $: breakMarkers = computeSingleBlockMarker(day?.items, categories, breakInfo);

  // A real gap between blocks (the only way a "break" is ever logged in this app) that
  // doesn't fully cover the day's legally required break minutes. Shown as a status
  // pill so the shortfall is visible while the day is still editable, instead of only
  // showing up later as a smaller-than-expected total.
  $: breakShortfall =
    breakInfo.blocks.length > 1 && breakInfo.deductionMin > 0
      ? { takenMin: breakInfo.takenMin, requiredMin: breakInfo.requiredMin }
      : null;
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
        class:target-met={!isAssistant &&
          dailyTotalMinutes / 60 >= dailyTargetHours}
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
      <div class="day-status-indicator" style:--status-color={statusColor}>
        <span class="day-status-dot" aria-hidden="true"></span>
        <span class="day-status-text">
          {day.absenceKind
            ? absenceKindLabel(day.absenceKind)
            : day.holidayName || $t("Public holiday")}
        </span>
      </div>
    {:else if breakShortfall}
      <div
        class="day-status-indicator"
        style:--status-color="var(--warning-text)"
      >
        <span class="day-status-dot" aria-hidden="true"></span>
        <span class="day-status-text">
          {$t("Break too short: {taken}/{required} min", {
            taken: breakShortfall.takenMin,
            required: breakShortfall.requiredMin,
          })}
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
        class="zf-btn zf-btn-ghost zf-btn-sm add-entry-btn"
        disabled={!canAdd}
        on:click={() => dispatch("add", { entry_date: day.ds })}
      >
        <Icon name="Plus" size={13} />{$t("Add")}
      </button>
    </div>
  {/if}
</div>

<style>
  /* Highlight the daily total once the target hours are reached. */
  .day-total.target-met {
    color: var(--accent);
  }

  .add-entry-btn {
    width: 100%;
    justify-content: center;
    border-style: dashed;
    border-color: var(--border);
  }

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
    font-size: 0.8125rem;
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
