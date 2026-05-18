<script>
  import { onMount } from "svelte";
  import Icon from "./Icons.svelte";

  export let title = "";
  export let onClose = null;
  export let style = "";

  let dlg;
  let _silent = false;

  onMount(() => dlg.showModal());

  export function close(silent = false) {
    _silent = silent;
    dlg.close();
  }

  export function querySelector(selector) {
    return dlg?.querySelector(selector);
  }

  export { dlg as element };
</script>

<dialog bind:this={dlg} on:close={() => { if (!_silent) onClose?.(); _silent = false; }} on:keydown {style}>
  <header>
    <slot name="title"><span style="flex:1">{title}</span></slot>
    <button class="zf-btn-icon-sm zf-btn-ghost" on:click={() => close()}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <slot />
  </div>
  {#if $$slots.footer}
    <footer><slot name="footer" /></footer>
  {/if}
</dialog>
