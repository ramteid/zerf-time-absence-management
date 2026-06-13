<script>
  import { api } from "../api.js";
  import { currentUser, settings, absenceCategories } from "../stores.js";
  import { t } from "../i18n.js";
  import { absenceKindLabel } from "../i18n.js";
  import { appTodayIsoDate } from "../format.js";
  import { countWorkdays } from "../apiMappers.js";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";

  export let template;
  export let onClose;
  export let holidays = new Set();
  let dialog;
  $: isNew = !template.id;
  // category_id is the primary field; fall back to slug lookup for editing
  // existing absences that were created before dynamic categories.
  $: defaultCategoryId = (() => {
    if (template.category_id) return template.category_id;
    if (template.kind && $absenceCategories.length) {
      const match = $absenceCategories.find((c) => c.slug === template.kind);
      if (match) return match.id;
    }
    return $absenceCategories[0]?.id ?? null;
  })();
  let category_id = defaultCategoryId;
  $: if (!category_id && $absenceCategories.length) {
    category_id = $absenceCategories[0]?.id ?? null;
  }
  let todayIso = appTodayIsoDate($settings?.timezone);
  let lastTodayIso = todayIso;
  let start_date = template.start_date || todayIso;
  let end_date = template.end_date || todayIso;
  let comment = template.comment || "";
  let error = "";

  // Keep untouched defaults aligned with app timezone changes.
  $: todayIso = appTodayIsoDate($settings?.timezone);
  $: if (isNew && !template.start_date && start_date === lastTodayIso && todayIso !== lastTodayIso) {
    start_date = todayIso;
  }
  $: if (isNew && !template.end_date && end_date === lastTodayIso && todayIso !== lastTodayIso) {
    end_date = todayIso;
  }
  // eslint-disable-next-line no-useless-assignment
  $: lastTodayIso = todayIso;

  $: if (start_date && end_date && start_date > end_date) {
    end_date = start_date;
  }

  $: selectedDays =
    start_date && end_date
      ? countWorkdays(
          start_date,
          end_date,
          holidays,
          Number($currentUser?.workdays_per_week || 5),
        )
      : null;
  let pendingClose = null;

  function localizeAbsenceError(message) {
    const text = String(message || "").trim();
    if (!text) return $t("Error");
    if (text.includes("Overlap with existing absence")) {
      return $t("Conflict: Overlap with existing absence.");
    }
    if (text.includes("end_date must be >= start_date")) {
      return $t("From cannot be after To.");
    }
    if (text.includes("Absence range exceeds one year")) {
      return $t("Absence range exceeds one year.");
    }
    if (text === "Invalid date" || text === "Invalid date.") {
      return $t("Invalid date.");
    }
    if (text.includes("Failed to deserialize")) {
      return $t("Invalid date.");
    }
    if (text.includes("Not enough remaining vacation days")) {
      return $t("Not enough remaining vacation days.");
    }

    const translated = $t(text);
    return translated === text ? text : translated;
  }

  function closeDialog(changed, savedAbsence = null) {
    pendingClose = { changed, savedAbsence };
    dialog.close();
  }

  async function save() {
    error = "";
    if (!start_date || !end_date) {
      error = $t("Invalid date.");
      return;
    }
    if (start_date > end_date) {
      error = $t("From cannot be after To.");
      return;
    }
    try {
      const body = {
        category_id,
        start_date,
        end_date,
        comment: comment || null,
      };
      const saved = isNew
        ? await api("/absences", { method: "POST", body })
        : await api("/absences/" + template.id, { method: "PUT", body });
      closeDialog(true, saved);
    } catch (e) {
      error = localizeAbsenceError(e?.message);
    }
  }

  function cancel() {
    closeDialog(false, null);
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t(isNew ? "Request Absence" : "Edit Absence")}
  onClose={() => onClose(pendingClose?.changed ?? false, pendingClose?.savedAbsence ?? null)}
  let:dlg
>
  <div>
    <label class="zf-label" for="absence-kind">{$t("Type")}</label>
    <select id="absence-kind" class="zf-select" bind:value={category_id}>
      {#each $absenceCategories as cat (cat.id)}
        <option value={cat.id}>{absenceKindLabel(cat.slug) !== cat.slug ? absenceKindLabel(cat.slug) : cat.name}</option>
      {/each}
    </select>
  </div>
  <div class="field-row">
    <div>
      <label class="zf-label" for="absence-start-date">{$t("From")}</label>
      <DatePicker
        id="absence-start-date"
        bind:value={start_date}
        min={$currentUser?.start_date}
        container={dlg}
      />
    </div>
    <div>
      <label class="zf-label" for="absence-end-date">{$t("To")}</label>
      <DatePicker
        id="absence-end-date"
        bind:value={end_date}
        container={dlg}
      />
    </div>
  </div>
  {#if selectedDays !== null}
    <div class="selected-days-hint">
      {selectedDays}
      {$t("days")}
    </div>
  {/if}
  <div>
    <label class="zf-label" for="absence-comment"
      >{$t("Notes (optional)")}</label
    >
    <textarea
      id="absence-comment"
      class="zf-textarea"
      rows="3"
      bind:value={comment}
    ></textarea>
  </div>
  <div class="error-text">{error}</div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="zf-btn zf-btn-primary" on:click={save}>
      {$t(isNew ? "Submit Request" : "Save")}
    </button>
  </svelte:fragment>
</Dialog>

<style>
  .selected-days-hint {
    font-size: 0.85rem;
    color: var(--text-secondary, #64748b);
    margin-top: -0.25rem;
  }
</style>
