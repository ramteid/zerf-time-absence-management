<script>
  import { createEventDispatcher } from "svelte";
  import { t, formatHours, statusLabel } from "../../i18n.js";
  import CategoryDot from "../../lib/ui/CategoryDot.svelte";
  import StatusChip from "../../lib/ui/StatusChip.svelte";
  import { entryDurationHours, entryTimeRange } from "../../lib/domain/time.js";

  export let entry;
  export let category;
  export let timeFormat = "24h";
  export let editable = false;
  export let showDuration = true;
  /** When set, renders a horizontal break marker within the block.
   *  positionFraction: 0–1 from top where the break starts.
   *  deductionFraction: 0–1 relative to entry height representing break duration. */
  export let breakMarker = null;

  const dispatch = createEventDispatcher();
</script>

{#if editable}
  <div
    class="time-block"
    on:click={() => dispatch("edit", entry)}
    on:keydown={() => {}}
    role="button"
    tabindex="0"
  >
    <div class="time-block-cat">
      <CategoryDot color={category.color} />
      <span class="time-block-cat-name">{$t(category.name)}</span>
    </div>
    <div class="time-block-times tab-num">
      <span>{entryTimeRange(entry, timeFormat)}</span>
      {#if showDuration}
        <span>
          {formatHours(
            entryDurationHours(
              entry.start_time.slice(0, 5),
              entry.end_time.slice(0, 5),
            ),
          )}
        </span>
      {/if}
    </div>
    {#if breakMarker}
      {@const top = Math.max(0, Math.min(1, breakMarker.positionFraction))}
      {@const height = Math.max(0, Math.min(1 - top, breakMarker.deductionFraction))}
      <div
        class="time-block-break"
        style="top: {top * 100}%; height: {height * 100}%"
      >{$t("Break")}</div>
    {/if}
  </div>
{:else}
  <div
    class="time-block time-block--locked"
    class:time-block--rejected={entry.status === "rejected"}
  >
    <div class="time-block-cat">
      <CategoryDot color={category.color} />
      <span class="time-block-cat-name">{$t(category.name)}</span>
      <span class="time-entry-chip">
        <StatusChip status={entry.status}
          >{statusLabel(entry.status)}</StatusChip
        >
      </span>
    </div>
    <div class="time-block-times tab-num">
      <span>{entryTimeRange(entry, timeFormat)}</span>
      {#if showDuration}
        <span>
          {formatHours(
            entryDurationHours(
              entry.start_time.slice(0, 5),
              entry.end_time.slice(0, 5),
            ),
          )}
        </span>
      {/if}
    </div>
    {#if breakMarker}
      {@const top = Math.max(0, Math.min(1, breakMarker.positionFraction))}
      {@const height = Math.max(0, Math.min(1 - top, breakMarker.deductionFraction))}
      <div
        class="time-block-break"
        style="top: {top * 100}%; height: {height * 100}%"
      >{$t("Break")}</div>
    {/if}
  </div>
{/if}

<style>
  .time-block--rejected .time-block-cat-name,
  .time-block--rejected .time-block-times {
    text-decoration: line-through;
    color: var(--text-tertiary);
  }

  .time-block--locked {
    cursor: default;
  }

  .time-block--locked:hover {
    background: var(--bg-subtle);
  }

  .time-entry-chip :global(.zf-chip) {
    height: 18px;
    font-size: 10px;
  }
</style>
