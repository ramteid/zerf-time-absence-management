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

<Dialog bind:this={dialog} {title} onClose={onDismiss} style="max-width:520px">
  <div
    style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);font-family:monospace;font-size:14px;word-break:break-all"
  >
    {$t("Temporary password:")} <strong>{password}</strong>
  </div>
  {#if smtpEnabled}
    <div style="font-size:12px;color:var(--text-tertiary);margin-top:8px">
      {mode === "reset" ? $t("Password reset email will be sent.") : $t("Registration email will be sent.")}
    </div>
  {:else}
    <div
      style="margin-top:10px;padding:10px 14px;background:var(--danger-bg, #fef2f2);border:2px solid var(--danger, #dc2626);border-radius:var(--radius-sm)"
    >
      <strong style="color:var(--danger, #dc2626);font-size:14px"
        >{$t("No email was sent! Email / SMTP is not configured.")}</strong
      >
      <div
        style="color:var(--danger, #dc2626);font-size:13px;margin-top:4px;font-weight:400"
      >
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
