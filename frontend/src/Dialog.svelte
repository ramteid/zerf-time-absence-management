<script>
  import { onMount } from "svelte";
  import Icon from "./Icons.svelte";

  export let title = "";
  export let onClose = null;
  // Wider layout for dialogs with tables or dense forms (e.g. user editor).
  export let wide = false;

  let dlg;
  let _silent = false;

  onMount(() => {
    try {
      dlg.showModal();
    } catch {
      dlg.setAttribute("open", "open");
    }
  });

  export function close(silent = false) {
    _silent = silent;
    if (typeof dlg.close === "function") {
      dlg.close();
      return;
    }
    dlg.removeAttribute("open");
    if (!_silent) onClose?.();
    _silent = false;
  }

  export function querySelector(selector) {
    return dlg?.querySelector(selector);
  }

  export { dlg as element };
</script>

<dialog
  bind:this={dlg}
  on:close={() => {
    if (!_silent) onClose?.();
    _silent = false;
  }}
  on:keydown
  class:dialog-wide={wide}
>
  <header>
    <slot name="title"><span class="flex-1">{title}</span></slot>
    <button class="zf-btn-icon-sm zf-btn-ghost" on:click={() => close()}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <slot {dlg} />
  </div>
  {#if $$slots.footer}
    <footer><slot name="footer" {dlg} /></footer>
  {/if}
</dialog>
