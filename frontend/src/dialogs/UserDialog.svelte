<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { settings, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { appTodayDate, appTodayIsoDate } from "../format.js";
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
  let workdays_per_week = template.workdays_per_week ?? 5;
  $: _thisYear = appTodayDate($settings?.timezone).getFullYear();
  $: _nextYear = _thisYear + 1;
  // Leave days — two explicit fields (current year + next year)
  let leave_days_current_year = 30;
  let leave_days_next_year = 30;
  let todayIso = appTodayIsoDate($settings?.timezone);
  let lastTodayIso = todayIso;
  let start_date = template.start_date || todayIso;
  let overtime_start_balance_hours =
    (template.overtime_start_balance_min || 0) / 60;
  let approver_ids = Array.isArray(template.approver_ids) ? template.approver_ids.map(Number) : [];
  let active = template.active ?? true;
  let error = "";
  let approvers = [];
  $: normalizedRole = String(role || "").trim().toLowerCase();
  $: requiresApprover = normalizedRole !== "admin";
  $: isAssistantRole = normalizedRole === "assistant";
  $: if (isAssistantRole) {
    weekly_hours = 0;
    overtime_start_balance_hours = 0;
  }

  // Password fields (only for new users)
  let password = "";
  let confirmPassword = "";
  let showTempPassword = null;
  let smtpEnabled = false;

  // Keep untouched start-date default aligned with timezone changes.
  $: todayIso = appTodayIsoDate($settings?.timezone);
  $: if (isNew && !template.start_date && start_date === lastTodayIso && todayIso !== lastTodayIso) {
    start_date = todayIso;
  }
  $: lastTodayIso = todayIso;

  function secureIndex(max) {
    const buf = new Uint32Array(1);
    crypto.getRandomValues(buf);
    return buf[0] % max;
  }

  function pick(chars) {
    return chars[secureIndex(chars.length)];
  }

  function shuffle(chars) {
    const shuffledCharacters = [...chars];
    for (let currentIndex = shuffledCharacters.length - 1; currentIndex > 0; currentIndex--) {
      const randomIndex = secureIndex(currentIndex + 1);
      [shuffledCharacters[currentIndex], shuffledCharacters[randomIndex]] = [shuffledCharacters[randomIndex], shuffledCharacters[currentIndex]];
    }
    return shuffledCharacters.join("");
  }

  function generatePassword() {
    const lower = "abcdefghijkmnpqrstuvwxyz";
    const upper = "ABCDEFGHJKLMNPQRSTUVWXYZ";
    const digits = "23456789";
    const symbols = "!@#$%";
    const all = lower + upper + digits + symbols;
    let generatedPassword = pick(lower) + pick(upper) + pick(digits) + pick(symbols);
    while (generatedPassword.length < 16) generatedPassword += pick(all);
    generatedPassword = shuffle(generatedPassword);
    password = generatedPassword;
    confirmPassword = generatedPassword;
  }

  onMount(async () => {
    dlg.showModal();
    try {
      const allUsers = await api("/users");
      approvers = allUsers.filter(
        (candidateUser) =>
          candidateUser.active &&
          (candidateUser.role === "team_lead" || candidateUser.role === "admin") &&
          candidateUser.id !== template.id,
      );
    } catch {
      approvers = [];
    }
    // Load leave days for existing users
    if (!isNew) {
      try {
        const rows = await api(`/users/${template.id}/leave-days`);
        const currentYearLeave = rows.find((leaveRow) => leaveRow.year === _thisYear);
        const nextYearLeave = rows.find((leaveRow) => leaveRow.year === _nextYear);
        if (currentYearLeave) leave_days_current_year = currentYearLeave.days;
        if (nextYearLeave) leave_days_next_year = nextYearLeave.days;
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
    if (requiresApprover && approver_ids.length === 0) {
      error = $t("At least one approver is required for employees and team leads.");
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
    if (
      Number(workdays_per_week) < 1 ||
      Number(workdays_per_week) > 7
    ) {
      error = $t("Workdays per week must be between 1 and 7.");
      return;
    }
    try {
      const normalizedWeeklyHours = isAssistantRole ? 0 : Number(weekly_hours);
      const normalizedOvertimeStartBalanceMin = isAssistantRole
        ? 0
        : Math.round(Number(overtime_start_balance_hours) * 60);
      const body = {
        email,
        first_name,
        last_name,
        role: normalizedRole,
        weekly_hours: normalizedWeeklyHours,
        workdays_per_week: Number(workdays_per_week),
        leave_days_current_year: Number(leave_days_current_year),
        leave_days_next_year: Number(leave_days_next_year),
        start_date,
        overtime_start_balance_min: normalizedOvertimeStartBalanceMin,
      };
      if (requiresApprover) {
        body.approver_ids = approver_ids;
      } else {
        body.approver_ids = [];
      }
      if (isNew && password) {
        body.password = password;
      }
      if (!isNew) {
        body.active = active;
      }
      if (isNew) {
        const createdUser = await api("/users", { method: "POST", body });
        showTempPassword = createdUser.temporary_password;
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
          <strong style="color:var(--danger, #dc2626);font-size:14px">{$t("No email was sent! Email / SMTP is not configured.")}</strong>
          <div style="color:var(--danger, #dc2626);font-size:13px;margin-top:4px;font-weight:400">
            {$t("You must deliver this password to the user in person!")}
          </div>
        </div>
      {/if}
    </div>
    <footer>
      <button class="zf-btn" on:click={copyPassword}>
        {copied ? $t("Copied!") : $t("Copy")}
      </button>
      <button class="zf-btn zf-btn-primary" on:click={dismissTempPassword}
        >{$t("OK")}</button
      >
    </footer>
  {:else}
    <header>
      <span style="flex:1">{$t(isNew ? "Add Member" : "Edit Member")}</span>
      <button class="zf-btn-icon-sm zf-btn-ghost" on:click={cancel}>
        <Icon name="X" size={16} />
      </button>
    </header>
    <div class="dialog-body">
      <div class="field-row">
        <div>
          <label class="zf-label" for="user-first-name"
            >{$t("First name")}</label
          >
          <input
            id="user-first-name"
            class="zf-input"
            bind:value={first_name}
            required
          />
        </div>
        <div>
          <label class="zf-label" for="user-last-name">{$t("Last name")}</label>
          <input
            id="user-last-name"
            class="zf-input"
            bind:value={last_name}
            required
          />
        </div>
      </div>
      <div>
        <label class="zf-label" for="user-email">{$t("Email")}</label>
        <input
          id="user-email"
          class="zf-input"
          type="email"
          bind:value={email}
          required
        />
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="user-role">{$t("Role")}</label>
          <select id="user-role" class="zf-select" bind:value={role}>
            <option value="employee">{$t("Employee")}</option>
            <option value="assistant">{$t("Assistant")}</option>
            <option value="team_lead">{$t("Team lead")}</option>
            <option value="admin">{$t("Admin")}</option>
          </select>
        </div>
        <div>
          <label class="zf-label" for="user-start-date"
            >{$t("Start date")}</label
          >
          <DatePicker
            id="user-start-date"
            bind:value={start_date}
            container={dlg}
          />
        </div>
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="user-weekly-hours"
            >{$t("Weekly hours")}</label
          >
          <input
            id="user-weekly-hours"
            class="zf-input"
            type="number"
            step="0.5"
            min="0"
            max="168"
            bind:value={weekly_hours}
            disabled={isAssistantRole}
          />
        </div>
        <div>
          <label class="zf-label" for="user-workdays-per-week"
            >{$t("Workdays per week")}</label
          >
          <input
            id="user-workdays-per-week"
            class="zf-input"
            type="number"
            step="1"
            min="1"
            max="7"
            bind:value={workdays_per_week}
          />
        </div>
      </div>
      <div>
        <label class="zf-label" for="user-overtime-balance"
          >{$t("Overtime start balance (hours)")}</label
        >
        <input
          id="user-overtime-balance"
          class="zf-input"
          type="number"
          step="0.5"
          bind:value={overtime_start_balance_hours}
          disabled={isAssistantRole}
        />
        <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
          {$t(
            "Initial overtime balance in hours when the user starts. Negative = deficit.",
          )}
        </div>
      </div>
      <div>
        <div
          style="font-size:13px;font-weight:400;margin-bottom:8px;margin-top:4px"
        >
          {$t("Vacation days per year")}
        </div>
        <div style="margin-bottom:10px;display:flex;gap:12px;flex-wrap:wrap">
          <div>
            <label class="zf-label" for="leave-cur">{$t("Annual leave days")} {_thisYear}</label>
            <input
              id="leave-cur"
              class="zf-input"
              type="number"
              min="0"
              max="366"
              bind:value={leave_days_current_year}
              style="max-width:120px"
            />
          </div>
          <div>
            <label class="zf-label" for="leave-nxt">{$t("Annual leave days")} {_nextYear}</label>
            <input
              id="leave-nxt"
              class="zf-input"
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
            class="zf-btn zf-btn-sm"
            class:zf-btn-danger={!active}
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
            <label class="zf-label" for="user-password"
              >{$t("Password (min 12 chars)")}</label
            >
            <input
              id="user-password"
              class="zf-input"
              type="password"
              bind:value={password}
              minlength="12"
              autocomplete="new-password"
            />
          </div>
          <div>
            <label class="zf-label" for="user-confirm-password"
              >{$t("Confirm password")}</label
            >
            <input
              id="user-confirm-password"
              class="zf-input"
              type="password"
              bind:value={confirmPassword}
              minlength="12"
              autocomplete="new-password"
            />
          </div>
        </div>
        <div>
          <button
            class="zf-btn zf-btn-sm"
            type="button"
            on:click={generatePassword}
          >
            {$t("Generate password")}
          </button>
        </div>
      {/if}
      {#if requiresApprover}
        <div>
          <div class="zf-label">{$t("Approvers (Team leads / Admins)")}</div>
          {#if approvers.length === 0}
            <div style="font-size:13px;color:var(--text-tertiary);padding:6px 0">
              {$t("No eligible approvers found.")}
            </div>
          {:else}
            <div style="display:flex;flex-direction:column;gap:6px;max-height:180px;overflow-y:auto;border:1px solid var(--border);border-radius:var(--radius-sm);padding:8px">
              {#each approvers as a}
                <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:13px">
                  <input
                    type="checkbox"
                    value={a.id}
                    bind:group={approver_ids}
                  />
                  {a.first_name}
                  {a.last_name} ({a.email})
                </label>
              {/each}
            </div>
          {/if}
          <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
            {$t("At least one approver is required for employees and team leads.")}
          </div>
        </div>
      {/if}
      <div class="error-text">{error}</div>
    </div>
    <footer>
      <button class="zf-btn" on:click={cancel}>{$t("Cancel")}</button>
      <button class="zf-btn zf-btn-primary" on:click={save}>
        {$t(isNew ? "Add Member" : "Save")}
      </button>
    </footer>
  {/if}
</dialog>
