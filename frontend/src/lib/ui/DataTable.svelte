<script>
  export let empty = false;
  export let emptyText = "";
  export let fit = false;
</script>

{#if empty}
  <div class="data-table-empty">{emptyText}</div>
{:else}
  <div class="zf-table-wrap">
    <table class="zf-table {fit ? 'zf-table--fit' : ''}">
      <slot />
    </table>
  </div>
{/if}

<style>
  .data-table-empty {
    padding: 16px;
    color: var(--text-tertiary);
    font-size: 13px;
  }
</style>
