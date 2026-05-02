<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";

  export let entry;
  export let onClose;
  let dlg;
  let reason = "";
  let error = "";

  onMount(() => dlg.showModal());

  async function submit() {
    error = "";
    if (!reason.trim()) {
      error = $t("Reason required");
      return;
    }
    try {
      await api("/change-requests", {
        method: "POST",
        body: { time_entry_id: entry.id, reason },
      });
      toast($t("Change request submitted."), "ok");
      dlg.close();
      onClose();
    } catch (e) {
      error = e.message;
    }
  }

  function cancel() {
    dlg.close();
    onClose();
  }
</script>

<dialog bind:this={dlg}>
  <header>
    <span style="flex:1">{$t("Request change")}</span>
    <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <p style="font-size:13px;color:var(--text-secondary)">
      {$t("Entry")}: {entry.entry_date}
      {entry.start_time?.slice(0, 5)}–{entry.end_time?.slice(0, 5)}
    </p>
    <div>
      <label class="kz-label" for="change-request-reason">{$t("Reason")}</label>
      <textarea
        id="change-request-reason"
        class="kz-textarea"
        rows="4"
        bind:value={reason}
        required
      ></textarea>
    </div>
    <div class="error-text">{error}</div>
  </div>
  <footer>
    <button class="kz-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="kz-btn kz-btn-primary" on:click={submit}
      >{$t("Submit")}</button
    >
  </footer>
</dialog>
