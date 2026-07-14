<script>
  import Icon from "../../Icons.svelte";

  export let title = "";
  export let open = false;
  export let onToggle = null;
  export let size = 14;
</script>

<button
  class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
  {title}
  aria-expanded={open}
  on:click={() => onToggle?.()}
  style:font-size={size + "px"}
>
  <Icon name="Info" {size} />
</button>
