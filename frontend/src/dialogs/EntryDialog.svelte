<script>
  import { onMount, tick } from "svelte";
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
  let errorElement;
  let busy = false;

  onMount(() => {
    let refreshInterval;
    const refreshToday = () => {
      todayIso = appTodayIsoDate($settings?.timezone);
    };
    const delayToNextMinute = 60_000 - (Date.now() % 60_000) + 50;
    const refreshTimeout = setTimeout(() => {
      refreshToday();
      refreshInterval = setInterval(refreshToday, 60_000);
    }, delayToNextMinute);
    return () => {
      clearTimeout(refreshTimeout);
      clearInterval(refreshInterval);
    };
  });

  // Keep untouched default date aligned with app timezone changes.
  $: todayIso = appTodayIsoDate($settings?.timezone);
  $: {
    if (
      isNew &&
      !template.entry_date &&
      entry_date === lastTodayIso &&
      todayIso !== lastTodayIso
    ) {
      entry_date = todayIso;
    }
    // eslint-disable-next-line no-useless-assignment
    lastTodayIso = todayIso;
  }

  $: if (category_id == null && $categories.length > 0) {
    category_id = $categories[0].id;
  }

  $: if (isNew && start_time >= end_time) {
    const [h, m] = start_time.split(":").map(Number);
    if (h >= 23) {
      // Edge case 23:xx – ensure end after start.
      if (start_time >= "23:59") {
        start_time = "23:00";
        end_time = "23:59";
      } else {
        end_time = "23:59";
      }
    } else {
      end_time =
        String(h + 1).padStart(2, "0") + ":" + String(m).padStart(2, "0");
    }
  }

  async function showError(message) {
    error = message;
    await tick();
    errorElement?.scrollIntoView?.({ block: "nearest" });
  }

  async function save() {
    if (busy) return;
    error = "";
    if (!entry_date) {
      await showError($t("Invalid date."));
      return;
    }
    if (entry_date > todayIso) {
      await showError($t("Entries in the future are not allowed."));
      return;
    }
    if (start_time >= end_time) {
      await showError($t("End time must be after start time."));
      return;
    }
    if (entry_date === todayIso) {
      const currentTime = appCurrentTimeHM($settings?.timezone);
      if (end_time > currentTime) {
        await showError($t("End time cannot be in the future."));
        return;
      }
      if (start_time > currentTime) {
        await showError($t("Start time cannot be in the future."));
        return;
      }
    }
    if (category_id == null) {
      await showError($t("Category required."));
      return;
    }
    busy = true;
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
      await showError($t(e?.message || "Error"));
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (busy) return;
    busy = true;
    try {
      const confirmed = await confirmDialog(
        $t("Delete?"),
        $t("Delete this entry?"),
        {
          danger: true,
          confirm: $t("Delete"),
        },
      );
      if (!confirmed) return;
      await api("/time-entries/" + template.id, { method: "DELETE" });
      dialog.close(true);
      onClose({ changed: true, entry: null, deletedId: template.id });
    } catch (e) {
      await showError($t(e?.message || "Error"));
    } finally {
      busy = false;
    }
  }

  function onDialogKeydown(e) {
    if (e.key !== "Enter" || busy) return;
    const pickerOpen =
      dialog.querySelector(".tp-drum") ||
      dialog.querySelector(".flatpickr-calendar.open") ||
      document.querySelector(".flatpickr-calendar.open");
    const interactiveTarget = e.target?.closest?.(
      "button, textarea, select, a, [role='button']",
    );
    if (pickerOpen || interactiveTarget) return;
    e.preventDefault();
    save();
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t(isNew ? "Add Entry" : "Edit Entry")}
  closeDisabled={busy}
  onClose={() => {
    if (!busy) onClose({ changed: false, entry: null, deletedId: null });
  }}
  on:keydown={onDialogKeydown}
  let:dlg
>
  <div class="field-group" aria-busy={busy}>
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
        <TimePicker
          id="entry-start-time"
          label={$t("Start")}
          bind:value={start_time}
        />
      </div>
      <div>
        <label class="zf-label" for="entry-end-time">{$t("End")}</label>
        <TimePicker
          id="entry-end-time"
          label={$t("End")}
          bind:value={end_time}
        />
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
          {#each $categories as c (c.id)}<option value={c.id}
              >{$t(c.name)}</option
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
        bind:value={comment}></textarea>
    </div>
    <div
      class="error-text"
      role="alert"
      aria-live="assertive"
      bind:this={errorElement}
    >
      {error}
    </div>
  </div>
  <svelte:fragment slot="footer">
    {#if !isNew}
      <button
        class="zf-btn zf-btn-danger"
        type="button"
        disabled={busy}
        on:click={remove}
      >
        <Icon name="Trash" size={14} />{$t("Delete")}
      </button>
    {/if}
    <span class="flex-1"></span>
    <button
      class="zf-btn"
      type="button"
      disabled={busy}
      on:click={() => dialog.close()}>{$t("Cancel")}</button
    >
    <button
      class="zf-btn zf-btn-primary"
      type="button"
      disabled={busy}
      on:click={save}
    >
      {$t(isNew ? "Add Entry" : "Save")}
    </button>
  </svelte:fragment>
</Dialog>
