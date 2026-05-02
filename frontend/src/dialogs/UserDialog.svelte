<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { isoDate } from "../format.js";
  import Icon from "../Icons.svelte";

  export let template;
  export let onClose;
  let dlg;
  $: isNew = !template.id;
  let email = template.email || "";
  let first_name = template.first_name || "";
  let last_name = template.last_name || "";
  let role = template.role || "employee";
  let weekly_hours = template.weekly_hours || 39;
  let annual_leave_days = template.annual_leave_days || 30;
  let start_date = template.start_date || isoDate(new Date());
  let error = "";

  onMount(() => dlg.showModal());

  async function save() {
    error = "";
    try {
      const body = {
        email,
        first_name,
        last_name,
        role,
        weekly_hours: Number(weekly_hours),
        annual_leave_days: Number(annual_leave_days),
        start_date,
      };
      if (isNew) {
        const r = await api("/users", { method: "POST", body });
        toast(
          $t("User created. Temporary password: {password}", {
            password: r.temporary_password,
          }),
          "info",
        );
      } else {
        await api("/users/" + template.id, { method: "PUT", body });
        toast($t("User updated."), "ok");
      }
      dlg.close();
      onClose(true);
    } catch (e) {
      error = e.message;
    }
  }

  function cancel() {
    dlg.close();
    onClose(false);
  }
</script>

<dialog bind:this={dlg} style="max-width:520px">
  <header>
    <span style="flex:1">{$t(isNew ? "Add Member" : "Edit Member")}</span>
    <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <div class="field-row">
      <div>
        <label class="kz-label" for="user-first-name">{$t("First name")}</label>
        <input
          id="user-first-name"
          class="kz-input"
          bind:value={first_name}
          required
        />
      </div>
      <div>
        <label class="kz-label" for="user-last-name">{$t("Last name")}</label>
        <input
          id="user-last-name"
          class="kz-input"
          bind:value={last_name}
          required
        />
      </div>
    </div>
    <div>
      <label class="kz-label" for="user-email">{$t("Email")}</label>
      <input
        id="user-email"
        class="kz-input"
        type="email"
        bind:value={email}
        required
      />
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="user-role">{$t("Role")}</label>
        <select id="user-role" class="kz-select" bind:value={role}>
          <option value="employee">{$t("Employee")}</option>
          <option value="team_lead">{$t("Team lead")}</option>
          <option value="admin">{$t("Admin")}</option>
        </select>
      </div>
      <div>
        <label class="kz-label" for="user-start-date">{$t("Start date")}</label>
        <input
          id="user-start-date"
          class="kz-input"
          type="date"
          bind:value={start_date}
        />
      </div>
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="user-weekly-hours"
          >{$t("Weekly hours")}</label
        >
        <input
          id="user-weekly-hours"
          class="kz-input"
          type="number"
          step="0.5"
          bind:value={weekly_hours}
        />
      </div>
      <div>
        <label class="kz-label" for="user-annual-leave-days"
          >{$t("Annual leave days")}</label
        >
        <input
          id="user-annual-leave-days"
          class="kz-input"
          type="number"
          bind:value={annual_leave_days}
        />
      </div>
    </div>
    <div class="error-text">{error}</div>
  </div>
  <footer>
    <button class="kz-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="kz-btn kz-btn-primary" on:click={save}>
      {$t(isNew ? "Add Member" : "Save")}
    </button>
  </footer>
</dialog>
