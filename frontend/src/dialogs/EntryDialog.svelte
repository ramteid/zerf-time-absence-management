<script>
  import { api } from "../api.js";
  import { categories, currentUser, settings } from "../stores.js";
  import { t } from "../i18n.js";
  import { appCurrentTimeHM, appTodayIsoDate } from "../format.js";
  import { confirmDialog } from "../confirm.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";
  import TimePicker from "../TimePicker.svelte";

  export let template;
  export let onClose;
  let dialog;
  $: isNew = !template.id;
  let todayIso = appTodayIsoDate($settings?.timezone);
  let lastTodayIso = todayIso;
  let entry_date = template.entry_date || todayIso;
  let start_time = template.start_time?.slice(0, 5) || "08:00";
  let end_time = template.end_time?.slice(0, 5) || "12:00";
  let category_id = template.category_id ?? $categories[0]?.id ?? null;
  let comment = template.comment || "";
  let error = "";

  // Keep untouched default date aligned with app timezone changes.
  $: todayIso = appTodayIsoDate($settings?.timezone);
  $: if (
    isNew &&
    !template.entry_date &&
    entry_date === lastTodayIso &&
    todayIso !== lastTodayIso
  ) {
    entry_date = todayIso;
  }
  // eslint-disable-next-line no-useless-assignment
  $: lastTodayIso = todayIso;

  $: if (isNew && start_time >= end_time) {
    const [h, m] = start_time.split(":").map(Number);
    if (h >= 23) {
      end_time = "23:59";
    } else {
      end_time = String(h + 1).padStart(2, "0") + ":" + String(m).padStart(2, "0");
    }
  }

  async function save() {
    error = "";
    if (!entry_date) {
      error = $t("Invalid date.");
      return;
    }
    if (start_time >= end_time) {
      error = $t("End time must be after start time.");
      return;
    }
    if (entry_date === todayIso) {
      const currentTime = appCurrentTimeHM($settings?.timezone);
      if (end_time > currentTime) {
        error = $t("End time cannot be in the future.");
        return;
      }
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
      dialog.close(true);
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
      dialog.close(true);
      onClose({ changed: true, entry: null, deletedId: template.id });
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }

  function onDialogKeydown(e) {
    const pickerOpen =
      dialog.querySelector(".tp-drum") ||
      document.querySelector(".flatpickr-calendar.open");
    if (e.key === "Enter" && !pickerOpen) {
      e.preventDefault();
      save();
    }
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t(isNew ? "Add Entry" : "Edit Entry")}
  onClose={() => onClose({ changed: false, entry: null, deletedId: null })}
  on:keydown={onDialogKeydown}
  let:dlg
>
  <div>
    <div>
      <label class="zf-label" for="entry-date">{$t("Date")}</label>
      <DatePicker
        id="entry-date"
        bind:value={entry_date}
        min={$currentUser?.start_date}
        max={todayIso}
        container={dlg}
      />
    </div>
    <div class="field-row">
      <div>
        <label class="zf-label" for="entry-start-time">{$t("Start")}</label>
        <TimePicker id="entry-start-time" bind:value={start_time} required />
      </div>
      <div>
        <label class="zf-label" for="entry-end-time">{$t("End")}</label>
        <TimePicker id="entry-end-time" bind:value={end_time} required />
      </div>
    </div>
    <div>
      <label class="zf-label" for="entry-category">{$t("Category")}</label>
      <select
        id="entry-category"
        class="zf-select"
        bind:value={category_id}
        disabled={$categories.length === 0}
      >
        {#if $categories.length === 0}
          <option value={null}>{$t("No categories available.")}</option>
        {:else}
          {#each $categories as c (c.id)}<option value={c.id}>{$t(c.name)}</option
            >{/each}
        {/if}
      </select>
    </div>
    <div>
      <label class="zf-label" for="entry-comment"
        >{$t("Comment (optional)")}</label
      >
      <textarea
        id="entry-comment"
        class="zf-textarea"
        rows="2"
        bind:value={comment}
      ></textarea>
    </div>
    <div class="error-text">{error}</div>
  </div>
  <svelte:fragment slot="footer">
    {#if !isNew}
      <button class="zf-btn zf-btn-danger" on:click={remove}>
        <Icon name="Trash" size={14} />{$t("Delete")}
      </button>
    {/if}
    <span style="flex:1"></span>
    <button class="zf-btn" on:click={() => dialog.close()}>{$t("Cancel")}</button>
    <button class="zf-btn zf-btn-primary" on:click={save}>
      {$t(isNew ? "Add Entry" : "Save")}
    </button>
  </svelte:fragment>
</Dialog>
