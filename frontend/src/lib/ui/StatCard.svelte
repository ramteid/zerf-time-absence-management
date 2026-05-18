<script>
  export let label = "";
  export let value = "";
  export let sub = "";
  export let color = "";
  export let loading = false;
</script>

<div class="zf-card stat-card">
  {#if label || $$slots.label}
    <div class="stat-card-label">
      <slot name="label">{label}</slot>
    </div>
  {/if}
  <div class="stat-card-value tab-num" style={color ? `color:${color}` : ""}>
    {#if loading}
      ...
    {:else}
      <slot>{value}</slot>
    {/if}
  </div>
  {#if sub || $$slots.sub}
    <div class="stat-card-sub">
      <slot name="sub">{sub}</slot>
    </div>
  {/if}
</div>
