<script>
  import { onMount } from "svelte";
  import { t } from "./i18n.js";
  import { toast } from "./stores.js";
  import Icon from "./Icons.svelte";

  export let title = "OK";
  export let text = "";
  export let confirmLabel = "OK";
  export let danger = false;
  export let needReason = false;
  export let onResolve;
  let dlg;
  let reason = "";
  onMount(() => dlg.showModal());

  function ok() {
    if (needReason && !reason.trim()) {
      toast($t("Reason required"), "error");
      return;
    }
    dlg.close();
    onResolve(needReason ? reason : true);
  }

  function cancel() {
    dlg.close();
    onResolve(null);
  }
</script>

<dialog bind:this={dlg}>
  <header>
    <span style="flex:1">{$t(title)}</span>
    <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    {#if text}<p style="font-size:13px;color:var(--text-secondary)">
        {$t(text)}
      </p>{/if}
    {#if needReason}
      <div>
        <label class="kz-label" for="confirm-reason">{$t("Reason")}</label>
        <textarea
          id="confirm-reason"
          class="kz-textarea"
          rows="3"
          bind:value={reason}
          required
        ></textarea>
      </div>
    {/if}
  </div>
  <footer>
    <button class="kz-btn" type="button" on:click={cancel}
      >{$t("Cancel")}</button
    >
    <button
      class="kz-btn {danger ? 'kz-btn-danger' : 'kz-btn-primary'}"
      type="button"
      on:click={ok}
    >
      {$t(confirmLabel)}
    </button>
  </footer>
</dialog>
