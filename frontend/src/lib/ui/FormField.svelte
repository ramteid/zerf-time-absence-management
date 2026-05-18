<script>
  export let label = "";
  export let forId = "";
</script>

<div>
  {#if label}
    <label class="zf-label" for={forId}>{label}</label>
  {/if}
  <slot />
</div>
