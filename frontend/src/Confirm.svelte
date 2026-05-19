<script>
  import { t } from "./i18n.js";
  import { toast } from "./stores.js";
  import Dialog from "./Dialog.svelte";

  export let title = "OK";
  export let text = "";
  export let confirmLabel = "OK";
  export let danger = false;
  export let needReason = false;
  // When set, the user must type this exact phrase before the OK button is enabled.
  export let requirePhrase = "";
  export let onResolve;
  let dialog;
  let reason = "";
  let phraseInput = "";

  function ok() {
    if (needReason && !reason.trim()) {
      toast($t("Reason required"), "error");
      return;
    }
    if (requirePhrase && phraseInput.trim() !== requirePhrase) {
      return;
    }
    dialog.close(true);
    onResolve(needReason ? reason : true);
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
  {#if requirePhrase}
    <div>
      <label class="zf-label" for="confirm-phrase">
        {$t('Type "{phrase}" to confirm', { phrase: requirePhrase })}
      </label>
      <input
        id="confirm-phrase"
        class="zf-input"
        type="text"
        bind:value={phraseInput}
        autocomplete="off"
      />
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
      disabled={!!requirePhrase && phraseInput.trim() !== requirePhrase}
    >
      {$t(confirmLabel)}
    </button>
  </svelte:fragment>
</Dialog>
