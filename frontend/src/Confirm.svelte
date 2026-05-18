<script>
  import { t } from "./i18n.js";
  import { toast } from "./stores.js";
  import Dialog from "./Dialog.svelte";

  export let title = "OK";
  export let text = "";
  export let confirmLabel = "OK";
  export let danger = false;
  export let needReason = false;
  export let onResolve;
  let dialog;
  let reason = "";

  function ok() {
    if (needReason && !reason.trim()) {
      toast($t("Reason required"), "error");
      return;
    }
    onResolve(needReason ? reason : true);
    dialog.close(true);
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t(title)}
  onClose={() => onResolve(null)}
>
  {#if text}<p style="font-size:13px;color:var(--text-secondary)">
      {$t(text)}
    </p>{/if}
  {#if needReason}
    <div>
      <label class="zf-label" for="confirm-reason">{$t("Reason")}</label>
      <textarea
        id="confirm-reason"
        class="zf-textarea"
        rows="3"
        bind:value={reason}
        required
      ></textarea>
    </div>
  {/if}
  <svelte:fragment slot="footer">
    <button class="zf-btn" type="button" on:click={() => dialog.close()}>
      {$t("Cancel")}
    </button>
    <button
      class="zf-btn {danger ? 'zf-btn-danger' : 'zf-btn-primary'}"
      type="button"
      on:click={ok}
    >
      {$t(confirmLabel)}
    </button>
  </svelte:fragment>
</Dialog>
