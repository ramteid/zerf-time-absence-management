<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { isoDate } from "../format.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";

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
  let approver_id =
    template.approver_id == null ? "" : String(template.approver_id);
  let error = "";
  let approvers = [];

  // Password fields (only for new users)
  let password = "";
  let confirmPassword = "";
  let showTempPassword = null;

  function generatePassword() {
    const chars =
      "abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789!@#$%";
    const buf = new Uint32Array(16);
    crypto.getRandomValues(buf);
    let pw = "";
    for (let i = 0; i < 16; i++) {
      pw += chars[buf[i] % chars.length];
    }
    password = pw;
    confirmPassword = pw;
  }

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
    // Prefill defaults for new users
    if (isNew) {
      try {
        const settings = await api("/settings");
        if (settings.default_weekly_hours != null) {
          weekly_hours = Number(settings.default_weekly_hours);
        }
        if (settings.default_annual_leave_days != null) {
          annual_leave_days = Number(settings.default_annual_leave_days);
        }
      } catch {}
    }
  });

  async function save() {
    error = "";
    if (role === "employee" && !approver_id) {
      error = $t("An approver is required for employees.");
      return;
    }
    if (isNew && password && password !== confirmPassword) {
      error = $t("Passwords do not match.");
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
      if (role === "employee") {
        body.approver_id = approver_id ? Number(approver_id) : null;
      } else if (!isNew && template.approver_id != null) {
        body.approver_id = null;
      }
      if (isNew && password) {
        body.password = password;
      }
      if (isNew) {
        const r = await api("/users", { method: "POST", body });
        // Always show the plaintext password once in a dedicated modal.
        showTempPassword = password || r.temporary_password;
      } else {
        await api("/users/" + template.id, { method: "PUT", body });
        toast($t("User updated."), "ok");
        dlg.close();
        onClose(true);
      }
    } catch (e) {
      error = e.message;
    }
  }

  let copied = false;
  async function copyPassword() {
    try {
      await navigator.clipboard.writeText(showTempPassword);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {}
  }

  function dismissTempPassword() {
    showTempPassword = null;
    dlg.close();
    onClose(true);
  }

  function cancel() {
    dlg.close();
    onClose(false);
  }
</script>

<dialog bind:this={dlg} style="max-width:520px">
  {#if showTempPassword}
    <header>
      <span style="flex:1">{$t("User created.")}</span>
    </header>
    <div class="dialog-body">
      <div
        style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);font-family:monospace;font-size:14px;word-break:break-all"
      >
        {$t("Temporary password:")} <strong>{showTempPassword}</strong>
      </div>
      <div style="font-size:12px;color:var(--text-tertiary);margin-top:8px">
        {$t("Registration email will be sent.")}
      </div>
    </div>
    <footer>
      <button class="kz-btn" on:click={copyPassword}>
        {copied ? $t("Copied!") : $t("Copy")}
      </button>
      <button class="kz-btn kz-btn-primary" on:click={dismissTempPassword}
        >{$t("OK")}</button
      >
    </footer>
  {:else}
    <header>
      <span style="flex:1">{$t(isNew ? "Add Member" : "Edit Member")}</span>
      <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
        <Icon name="X" size={16} />
      </button>
    </header>
    <div class="dialog-body">
      <div class="field-row">
        <div>
          <label class="kz-label" for="user-first-name"
            >{$t("First name")}</label
          >
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
          <label class="kz-label" for="user-start-date"
            >{$t("Start date")}</label
          >
          <DatePicker id="user-start-date" bind:value={start_date} />
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
      {#if isNew}
        <div class="field-row">
          <div>
            <label class="kz-label" for="user-password"
              >{$t("Password (min 12 chars)")}</label
            >
            <input
              id="user-password"
              class="kz-input"
              type="password"
              bind:value={password}
              minlength="12"
              autocomplete="new-password"
            />
          </div>
          <div>
            <label class="kz-label" for="user-confirm-password"
              >{$t("Confirm password")}</label
            >
            <input
              id="user-confirm-password"
              class="kz-input"
              type="password"
              bind:value={confirmPassword}
              minlength="12"
              autocomplete="new-password"
            />
          </div>
        </div>
        <div>
          <button
            class="kz-btn kz-btn-ghost kz-btn-sm"
            type="button"
            on:click={generatePassword}
          >
            {$t("Generate password")}
          </button>
        </div>
      {/if}
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
  {/if}
</dialog>
