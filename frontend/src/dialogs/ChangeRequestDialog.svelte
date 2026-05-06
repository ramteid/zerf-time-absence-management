<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { categories, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { isoDate } from "../format.js";
  import { buildChangeRequestPayload } from "../changeRequests.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";
  import TimePicker from "../TimePicker.svelte";

  export let entry;
  export let onClose;
  let dlg;
  let _closed = false;
  let entry_date = entry.entry_date || isoDate(new Date());
  let start_time = entry.start_time?.slice(0, 5) || "08:00";
  let end_time = entry.end_time?.slice(0, 5) || "12:00";
  let category_id = entry.category_id ?? $categories[0]?.id ?? null;
  let comment = entry.comment || "";
  let reason = "";
  let error = "";

  $: if (start_time && end_time && start_time > end_time) {
    end_time = start_time;
  }

  onMount(() => {
    try {
      dlg.showModal();
    } catch {
      dlg?.setAttribute("open", "open");
    }
  });

  async function submit() {
    error = "";
    const result = buildChangeRequestPayload(entry, {
      entry_date,
      start_time,
      end_time,
      category_id,
      comment,
      reason,
    });
    if (result.error) {
      error = $t(result.error);
      return;
    }

    try {
      await api("/change-requests", {
        method: "POST",
        body: result.payload,
      });
      toast($t("Change request submitted."), "ok");
      _closed = true;
      dlg.close();
      onClose();
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }

  function cancel() {
    if (_closed) return;
    _closed = true;
    dlg.close();
    onClose();
  }
</script>

<dialog bind:this={dlg} on:close={cancel}>
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
      <label class="kz-label" for="change-request-date">{$t("Date")}</label>
      <DatePicker
        id="change-request-date"
        bind:value={entry_date}
        max={isoDate(new Date())}
        container={dlg}
      />
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="change-request-start">{$t("Start")}</label>
        <TimePicker id="change-request-start" bind:value={start_time} required />
      </div>
      <div>
        <label class="kz-label" for="change-request-end">{$t("End")}</label>
        <TimePicker id="change-request-end" bind:value={end_time} required />
      </div>
    </div>
    <div>
      <label class="kz-label" for="change-request-category">{$t("Category")}</label>
      <select
        id="change-request-category"
        class="kz-select"
        bind:value={category_id}
        disabled={$categories.length === 0}
      >
        {#if $categories.length === 0}
          <option value={null}>{$t("No categories available.")}</option>
        {:else}
          {#each $categories as category}
            <option value={category.id}>{$t(category.name)}</option>
          {/each}
        {/if}
      </select>
    </div>
    <div>
      <label class="kz-label" for="change-request-comment">{$t("Comment (optional)")}</label>
      <textarea
        id="change-request-comment"
        class="kz-textarea"
        rows="3"
        bind:value={comment}
      ></textarea>
    </div>
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
