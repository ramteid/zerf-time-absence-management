<script>
  import Icon from "../../Icons.svelte";

  export let icon = "";
  export let text = "";
</script>

<div class="empty-state">
  {#if icon}<Icon name={icon} size={24} sw={1.2} />{/if}
  {#if text}<div>{text}</div>{/if}
  <slot />
</div>

<style>
  .empty-state {
    padding: 32px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 13px;
  }

  .empty-state :global(svg) {
    margin-bottom: 8px;
  }
</style>
