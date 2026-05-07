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
  let _closed = false;
  $: isNew = !template.id;
  let email = template.email || "";
  let first_name = template.first_name || "";
  let last_name = template.last_name || "";
  let role = template.role || "employee";
  let weekly_hours = template.weekly_hours ?? 39;
  const _thisYear = new Date().getFullYear();
  const _nextYear = _thisYear + 1;
  // Leave days — two explicit fields (current year + next year)
  let leave_days_current_year = 30;
  let leave_days_next_year = 30;
  let start_date = template.start_date || isoDate(new Date());
  let overtime_start_balance_hours =
    (template.overtime_start_balance_min || 0) / 60;
  let approver_id =
    template.approver_id == null ? "" : String(template.approver_id);
  let active = template.active ?? true;
  let error = "";
  let approvers = [];
  $: requiresApprover = role !== "admin";

  // Password fields (only for new users)
  let password = "";
  let confirmPassword = "";
  let showTempPassword = null;
  let smtpEnabled = false;

  function secureIndex(max) {
    const buf = new Uint32Array(1);
    crypto.getRandomValues(buf);
    return buf[0] % max;
  }

  function pick(chars) {
    return chars[secureIndex(chars.length)];
  }

  function shuffle(chars) {
    const out = [...chars];
    for (let i = out.length - 1; i > 0; i--) {
      const j = secureIndex(i + 1);
      [out[i], out[j]] = [out[j], out[i]];
    }
    return out.join("");
  }

  function generatePassword() {
    const lower = "abcdefghijkmnpqrstuvwxyz";
    const upper = "ABCDEFGHJKLMNPQRSTUVWXYZ";
    const digits = "23456789";
    const symbols = "!@#$%";
    const all = lower + upper + digits + symbols;
    let pw = pick(lower) + pick(upper) + pick(digits) + pick(symbols);
    while (pw.length < 16) pw += pick(all);
    pw = shuffle(pw);
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
    // Load leave days for existing users
    if (!isNew) {
      try {
        const rows = await api(`/users/${template.id}/leave-days`);
        const cur = rows.find((r) => r.year === _thisYear);
        const nxt = rows.find((r) => r.year === _nextYear);
        if (cur) leave_days_current_year = cur.days;
        if (nxt) leave_days_next_year = nxt.days;
      } catch {
        // leave defaults
      }
    }
    // Prefill defaults for new users
    if (isNew) {
      try {
        const settings = await api("/settings");
        if (settings.default_weekly_hours != null) {
          weekly_hours = Number(settings.default_weekly_hours);
        }
        if (settings.default_annual_leave_days != null) {
          leave_days_current_year = Number(settings.default_annual_leave_days);
          leave_days_next_year = Number(settings.default_annual_leave_days);
        }
        smtpEnabled = !!settings.smtp_enabled;
      } catch {}
    }
  });

  async function save() {
    error = "";
    if (requiresApprover && !approver_id) {
      error = $t("An approver is required for employees and team leads.");
      return;
    }
    if (isNew && password && password !== confirmPassword) {
      error = $t("Passwords do not match.");
      return;
    }
    if (!start_date) {
      error = $t("Invalid date.");
      return;
    }
    try {
      const body = {
        email,
        first_name,
        last_name,
        role,
        weekly_hours: Number(weekly_hours),
        leave_days_current_year: Number(leave_days_current_year),
        leave_days_next_year: Number(leave_days_next_year),
        start_date,
        overtime_start_balance_min: Math.round(
          Number(overtime_start_balance_hours) * 60,
        ),
      };
      if (requiresApprover) {
        body.approver_id = approver_id ? Number(approver_id) : null;
      } else if (!isNew && template.approver_id != null) {
        body.approver_id = null;
      }
      if (isNew && password) {
        body.password = password;
      }
      if (!isNew) {
        body.active = active;
      }
      if (isNew) {
        const r = await api("/users", { method: "POST", body });
        showTempPassword = r.temporary_password;
      } else {
        await api("/users/" + template.id, { method: "PUT", body });
        toast($t("User updated."), "ok");
        _closed = true;
        dlg.close();
        onClose(true);
      }
    } catch (e) {
      error = $t(e?.message || "Error");
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
    _closed = true;
    showTempPassword = null;
    dlg.close();
    onClose(true);
  }

  function cancel() {
    if (_closed) return;
    _closed = true;
    dlg.close();
    onClose(false);
  }
</script>

<dialog bind:this={dlg} on:close={cancel} style="max-width:520px">
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
      {#if smtpEnabled}
        <div style="font-size:12px;color:var(--text-tertiary);margin-top:8px">
          {$t("Registration email will be sent.")}
        </div>
      {:else}
        <div style="margin-top:10px;padding:10px 14px;background:var(--danger-bg, #fef2f2);border:2px solid var(--danger, #dc2626);border-radius:var(--radius-sm)">
          <strong style="color:var(--danger, #dc2626);font-size:14px">⚠ {$t("No email was sent! Email / SMTP is not configured.")}</strong>
          <div style="color:var(--danger, #dc2626);font-size:13px;margin-top:4px;font-weight:600">
            {$t("You must deliver this password to the user in person!")}
          </div>
        </div>
      {/if}
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
          <DatePicker
            id="user-start-date"
            bind:value={start_date}
            container={dlg}
          />
        </div>
      </div>
      {#if isNew}
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
        </div>
      {/if}
      <div>
        <label class="kz-label" for="user-overtime-balance"
          >{$t("Overtime start balance (hours)")}</label
        >
        <input
          id="user-overtime-balance"
          class="kz-input"
          type="number"
          step="0.5"
          bind:value={overtime_start_balance_hours}
        />
        <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
          {$t(
            "Initial overtime balance in hours when the user starts. Negative = deficit.",
          )}
        </div>
      </div>
      <div>
        <div
          style="font-size:13px;font-weight:600;margin-bottom:8px;margin-top:4px"
        >
          {$t("Vacation days per year")}
        </div>
        <div style="margin-bottom:10px;display:flex;gap:12px;flex-wrap:wrap">
          <div>
            <label class="kz-label" for="leave-cur">{$t("Annual leave days")} {_thisYear}</label>
            <input
              id="leave-cur"
              class="kz-input"
              type="number"
              min="0"
              max="366"
              bind:value={leave_days_current_year}
              style="max-width:120px"
            />
          </div>
          <div>
            <label class="kz-label" for="leave-nxt">{$t("Annual leave days")} {_nextYear}</label>
            <input
              id="leave-nxt"
              class="kz-input"
              type="number"
              min="0"
              max="366"
              bind:value={leave_days_next_year}
              style="max-width:120px"
            />
          </div>
        </div>
      </div>
      {#if !isNew}
        <div style="display:flex;align-items:center;justify-content:space-between;padding:10px 0;border-top:1px solid var(--border)">
          <div>
            <div style="font-size:13px;font-weight:500">{$t("Account active")}</div>
            <div style="font-size:11px;color:var(--text-tertiary);margin-top:2px">
              {$t("Inactive users cannot log in.")}
            </div>
          </div>
          <button
            class="kz-btn kz-btn-sm"
            class:kz-btn-danger={!active}
            type="button"
            on:click={() => (active = !active)}
          >
            {active ? $t("Active") : $t("Inactive")}
          </button>
        </div>
      {/if}
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
            class="kz-btn kz-btn-sm"
            type="button"
            on:click={generatePassword}
          >
            {$t("Generate password")}
          </button>
        </div>
      {/if}
      {#if requiresApprover}
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
            {$t("Required for employees and team leads.")}
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
