<script>
  import { api } from "../api.js";
  import { currentUser, settings, absenceCategories } from "../stores.js";
  import { t } from "../i18n.js";
  import { appTodayIsoDate } from "../format.js";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";

  export let template;
  export let onClose;
  let dialog;
  $: isNew = !template.id;
  // category_id is always set for existing absences (guaranteed by migration 017).
  // The store now includes inactive categories for behavior resolution; filter to
  // active-only for the request dropdown so deactivated types don't appear there.
  $: activeAbsenceCategories = $absenceCategories.filter((c) => c.active);
  $: defaultCategoryId =
    template.category_id ?? activeAbsenceCategories[0]?.id ?? null;
  let category_id = defaultCategoryId;
  // Assign once the store finishes loading when opening a new request.
  $: if (!category_id && activeAbsenceCategories.length) {
    category_id = activeAbsenceCategories[0]?.id ?? null;
  }
  let todayIso = appTodayIsoDate($settings?.timezone);
  let lastTodayIso = todayIso;
  let start_date = template.start_date || todayIso;
  let end_date = template.end_date || todayIso;
  let comment = template.comment || "";
  let error = "";

  // Keep untouched defaults aligned with app timezone changes.
  $: todayIso = appTodayIsoDate($settings?.timezone);
  $: if (
    isNew &&
    !template.start_date &&
    start_date === lastTodayIso &&
    todayIso !== lastTodayIso
  ) {
    start_date = todayIso;
  }
  $: if (
    isNew &&
    !template.end_date &&
    end_date === lastTodayIso &&
    todayIso !== lastTodayIso
  ) {
    end_date = todayIso;
  }
  // eslint-disable-next-line no-useless-assignment
  $: lastTodayIso = todayIso;

  $: if (start_date && end_date && start_date > end_date) {
    end_date = start_date;
  }

  // How many of this contract's own working days the request covers. The
  // server answers it, because the very same question decides what the booking
  // is charged, and an answer worked out twice is an answer that can disagree
  // with itself. It also means the dialog no longer has to fetch holidays for
  // years the surrounding page had not loaded.
  let selectedDays = null;
  let selectedDaysRequestId = 0;

  async function refreshSelectedDays(fromIso, toIso) {
    if (!fromIso || !toIso || toIso < fromIso) {
      selectedDays = null;
      return;
    }
    const requestId = ++selectedDaysRequestId;
    try {
      const result = await api(
        `/absences/workday-preview?start_date=${fromIso}&end_date=${toIso}`,
      );
      // A slower earlier request must not overwrite a newer answer.
      if (requestId === selectedDaysRequestId) {
        selectedDays = result?.days ?? null;
      }
    } catch {
      if (requestId === selectedDaysRequestId) selectedDays = null;
    }
  }

  $: refreshSelectedDays(start_date, end_date);
  let pendingClose = null;

  // AU (medical certificate) preview: only categories flagged
  // medical_certificate_relevant show this — see services::medical_certificate
  // on the backend, which is the single source of truth for the calculation.
  // This is read-only information for the requester; it cannot be edited here.
  $: selectedCategory = $absenceCategories.find((c) => c.id === category_id);
  $: medicalCertificateRelevant =
    selectedCategory?.medical_certificate_relevant ?? false;

  let medicalCertificatePreview = null;
  let previewRequestId = 0;

  async function loadMedicalCertificatePreview(
    relevant,
    catId,
    fromDate,
    toDate,
  ) {
    if (!relevant || !catId || !fromDate || !toDate || fromDate > toDate) {
      medicalCertificatePreview = null;
      return;
    }
    const requestId = ++previewRequestId;
    const params = new URLSearchParams({
      category_id: String(catId),
      start_date: fromDate,
      end_date: toDate,
    });
    if (!isNew && template.id) {
      params.set("exclude_absence_id", String(template.id));
    }
    try {
      const result = await api(
        `/absences/medical-certificate-preview?${params}`,
      );
      if (requestId === previewRequestId) medicalCertificatePreview = result;
    } catch {
      if (requestId === previewRequestId) medicalCertificatePreview = null;
    }
  }

  $: loadMedicalCertificatePreview(
    medicalCertificateRelevant,
    category_id,
    start_date,
    end_date,
  );

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
    if (text.includes("Not enough remaining leave-account days")) {
      return $t("Not enough remaining leave-account days.");
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
  onClose={() =>
    onClose(pendingClose?.changed ?? false, pendingClose?.savedAbsence ?? null)}
  let:dlg
>
  <div>
    <label class="zf-label" for="absence-kind">{$t("Type")}</label>
    <select id="absence-kind" class="zf-select" bind:value={category_id}>
      {#if !isNew && template.category_id && !activeAbsenceCategories.find((c) => c.id === template.category_id)}
        <option value={template.category_id}
          >{template.category_name || $t("Unknown type")}</option
        >
      {/if}
      {#each activeAbsenceCategories as cat (cat.id)}
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
      <DatePicker id="absence-end-date" bind:value={end_date} container={dlg} />
    </div>
  </div>
  {#if selectedDays !== null}
    <!-- selectedDays counts only the days this contract works, with public
         holidays already taken off by the server, so the
         label must read "workday(s)" rather than the calendar-day "days". -->
    <div class="selected-days-hint">
      {selectedDays}
      {selectedDays === 1 ? $t("workday") : $t("workdays")}
    </div>
  {/if}
  {#if medicalCertificateRelevant}
    <div class="medical-certificate-info">
      <!-- Only one of these two is ever visible at a time — the verdict is
           conveyed by which one shows, so there's no separate indicator
           (e.g. a checkbox) restating it a third time. -->
      {#if medicalCertificatePreview?.required}
        <div class="medical-certificate-warning" role="alert">
          <strong>{$t("medical_certificate_required_warning_title")}</strong>
          <div>
            {$t("medical_certificate_required_warning_body", {
              days: medicalCertificatePreview.chain_days,
            })}
          </div>
        </div>
      {:else if medicalCertificatePreview}
        <div class="field-hint">
          {$t("medical_certificate_chain_days_hint", {
            days: medicalCertificatePreview.chain_days,
            threshold: medicalCertificatePreview.threshold_days,
          })}
        </div>
      {/if}
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
      bind:value={comment}></textarea>
  </div>
  <div class="error-text">{error}</div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button
      class="zf-btn zf-btn-primary"
      on:click={save}
      disabled={!category_id}
    >
      {$t(isNew ? "Submit Request" : "Save")}
    </button>
  </svelte:fragment>
</Dialog>

<style>
  .selected-days-hint {
    font-size: 0.9rem;
    color: var(--text-secondary, #64748b);
    margin-top: -0rem;
  }

  .medical-certificate-info {
    margin-top: 8px;
  }

  .medical-certificate-warning {
    margin-bottom: 8px;
    padding: 10px 12px;
    border: 1px solid var(--danger, #c64a3f);
    border-radius: 6px;
    background: var(--danger-soft, #fbe8e5);
    color: var(--danger-text, #8a3128);
    font-size: 0.9rem;
    line-height: 1.4;
  }
</style>
