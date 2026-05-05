<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { currentUser } from "../stores.js";
  import { t } from "../i18n.js";
  import { isoDate } from "../format.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";

  export let template;
  export let onClose;
  let dlg;
  $: isNew = !template.id;
  let kind = template.kind || "vacation";
  let start_date = template.start_date || isoDate(new Date());
  let end_date = template.end_date || isoDate(new Date());
  let comment = template.comment || "";
  let error = "";

  $: if (start_date && end_date && start_date > end_date) {
    end_date = start_date;
  }
  let closeHandled = false;
  let closeResult = { changed: false, savedAbsence: null };

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

  onMount(() => {
    const timer = setTimeout(openDialog, 0);
    return () => clearTimeout(timer);
  });

  function openDialog() {
    if (!dlg || dlg.open) return;
    try {
      dlg.showModal();
    } catch {
      dlg.setAttribute("open", "open");
    }
  }

  function completeClose(changed, savedAbsence = null) {
    if (closeHandled) return;
    closeHandled = true;
    onClose(changed, savedAbsence);
  }

  function closeDialog(changed, savedAbsence = null) {
    closeResult = { changed, savedAbsence };
    if (dlg?.open) {
      dlg.close();
      return;
    }
    completeClose(changed, savedAbsence);
  }

  function handleNativeClose() {
    completeClose(closeResult.changed, closeResult.savedAbsence);
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
        kind,
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

<dialog bind:this={dlg} on:close={handleNativeClose}>
  <header>
    <span style="flex:1">{$t(isNew ? "Request Absence" : "Edit Absence")}</span>
    <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <div>
      <label class="kz-label" for="absence-kind">{$t("Type")}</label>
      <select id="absence-kind" class="kz-select" bind:value={kind}>
        <option value="vacation">{$t("Vacation")}</option>
        <option value="sick">{$t("Sick")}</option>
        <option value="training">{$t("Training")}</option>
        <option value="special_leave">{$t("Special leave")}</option>
        <option value="unpaid">{$t("Unpaid")}</option>
        <option value="general_absence">{$t("General absence")}</option>
      </select>
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="absence-start-date">{$t("From")}</label>
        <DatePicker
          id="absence-start-date"
          bind:value={start_date}
          min={$currentUser?.start_date}
          container={dlg}
        />
      </div>
      <div>
        <label class="kz-label" for="absence-end-date">{$t("To")}</label>
        <DatePicker
          id="absence-end-date"
          bind:value={end_date}
          container={dlg}
        />
      </div>
    </div>
    <div>
      <label class="kz-label" for="absence-comment"
        >{$t("Notes (optional)")}</label
      >
      <textarea
        id="absence-comment"
        class="kz-textarea"
        rows="3"
        bind:value={comment}
      ></textarea>
    </div>
    <div class="error-text">{error}</div>
  </div>
  <footer>
    <button class="kz-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="kz-btn kz-btn-primary" on:click={save}>
      {$t(isNew ? "Submit Request" : "Save")}
    </button>
  </footer>
</dialog>
