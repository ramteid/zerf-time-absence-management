<script>
  import { onDestroy, onMount, tick } from "svelte";
  import Icon from "./Icons.svelte";
  import { t } from "./i18n.js";

  export let title = "";
  export let onClose = null;
  // Wider layout for dialogs with tables or dense forms (e.g. user editor).
  export let wide = false;

  let dlg;
  let _silent = false;
  let fallbackMode = false;
  let restoreOutsideState = null;
  let previouslyFocusedElement = null;

  const focusableSelector = [
    "button:not([disabled])",
    "a[href]",
    "input:not([disabled]):not([type='hidden'])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    "[tabindex]:not([tabindex='-1'])",
  ].join(",");

  function focusableElements() {
    return [...dlg.querySelectorAll(focusableSelector)].filter(
      (element) => !element.closest("[inert]") && element.offsetParent !== null,
    );
  }

  function makeOutsideContentInert() {
    const changedElements = [];
    let branch = dlg;

    while (branch?.parentElement) {
      const parent = branch.parentElement;
      for (const sibling of parent.children) {
        if (sibling === branch || sibling.classList.contains("dialog-backdrop"))
          continue;
        changedElements.push({
          element: sibling,
          inert: sibling.inert,
          ariaHidden: sibling.getAttribute("aria-hidden"),
        });
        sibling.inert = true;
        sibling.setAttribute("aria-hidden", "true");
      }
      if (parent === document.body) break;
      branch = parent;
    }

    restoreOutsideState = () => {
      for (const { element, inert, ariaHidden } of changedElements) {
        element.inert = inert;
        if (ariaHidden == null) element.removeAttribute("aria-hidden");
        else element.setAttribute("aria-hidden", ariaHidden);
      }
      restoreOutsideState = null;
    };
  }

  function restoreFallbackState() {
    const shouldRestoreFocus = fallbackMode;
    restoreOutsideState?.();
    fallbackMode = false;
    if (shouldRestoreFocus && previouslyFocusedElement?.isConnected) {
      previouslyFocusedElement.focus();
    }
    previouslyFocusedElement = null;
  }

  onMount(() => {
    if (typeof dlg.showModal === "function") {
      try {
        dlg.showModal();
        return;
      } catch {
        // Fall through to the accessible non-native modal implementation.
      }
    }

    previouslyFocusedElement = document.activeElement;
    fallbackMode = true;
    dlg.setAttribute("open", "open");
    tick().then(() => {
      if (!fallbackMode) return;
      focusableElements()[0]?.focus();
      makeOutsideContentInert();
    });
  });

  onDestroy(restoreFallbackState);

  export function close(silent = false) {
    _silent = silent;
    restoreFallbackState();
    if (typeof dlg.close === "function") {
      dlg.close();
      return;
    }
    dlg.removeAttribute("open");
    if (!_silent) onClose?.();
    _silent = false;
  }

  function onFallbackKeydown(event) {
    if (!fallbackMode || event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = focusableElements();
    if (focusable.length === 0) {
      event.preventDefault();
      dlg.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  export function querySelector(selector) {
    return dlg?.querySelector(selector);
  }

  export { dlg as element };
</script>

<svelte:window on:keydown={onFallbackKeydown} />

{#if fallbackMode}
  <div class="dialog-backdrop" aria-hidden="true"></div>
{/if}

<dialog
  bind:this={dlg}
  role={fallbackMode ? "dialog" : undefined}
  aria-modal="true"
  aria-label={title}
  tabindex="-1"
  on:close={() => {
    restoreFallbackState();
    if (!_silent) onClose?.();
    _silent = false;
  }}
  on:keydown
  class:dialog-wide={wide}
  class:dialog-fallback={fallbackMode}
>
  <header>
    <slot name="title"><span class="flex-1">{title}</span></slot>
    <button
      class="zf-btn-icon-sm zf-btn-ghost"
      type="button"
      aria-label={$t("Close")}
      title={$t("Close")}
      on:click={() => close()}
    >
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
