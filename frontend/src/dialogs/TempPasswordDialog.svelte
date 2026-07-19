<script>
  import Dialog from "../Dialog.svelte";
  import { t } from "../i18n.js";

  export let password;
  export let smtpEnabled = false;
  /** "create" (default) for new user, "reset" for admin password reset. */
  export let mode = "create";
  export let title;
  export let onDismiss;

  let dialog;
  let copied = false;

  async function copyPassword() {
    try {
      await navigator.clipboard.writeText(password);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {}
  }
</script>

<Dialog bind:this={dialog} {title} onClose={onDismiss} wide>
  <div class="pw-box">
    {$t("Temporary password:")} <strong>{password}</strong>
  </div>
  {#if smtpEnabled}
    <div class="text-hint mt-8">
      {mode === "reset"
        ? $t("Password reset email will be sent.")
        : $t("Registration email will be sent.")}
    </div>
  {:else}
    <div class="danger-box">
      <strong>{$t("No email was sent! Email / SMTP is not configured.")}</strong
      >
      <div>
        {$t("You must deliver this password to the user in person!")}
      </div>
    </div>
  {/if}
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={copyPassword}>
      {copied ? $t("Copied!") : $t("Copy")}
    </button>
    <button
      class="zf-btn zf-btn-primary"
      on:click={() => {
        dialog.close(true);
        onDismiss();
      }}>{$t("OK")}</button
    >
  </svelte:fragment>
</Dialog>

<style>
  /* Monospace box so the generated password is easy to read and copy. */
  .pw-box {
    padding: 12px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
    font-family: monospace;
    font-size: 0.9375rem;
    word-break: break-all;
  }

  /* Hard-to-miss warning that no e-mail was sent. Uses the danger tokens
     (the old inline version referenced an undefined --danger-bg variable). */
  .danger-box {
    margin-top: 10px;
    padding: 10px 14px;
    background: var(--danger-soft);
    border: 2px solid var(--danger);
    border-radius: var(--radius-sm);
  }

  .danger-box strong {
    color: var(--danger-text);
    font-size: 0.9375rem;
  }

  .danger-box div {
    color: var(--danger-text);
    font-size: 0.875rem;
    margin-top: 4px;
    font-weight: 400;
  }
</style>
