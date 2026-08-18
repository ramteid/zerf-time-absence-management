<script>
  // Category filter for the calendar: one button in the top bar that opens a
  // menu listing every category the displayed month actually contains, so the
  // viewer never picks from categories that would show nothing.
  //
  // Hidden categories stay in the list, dimmed and unchecked, because the menu
  // is also the only place to bring them back — and because seeing what is
  // currently filtered out is the point of opening it.
  import { onDestroy } from "svelte";
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";

  export let items = []; // [{ colorKey, color, label }], already ordered
  export let hidden = new Set(); // colorKeys currently filtered out
  export let onToggle = () => {};
  export let onShowAll = () => {};
  export let onHideAll = () => {};

  let isOpen = false;
  let rootElement;

  $: visibleCount = items.filter((item) => !hidden.has(item.colorKey)).length;
  $: isFiltered = visibleCount < items.length;
  $: allHidden = items.length > 0 && visibleCount === 0;

  function toggleMenu() {
    isOpen = !isOpen;
  }

  function closeMenu() {
    isOpen = false;
  }

  // The menu stays open while categories are toggled: filtering is usually a
  // few clicks in a row, and closing after each one would mean reopening it
  // to see the result of the last click.
  function onWindowClick(event) {
    if (isOpen && rootElement && !rootElement.contains(event.target)) {
      closeMenu();
    }
  }

  function onWindowKeydown(event) {
    if (isOpen && event.key === "Escape") {
      closeMenu();
      rootElement?.querySelector(".cal-filter-trigger")?.focus();
    }
  }

  // A month with no entries at all has nothing to filter; the button would
  // open an empty menu, so it is hidden along with any stale open state.
  $: if (items.length === 0 && isOpen) closeMenu();

  onDestroy(closeMenu);
</script>

<svelte:window on:click={onWindowClick} on:keydown={onWindowKeydown} />

{#if items.length > 0}
  <div class="cal-filter" bind:this={rootElement}>
    <button
      type="button"
      class="zf-btn cal-filter-trigger"
      class:filtered={isFiltered}
      aria-haspopup="true"
      aria-expanded={isOpen}
      on:click={toggleMenu}
    >
      <Icon name="Filter" size={14} />
      <span>{$t("Categories")}</span>
      <!-- The count only appears once something is actually filtered out, so
           the button reads as a plain label while everything is shown. -->
      {#if isFiltered}
        <span class="cal-filter-count tab-num"
          >{visibleCount}/{items.length}</span
        >
      {/if}
      <Icon name="ChevDown" size={14} />
    </button>

    {#if isOpen}
      <div class="cal-filter-menu">
        <div class="cal-filter-actions">
          <button
            type="button"
            class="zf-btn zf-btn-sm"
            on:click={onShowAll}
            disabled={!isFiltered}>{$t("Show all")}</button
          >
          <button
            type="button"
            class="zf-btn zf-btn-sm"
            on:click={onHideAll}
            disabled={allHidden}>{$t("Hide all")}</button
          >
        </div>
        <div class="cal-filter-list">
          {#each items as item (item.colorKey)}
            {@const isVisible = !hidden.has(item.colorKey)}
            <button
              type="button"
              class="cal-filter-option"
              class:off={!isVisible}
              role="menuitemcheckbox"
              aria-checked={isVisible}
              on:click={() => onToggle(item.colorKey)}
            >
              <span class="cal-filter-check" aria-hidden="true">
                {#if isVisible}
                  <Icon name="Check" size={13} />
                {/if}
              </span>
              <span class="cal-filter-swatch" style:background={item.color}
              ></span>
              <span class="cal-filter-label">{item.label}</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .cal-filter {
    position: relative;
  }

  /* An active filter is called out on the button itself: the menu is closed
     most of the time, and a calendar that silently hides entries is worse than
     no filter at all. */
  .cal-filter-trigger.filtered {
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .cal-filter-count {
    font-size: 0.8125rem;
    font-weight: 500;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .cal-filter-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 100;
    min-width: 220px;
    max-width: min(320px, calc(100vw - 32px));
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--bg-surface);
    box-shadow: var(--shadow-lg);
  }

  .cal-filter-actions {
    display: flex;
    gap: 6px;
    padding-bottom: 8px;
    margin-bottom: 6px;
    border-bottom: 1px solid var(--border);
  }

  .cal-filter-actions .zf-btn {
    flex: 1;
    justify-content: center;
  }

  .cal-filter-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    /* Long category lists scroll inside the menu instead of growing past the
       bottom of the viewport. */
    max-height: min(320px, 50vh);
    overflow-y: auto;
  }

  .cal-filter-option {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.875rem;
    text-align: left;
    cursor: pointer;
    transition: background-color 120ms ease-in-out;
  }

  .cal-filter-option:hover {
    background: var(--bg-muted);
  }

  /* Hidden categories stay legible but clearly recede, so the menu doubles as
     the answer to "what is missing from the calendar right now?". */
  .cal-filter-option.off .cal-filter-label {
    color: var(--text-tertiary);
  }

  .cal-filter-option.off .cal-filter-swatch {
    opacity: 0.3;
  }

  .cal-filter-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    flex-shrink: 0;
    color: var(--accent);
  }

  .cal-filter-swatch {
    display: inline-block;
    width: 12px;
    height: 12px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .cal-filter-label {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  /* On narrow screens the top bar wraps and the button can end up on the left,
     so the menu is anchored to the left edge to stay on screen. */
  @media (max-width: 640px) {
    .cal-filter-menu {
      right: auto;
      left: 0;
    }
  }
</style>
