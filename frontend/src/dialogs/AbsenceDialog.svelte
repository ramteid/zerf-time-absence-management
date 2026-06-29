<script>
  import { api } from "../api.js";
  import { currentUser, settings, absenceCategories } from "../stores.js";
  import { t } from "../i18n.js";
  import { appTodayIsoDate } from "../format.js";
  import { countWorkdays, holidayDateSet } from "../apiMappers.js";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";

  export let template;
  export let onClose;
  export let holidays = new Set();
  let dialog;
  $: isNew = !template.id;
  // category_id is always set for existing absences (guaranteed by migration 017).
  $: defaultCategoryId = template.category_id ?? $absenceCategories[0]?.id ?? null;
  let category_id = defaultCategoryId;
  // Assign once the store finishes loading when opening a new request.
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

  // Holiday dates the dialog fetches itself for any year in the selected range
  // that the parent screen did not preload (e.g. the user picks a date in a
  // year other than the one currently shown on the Absences page, whose
  // holidays the parent loaded). Merged with the `holidays` prop below so
  // countWorkdays excludes those holidays too.
  let fetchedHolidays = new Set();
  // Years we already hold holiday data for, so we never re-fetch. Years present
  // in the prop count as loaded, avoiding a redundant request in the common
  // case where the request stays within the screen's currently-viewed year.
  let fetchedYears = new Set();

  async function ensureHolidaysForRange(startIso, endIso) {
    if (!startIso || !endIso) return;
    const startYear = Number(String(startIso).slice(0, 4));
    const endYear = Number(String(endIso).slice(0, 4));
    if (!Number.isFinite(startYear) || !Number.isFinite(endYear)) return;
    // Years already provided by the parent prop are considered loaded.
    const propYears = new Set(
      [...holidays].map((iso) => Number(String(iso).slice(0, 4))),
    );
    let added = false;
    for (let year = startYear; year <= endYear; year += 1) {
      if (propYears.has(year) || fetchedYears.has(year)) continue;
      // Mark before awaiting so a rapid second trigger does not double-fetch.
      fetchedYears.add(year);
      try {
        const list = await api(`/holidays?year=${year}`);
        for (const date of holidayDateSet(list)) fetchedHolidays.add(date);
        added = true;
      } catch {
        // Non-fatal: this year's holidays simply won't be excluded. Un-mark the
        // year so a later interaction can retry.
        fetchedYears.delete(year);
      }
    }
    // Reassign so Svelte reacts to the mutated Set and recomputes selectedDays.
    if (added) fetchedHolidays = new Set(fetchedHolidays);
  }

  $: ensureHolidaysForRange(start_date, end_date);

  // Effective holiday set = parent-provided holidays plus any the dialog
  // fetched itself for years the parent had not loaded.
  $: effectiveHolidays =
    fetchedHolidays.size > 0
      ? new Set([...holidays, ...fetchedHolidays])
      : holidays;

  $: selectedDays =
    start_date && end_date
      ? countWorkdays(
          start_date,
          end_date,
          effectiveHolidays,
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
    if (!category_id) {
      error = $t("Type is required.");
      return;
    }
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
      {#if !isNew && template.category_id && !$absenceCategories.find((c) => c.id === template.category_id)}
        <option value={template.category_id}>{template.category_name || $t("Unknown type")}</option>
      {/if}
      {#each $absenceCategories as cat (cat.id)}
        <option value={cat.id}>{$t(cat.name)}</option>
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
    <!-- selectedDays is a contract-workday count (countWorkdays already
         excludes weekends per workdays_per_week and public holidays), so the
         label must read "workday(s)" rather than the calendar-day "days". -->
    <div class="selected-days-hint">
      {selectedDays}
      {selectedDays === 1 ? $t("workday") : $t("workdays")}
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
    <button class="zf-btn zf-btn-primary" on:click={save} disabled={!category_id}>
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
