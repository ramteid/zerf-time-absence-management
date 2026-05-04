<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { currentUser, categories } from "../stores.js";
  import { t } from "../i18n.js";
  import { isoDate } from "../format.js";
  import { confirmDialog } from "../confirm.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";
  import TimePicker from "../TimePicker.svelte";

  export let template;
  export let onClose;
  let dlg;
  $: isNew = !template.id;
  let entry_date = template.entry_date || isoDate(new Date());
  let start_time = template.start_time?.slice(0, 5) || "08:00";
  let end_time = template.end_time?.slice(0, 5) || "12:00";
  let category_id = template.category_id ?? $categories[0]?.id ?? null;
  let comment = template.comment || "";
  let error = "";

  onMount(() => dlg.showModal());

  async function save() {
    error = "";
    if (!entry_date) {
      error = $t("Invalid date.");
      return;
    }
    if (start_time >= end_time) {
      error = $t("Start cannot be after End.");
      return;
    }
    if (category_id == null) {
      error = $t("Category required.");
      return;
    }
    try {
      const body = {
        entry_date,
        start_time,
        end_time,
        category_id: Number(category_id),
        comment: comment || null,
      };
      const saved = isNew
        ? await api("/time-entries", { method: "POST", body })
        : await api("/time-entries/" + template.id, { method: "PUT", body });
      dlg.close();
      onClose({ changed: true, entry: saved, deletedId: null });
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }

  async function remove() {
    if (
      !(await confirmDialog($t("Delete?"), $t("Delete this entry?"), {
        danger: true,
        confirm: $t("Delete"),
      }))
    )
      return;
    try {
      await api("/time-entries/" + template.id, { method: "DELETE" });
      dlg.close();
      onClose({ changed: true, entry: null, deletedId: template.id });
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }

  function cancel() {
    dlg.close();
    onClose({ changed: false, entry: null, deletedId: null });
  }
</script>

<dialog bind:this={dlg}>
  <header>
    <span style="flex:1">{$t(isNew ? "Add Entry" : "Edit Entry")}</span>
    <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <div>
      <label class="kz-label" for="entry-date">{$t("Date")}</label>
      <DatePicker
        id="entry-date"
        bind:value={entry_date}
        min={$currentUser?.start_date}
        max={isoDate(new Date())}
        container={dlg}
      />
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="entry-start-time">{$t("Start")}</label>
        <TimePicker
          id="entry-start-time"
          bind:value={start_time}
          required
        />
      </div>
      <div>
        <label class="kz-label" for="entry-end-time">{$t("End")}</label>
        <TimePicker
          id="entry-end-time"
          bind:value={end_time}
          required
        />
      </div>
    </div>
    <div>
      <label class="kz-label" for="entry-category">{$t("Category")}</label>
      <select
        id="entry-category"
        class="kz-select"
        bind:value={category_id}
        disabled={$categories.length === 0}
      >
        {#if $categories.length === 0}
          <option value={null}>{$t("No categories available.")}</option>
        {:else}
          {#each $categories as c}<option value={c.id}>{$t(c.name)}</option
            >{/each}
        {/if}
      </select>
    </div>
    <div>
      <label class="kz-label" for="entry-comment"
        >{$t("Comment (optional)")}</label
      >
      <textarea
        id="entry-comment"
        class="kz-textarea"
        rows="2"
        bind:value={comment}
      ></textarea>
    </div>
    <div class="error-text">{error}</div>
  </div>
  <footer>
    {#if !isNew}
      <button class="kz-btn kz-btn-danger" on:click={remove}>
        <Icon name="Trash" size={14} />{$t("Delete")}
      </button>
    {/if}
    <span style="flex:1"></span>
    <button class="kz-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="kz-btn kz-btn-primary" on:click={save}>
      {$t(isNew ? "Add Entry" : "Save")}
    </button>
  </footer>
</dialog>
