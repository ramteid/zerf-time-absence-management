<script>
  export let title = "";
  export let subtitle = "";
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{title}</h1>
    {#if subtitle}
      <div class="top-bar-subtitle">{subtitle}</div>
    {/if}
  </div>
  {#if $$slots.actions}
    <div class="top-bar-actions">
      <slot name="actions" />
    </div>
  {/if}
</div>
