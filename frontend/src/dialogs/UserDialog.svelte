<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { settings, toast } from "../stores.js";
  import { t, fmtDecimal, parseDecimal } from "../i18n.js";
  import { confirmDialog } from "../confirm.js";
  import { appTodayDate, appTodayIsoDate } from "../format.js";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";
  import Icon from "../Icons.svelte";
  import TempPasswordDialog from "./TempPasswordDialog.svelte";

  export let template;
  export let onClose;
  // When set, the role is fixed (no role picker, no approver picker — the
  // approver is implicitly the requester) and all API calls go through
  // `apiBase` instead of "/users". Used by the scoped team-lead "assistant
  // management" page (TeamUsers.svelte), where only the "assistant" role and
  // only the requester's own assigned users can ever be touched.
  export let lockedRole = null;
  export let apiBase = "/users";
  let dialog;
  $: isNew = !template.id;
  let email = template.email || "";
  let first_name = template.first_name || "";
  let last_name = template.last_name || "";
  let role = lockedRole || template.role || "employee";
  let weekly_hours = fmtDecimal(template.weekly_hours ?? 39, 2);
  let workdays_per_week = Math.min(template.workdays_per_week ?? 5, 5);
  $: _thisYear = appTodayDate($settings?.timezone).getFullYear();
  $: _nextYear = _thisYear + 1;
  // Base annual leave entitlement (days/year), used whenever no per-year
  // override below exists. Defaults to the org-wide setting for new users,
  // but admins may set a different value (e.g. special agreements).
  let annual_leave_days = template.annual_leave_days ?? 30;
  // Leave days — two explicit per-year override fields (current + next year)
  let leave_days_current_year = 30;
  let leave_days_next_year = 30;
  let todayIso = appTodayIsoDate($settings?.timezone);
  let lastTodayIso = todayIso;
  let start_date = template.start_date || todayIso;
  // Optional employment-start anchor for leave proration; "" = unset (falls
  // back to start_date on the backend). Lets admins onboard an employee who
  // already worked the full year before adopting Zerf mid-year without their
  // entitlement being wrongly pro-rated from the (later) Zerf start date.
  let hire_date = template.hire_date || "";
  let overtime_start_balance_hours = fmtDecimal(
    Math.round((template.overtime_start_balance_min || 0) / 60 * 100) / 100,
    2,
  );
  let approver_ids = Array.isArray(template.approver_ids) ? template.approver_ids.map(Number) : [];
  let active = template.active ?? true;
  let tracks_time = template.tracks_time ?? true;
  let error = "";
  let approvers = [];
  let allCategories = [];
  let allAbsenceCategories = [];
  let selectedCategoryIds = [];
  let selectedAbsenceCategoryIds = [];
  $: normalizedRole = String(role || "").trim().toLowerCase();
  $: requiresApprover = !lockedRole && normalizedRole !== "admin";
  $: isAssistantRole = normalizedRole === "assistant";

  function roleDisplayLabel(r) {
    switch (r) {
      case "admin":
        return "Admin";
      case "team_lead":
        return "Team lead";
      case "assistant":
        return "Assistant";
      default:
        return "Employee";
    }
  }
  $: if (isAssistantRole) {
    weekly_hours = fmtDecimal(0, 2);
    overtime_start_balance_hours = fmtDecimal(0, 2);
  }
  // Non-admin users always have tracks_time=true (backend enforces this too).
  $: if (normalizedRole !== "admin") tracks_time = true;

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
  // eslint-disable-next-line no-useless-assignment
  $: lastTodayIso = todayIso;

  // Rejection sampling to avoid modulo bias (matches backend approach).
  function secureIndex(max) {
    const limit = 2 ** 32 - (2 ** 32 % max);
    let value;
    do {
      const buf = new Uint32Array(1);
      crypto.getRandomValues(buf);
      value = buf[0];
    } while (value >= limit);
    return value % max;
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
    const lower = "abcdefghjkmnpqrstuvwxyz";
    const upper = "ABCDEFGHJKLMNPQRSTUVWXYZ";
    const digits = "23456789";
    const symbols = "!@#*-_+";
    const all = lower + upper + digits + symbols;
    let generatedPassword = pick(lower) + pick(upper) + pick(digits) + pick(symbols);
    while (generatedPassword.length < 16) generatedPassword += pick(all);
    generatedPassword = shuffle(generatedPassword);
    password = generatedPassword;
    confirmPassword = generatedPassword;
  }

  onMount(async () => {
    if (!lockedRole) {
      try {
        const allUsers = await api("/users");
        approvers = allUsers
          .filter(
            (candidateUser) =>
              candidateUser.active &&
              (candidateUser.role === "team_lead" || candidateUser.role === "admin") &&
              candidateUser.id !== template.id,
          )
          .sort((a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name));
      } catch {
        approvers = [];
      }
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
    // Prefill defaults for new users. Skipped in locked-role mode: `/settings`
    // is admin-only and weekly_hours/overtime are forced to 0 for assistants
    // anyway, so there is nothing useful to prefill here.
    if (isNew && !lockedRole) {
      try {
        const settings = await api("/settings");
        if (settings.default_weekly_hours != null) {
          weekly_hours = fmtDecimal(Number(settings.default_weekly_hours), 2);
        }
        if (settings.default_annual_leave_days != null) {
          annual_leave_days = Number(settings.default_annual_leave_days);
          leave_days_current_year = Number(settings.default_annual_leave_days);
          leave_days_next_year = Number(settings.default_annual_leave_days);
        }
        smtpEnabled = !!settings.smtp_enabled;
      } catch {}
    }
    // In locked-role mode (team lead creating assistant), fetch SMTP status
    // from public settings so the TempPasswordDialog shows the correct notice.
    if (isNew && lockedRole) {
      try {
        const pubSettings = await api("/settings/public");
        smtpEnabled = !!pubSettings.smtp_enabled;
      } catch {}
    }
    if (isNew) {
      // Categories/absence categories default to "all enabled" (matching
      // the backend default), but shown as checkboxes so the admin can
      // deselect some before the user is even created.
      try {
        allCategories = await api("/categories/all");
        selectedCategoryIds = allCategories.map((c) => c.id);
      } catch {
        allCategories = [];
      }
      try {
        allAbsenceCategories = await api("/absence-categories/all");
        selectedAbsenceCategoryIds = allAbsenceCategories.map((c) => c.id);
      } catch {
        allAbsenceCategories = [];
      }
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
    if (!isAssistantRole && (Number(workdays_per_week) < 1 || Number(workdays_per_week) > 5)) {
      error = $t("Workdays per week must be between 1 and 5.");
      return;
    }
    // Double-confirmation when disabling time tracking for an existing admin user.
    // All their time entries, absences, and edit requests will be permanently deleted.
    const wasTracksTime = template.tracks_time ?? true;
    if (!isNew && !tracks_time && wasTracksTime && normalizedRole === "admin") {
      const firstConfirmed = await confirmDialog(
        $t("Disable time tracking?"),
        $t("Disabling time tracking will permanently delete all time entries, absences, and edit requests for this user. This cannot be undone."),
        { danger: true, confirm: $t("Disable time tracking") },
      );
      if (!firstConfirmed) return;
      const secondConfirmed = await confirmDialog(
        $t("Disable time tracking?"),
        $t("Disabling time tracking will permanently delete all time entries, absences, and edit requests for this user. This cannot be undone."),
        { danger: true, confirm: $t("Disable time tracking"), requirePhrase: $t("I understand") },
      );
      if (!secondConfirmed) return;
    }
    try {
      const normalizedWeeklyHours = isAssistantRole ? 0 : (parseDecimal(weekly_hours) || 0);
      const normalizedOvertimeStartBalanceMin = isAssistantRole
        ? 0
        : Math.round(Math.round((parseDecimal(overtime_start_balance_hours) || 0) * 100) / 100 * 60);
      const body = {
        email,
        first_name,
        last_name,
        role: normalizedRole,
        weekly_hours: normalizedWeeklyHours,
        ...(isAssistantRole ? {} : { workdays_per_week: Number(workdays_per_week) }),
        annual_leave_days: Number(annual_leave_days),
        leave_days_current_year: Number(leave_days_current_year),
        leave_days_next_year: Number(leave_days_next_year),
        start_date,
        // Always send explicitly: `null` clears it back to the start_date
        // fallback on update, and is simply stored as unset on create.
        hire_date: hire_date || null,
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
      if (isNew) {
        body.category_ids = selectedCategoryIds;
        body.absence_category_ids = selectedAbsenceCategoryIds;
      }
      if (!isNew) {
        body.active = active;
      }
      // Only admin users may have tracks_time=false; non-admin always sends true
      // to be consistent with the backend's auto-restore on demotion.
      body.tracks_time = normalizedRole === "admin" ? tracks_time : true;
      if (isNew) {
        const createdUser = await api(apiBase, { method: "POST", body });
        dialog.close(true);
        showTempPassword = createdUser.temporary_password;
      } else {
        await api(apiBase + "/" + template.id, { method: "PUT", body });
        toast($t("User updated."), "ok");
        dialog.close(true);
        onClose(true);
      }
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }

  function dismissTempPassword() {
    showTempPassword = null;
    dialog.close(true);
    onClose(true);
  }
</script>

{#if showTempPassword}
  <TempPasswordDialog
    password={showTempPassword}
    {smtpEnabled}
    title={$t("User created.")}
    onDismiss={dismissTempPassword}
  />
{/if}

<Dialog
  bind:this={dialog}
  title={$t(isNew ? "Add User" : "Edit User")}
  onClose={() => onClose(false)}
  style="max-width:520px"
  let:dlg
>
  {#if !showTempPassword}

    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label" for="user-first-name">{$t("First name")}</label>
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
      <div>
        <label class="zf-label" for="user-role">{$t("Role")}</label>
        {#if lockedRole}
          <input
            id="user-role"
            class="zf-input"
            value={$t(roleDisplayLabel(lockedRole))}
            disabled
          />
          <div class="field-hint">
            {$t("You will be set as their approver.")}
          </div>
        {:else}
          <select id="user-role" class="zf-select" bind:value={role}>
            <option value="employee">{$t("Employee")}</option>
            <option value="assistant">{$t("Assistant")}</option>
            <option value="team_lead">{$t("Team lead")}</option>
            <option value="admin">{$t("Admin")}</option>
          </select>
        {/if}
      </div>
      <div class="field-row">
        <div>
          <div class="field-label-row">
            <label class="zf-label" for="user-start-date">{$t("Start date")}</label>
          </div>
          <DatePicker
            id="user-start-date"
            bind:value={start_date}
            container={dlg}
          />
        </div>
        <div>
          <div class="field-label-row">
            <label class="zf-label" for="user-hire-date">{$t("Hire date")}</label>
            {#if hire_date}
              <button
                type="button"
                class="zf-btn-icon-sm zf-btn-ghost"
                title={$t("Clear")}
                on:click={() => (hire_date = "")}
              >
                <Icon name="X" size={14} />
              </button>
            {/if}
          </div>
          <DatePicker id="user-hire-date" bind:value={hire_date} container={dlg} />
          <div class="field-hint">
            {$t(
              "Used to calculate the prorated annual leave entitlement for employees who already worked before they started using Zerf. Leave empty to use the start date.",
            )}
          </div>
        </div>
      </div>
      {#if !isAssistantRole}
        <div class="field-row">
          <div>
            <label class="zf-label" for="user-weekly-hours">{$t("Weekly hours")}</label>
            <input
              id="user-weekly-hours"
              class="zf-input"
              type="text"
              inputmode="decimal"
              bind:value={weekly_hours}
            />
          </div>
          <div>
            <label class="zf-label" for="user-workdays-per-week">{$t("Workdays per week")}</label>
            <input
              id="user-workdays-per-week"
              class="zf-input"
              type="number"
              step="1"
              min="1"
              max="5"
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
            type="text"
            inputmode="decimal"
            bind:value={overtime_start_balance_hours}
          />
          <div class="field-hint">
            {$t(
              "Initial overtime balance in hours when the user starts. Negative = deficit.",
            )}
          </div>
        </div>
      {/if}
      <div>
        <div class="field-section-label">{$t("Vacation days per year")}</div>
        <div>
          <label class="zf-label" for="leave-base">{$t("Annual leave days (base)")}</label>
          <input
            id="leave-base"
            class="zf-input"
            type="number"
            min="0"
            max="366"
            bind:value={annual_leave_days}
          />
          <div class="field-hint">
            {$t(
              "Default entitlement used for every year unless overridden below (e.g. for special agreements).",
            )}
          </div>
        </div>
        <div class="field-row">
          <div>
            <label class="zf-label" for="leave-cur"
              >{$t("Override")} {_thisYear}</label
            >
            <input
              id="leave-cur"
              class="zf-input"
              type="number"
              min="0"
              max="366"
              bind:value={leave_days_current_year}
            />
          </div>
          <div>
            <label class="zf-label" for="leave-nxt"
              >{$t("Override")} {_nextYear}</label
            >
            <input
              id="leave-nxt"
              class="zf-input"
              type="number"
              min="0"
              max="366"
              bind:value={leave_days_next_year}
            />
          </div>
        </div>
      </div>
      {#if !isNew}
        <div class="field-toggle-row">
          <div>
            <div class="field-toggle-row-title">{$t("Account active")}</div>
            <div class="field-toggle-row-hint">
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
      {#if normalizedRole === "admin"}
        <div class="field-toggle-row">
          <div>
            <div class="field-toggle-row-title">{$t("Enable time tracking")}</div>
            <div class="field-toggle-row-hint">
              {$t("When disabled, this admin works in management-only mode (no time entries or absences).")}
            </div>
          </div>
          <button
            class="zf-btn zf-btn-sm"
            class:zf-btn-danger={!tracks_time}
            type="button"
            on:click={() => (tracks_time = !tracks_time)}
          >
            {tracks_time ? $t("Active") : $t("Inactive")}
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
              {#each approvers as a (a.id)}
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
          <div class="field-hint">
            {$t("At least one approver is required for employees and team leads.")}
          </div>
        </div>
      {/if}
      {#if isNew && allCategories.length > 0}
        <div>
          <div class="zf-label">{$t("Time Categories")}</div>
          <div style="display:flex;flex-direction:column;gap:6px;max-height:160px;overflow-y:auto;border:1px solid var(--border);border-radius:var(--radius-sm);padding:8px">
            {#each allCategories as c (c.id)}
              <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:13px">
                <input
                  type="checkbox"
                  value={c.id}
                  bind:group={selectedCategoryIds}
                />
                {$t(c.name)}
              </label>
            {/each}
          </div>
        </div>
      {/if}
      {#if isNew && allAbsenceCategories.length > 0}
        <div>
          <div class="zf-label">{$t("Absence Categories")}</div>
          <div style="display:flex;flex-direction:column;gap:6px;max-height:160px;overflow-y:auto;border:1px solid var(--border);border-radius:var(--radius-sm);padding:8px">
            {#each allAbsenceCategories as c (c.id)}
              <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:13px">
                <input
                  type="checkbox"
                  value={c.id}
                  bind:group={selectedAbsenceCategoryIds}
                />
                {$t(c.name)}
              </label>
            {/each}
          </div>
        </div>
      {/if}
      <div class="error-text">{error}</div>
    </div>
  {/if}
  <svelte:fragment slot="footer">
    {#if !showTempPassword}
      <button class="zf-btn" on:click={() => dialog.close()}>{$t("Cancel")}</button>
      <button class="zf-btn zf-btn-primary" on:click={save}>
        {$t(isNew ? "Add User" : "Save")}
      </button>
    {/if}
  </svelte:fragment>
</Dialog>
