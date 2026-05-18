<script>
  import HelpToggle from "./HelpToggle.svelte";

  export let title = "";
  export let helpText = "";
  export let helpOpen = false;
  export let onHelpToggle = null;
  export let padded = true;
</script>

<section class="zf-card section-card" class:section-card--padded={padded}>
  {#if title || $$slots.actions}
    <div class="section-card-header">
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
        <div class="section-card-actions"><slot name="actions" /></div>
      {/if}
    </div>
  {/if}

  {#if helpOpen && helpText}
    <div class="section-card-help">{helpText}</div>
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
    gap: 12px;
    margin-bottom: 14px;
  }

  .section-card-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 400;
  }

  .section-card-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .section-card-help {
    font-size: 12px;
    color: var(--text-tertiary);
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }
</style>
