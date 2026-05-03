<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t, language } from "../i18n.js";
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
  // approver_id is mandatory for employees, ignored for leads/admins.
  let approver_id =
    template.approver_id == null ? "" : String(template.approver_id);
  let error = "";
  let approvers = [];

  onMount(async () => {
    dlg.showModal();
    try {
      const all = await api("/users");
      approvers = all.filter(
        (u) =>
          u.active &&
          (u.role === "team_lead" || u.role === "admin") &&
          u.id !== template.id,
      );
    } catch {
      approvers = [];
    }
  });

  async function save() {
    error = "";
    if (role === "employee" && !approver_id) {
      error = $t("An approver is required for employees.");
      return;
    }
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
      // Only send approver_id when it's relevant (employee role).
      // Sending null for leads/admins would explicitly clear it; we want
      // "leave untouched" so omit the key entirely there.
      if (role === "employee") {
        body.approver_id = approver_id ? Number(approver_id) : null;
      } else if (!isNew && template.approver_id != null) {
        // When promoting an employee → lead, clear the now-meaningless
        // approver to keep data tidy.
        body.approver_id = null;
      }
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
          lang={$language}
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
          min="0"
          max="168"
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
          min="0"
          max="366"
          bind:value={annual_leave_days}
        />
      </div>
    </div>
    {#if role === "employee"}
      <div>
        <label class="kz-label" for="user-approver"
          >{$t("Approver (Team lead / Admin)")}</label
        >
        <select
          id="user-approver"
          class="kz-select"
          bind:value={approver_id}
          required
        >
          <option value="">{$t("— None —")}</option>
          {#each approvers as a}
            <option value={String(a.id)}>
              {a.first_name}
              {a.last_name} ({a.email})
            </option>
          {/each}
        </select>
        <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
          {$t("Required for employees.")}
        </div>
      </div>
    {/if}
    <div class="error-text">{error}</div>
  </div>
  <footer>
    <button class="kz-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="kz-btn kz-btn-primary" on:click={save}>
      {$t(isNew ? "Add Member" : "Save")}
    </button>
  </footer>
</dialog>
