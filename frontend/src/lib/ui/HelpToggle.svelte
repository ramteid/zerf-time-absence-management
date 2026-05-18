<script>
  import Icon from "../../Icons.svelte";

  export let title = "";
  export let open = false;
  export let onToggle = null;
  export let size = 14;
</script>

<button
  class="zf-btn-icon-sm zf-btn-ghost"
  {title}
  aria-expanded={open}
  on:click={() => onToggle?.()}
  style="color:var(--text-tertiary);font-size:{size}px;cursor:help"
>
  <Icon name="Info" {size} />
</button>
