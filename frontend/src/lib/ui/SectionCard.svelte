<script>
  import HelpToggle from "./HelpToggle.svelte";

  export let title = "";
  export let helpText = "";
  export let helpOpen = false;
  export let onHelpToggle = null;
  export let padded = true;
  /// Give the actions their own full-width row underneath the title instead of
  /// placing them next to it. For wide control rows (e.g. the flextime range
  /// picker) that would otherwise be squeezed into a narrow column beside the
  /// title.
  export let actionsOwnRow = false;
</script>

<section class="zf-card section-card" class:section-card--padded={padded}>
  {#if title || $$slots.actions}
    <div class="section-card-header" class:section-card-header--inset={!padded}>
      <div class="section-card-title">
        {#if title}<span>{title}</span>{/if}
        {#if helpText}
          <HelpToggle
            title={helpText}
            open={helpOpen}
            onToggle={onHelpToggle}
          />
        {/if}
      </div>
      {#if $$slots.actions}
        <div
          class="section-card-actions"
          class:section-card-actions--own-row={actionsOwnRow}
        >
          <slot name="actions" />
        </div>
      {/if}
    </div>
  {/if}

  {#if helpOpen && helpText}
    <div class="section-card-help" class:section-card-help--inset={!padded}>
      {helpText}
    </div>
  {/if}

  <slot />
</section>

<style>
  .section-card {
    margin-bottom: 16px;
  }

  .section-card--padded {
    padding: 20px;
  }

  .section-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    /* The row gap only applies once the actions no longer fit beside the
       title and wrap onto their own line below it. */
    gap: 10px 12px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }

  /* When the card has no padding (e.g. edge-to-edge tables), add horizontal
     inset to the header and help text so they match the standard card look. */
  .section-card-header--inset {
    padding: 20px 20px 0;
  }

  .section-card-help--inset {
    margin: 0 20px 12px;
  }

  .section-card-title {
    display: flex;
    align-items: center;
    gap: 8px;
    /* Absorb the free space so the actions sit at the far right, and give way
       first when the header gets tight. */
    flex: 1 1 auto;
    min-width: 0;
    font-size: 0.9375rem;
    font-weight: 400;
  }

  .section-card-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 1 auto;
    min-width: 0;
  }

  /* Own row: a full-width basis forces the actions onto a line of their own,
     where they get the whole card width to lay themselves out in. */
  .section-card-actions--own-row {
    flex: 1 1 100%;
  }

  .section-card-help {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }
</style>
