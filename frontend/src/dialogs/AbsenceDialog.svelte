<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
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

  onMount(() => dlg.showModal());

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
      if (isNew) await api("/absences", { method: "POST", body });
      else await api("/absences/" + template.id, { method: "PUT", body });
      dlg.close();
      onClose(true);
    } catch (e) {
      error = $t(e.message) || e.message;
    }
  }

  function cancel() {
    dlg.close();
    onClose(false);
  }
</script>

<dialog bind:this={dlg}>
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
          max={end_date}
          container={dlg}
        />
      </div>
      <div>
        <label class="kz-label" for="absence-end-date">{$t("To")}</label>
        <DatePicker
          id="absence-end-date"
          bind:value={end_date}
          min={start_date}
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
