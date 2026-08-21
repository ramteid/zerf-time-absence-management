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
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

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
  let leaveAccounts = [];
  let leaveAccountsLoaded = false;
  let leaveAccountsLoading = false;
  let leaveAccountsLoadError = "";
  // Aushilfen start with zero values for every leave account. Preserve all
  // account values when the role is changed temporarily, so a switch back
  // before saving does not discard deliberate individual entitlements.
  let _leaveSnapshot = null;
  let assistantSnapshotPending = false;
  let todayIso = appTodayIsoDate($settings?.timezone);
  let lastTodayIso = todayIso;
  let start_date = template.start_date || todayIso;
  // Optional employment-start anchor for leave proration; "" = unset (falls
  // back to start_date on the backend). Lets admins onboard an employee who
  // already worked the full year before adopting Zerf mid-year without their
  // entitlement being wrongly pro-rated from the (later) Zerf start date.
  let hire_date = template.hire_date || "";
  // Flextime hours the employee already carried before the account existed.
  // Asked once, when the account is created, and stored as the first booking
  // in their flextime ledger — never as an editable profile setting, because
  // changing it later would move every balance ever reported for them. Later
  // changes go through the flextime account view instead.
  let flextime_opening_balance_hours = fmtDecimal(0, 2);
  let approver_ids = Array.isArray(template.approver_ids)
    ? template.approver_ids.map(Number)
    : [];
  let active = template.active ?? true;
  let tracks_time = template.tracks_time ?? true;
  // Admin-only: opt in to technical system-error notifications. Default off.
  let receives_error_notifications =
    template.receives_error_notifications ?? false;
  let error = "";
  let approvers = [];
  let allCategories = [];
  let allAbsenceCategories = [];
  let selectedCategoryIds = [];
  let selectedAbsenceCategoryIds = [];
  $: normalizedRole = String(role || "")
    .trim()
    .toLowerCase();
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

  function cloneLeaveAccounts(accounts) {
    return accounts.map((account) => ({ ...account }));
  }

  function zeroLeaveAccounts(accounts) {
    return accounts.map((account) => ({
      ...account,
      base_days: 0,
      current_year_days: 0,
      next_year_days: 0,
    }));
  }

  function newUserLeaveAccounts(definitions) {
    return definitions.map((definition) => ({
      category_id: definition.category_id,
      category_name: definition.category_name,
      color: definition.color,
      active: definition.active,
      base_days: definition.default_base_days,
      current_year: _thisYear,
      current_year_days: definition.default_base_days,
      next_year: _nextYear,
      next_year_days: definition.default_base_days,
    }));
  }

  function applyLoadedLeaveAccounts(accounts) {
    if (isAssistantRole && (isNew || assistantSnapshotPending)) {
      if (assistantSnapshotPending) {
        _leaveSnapshot = cloneLeaveAccounts(accounts);
        assistantSnapshotPending = false;
      }
      leaveAccounts = zeroLeaveAccounts(accounts);
      return;
    }
    leaveAccounts = accounts;
  }

  async function loadLeaveAccounts() {
    leaveAccountsLoading = true;
    leaveAccountsLoadError = "";
    try {
      const path = isNew
        ? "/leave-accounts"
        : `/users/${template.id}/leave-accounts`;
      const rows = await api(path);
      if (!Array.isArray(rows)) {
        throw new Error("Invalid leave accounts response.");
      }
      const accounts = isNew ? newUserLeaveAccounts(rows) : rows;
      applyLoadedLeaveAccounts(accounts);
      leaveAccountsLoaded = true;
    } catch {
      leaveAccounts = [];
      leaveAccountsLoaded = false;
      leaveAccountsLoadError = $t("Leave accounts could not be loaded.");
    } finally {
      leaveAccountsLoading = false;
    }
  }

  function leaveAccountsPayload() {
    const payload = [];
    for (const account of leaveAccounts) {
      const values = [
        account.base_days,
        account.current_year_days,
        account.next_year_days,
      ].map(Number);
      if (
        values.some(
          (value) => !Number.isInteger(value) || value < 0 || value > 366,
        )
      ) {
        error = $t("Leave account values must be between 0 and 366.");
        return null;
      }
      payload.push({
        category_id: account.category_id,
        base_days: values[0],
        current_year_days: values[1],
        next_year_days: values[2],
      });
    }
    return payload;
  }

  function changeRole(nextRole) {
    const previousRole = normalizedRole;
    const normalizedNextRole = String(nextRole || "")
      .trim()
      .toLowerCase();
    if (normalizedNextRole === previousRole) return;

    if (normalizedNextRole === "assistant") {
      if (leaveAccountsLoaded) {
        _leaveSnapshot = cloneLeaveAccounts(leaveAccounts);
      } else {
        assistantSnapshotPending = true;
      }
      leaveAccounts = zeroLeaveAccounts(leaveAccounts);
    } else if (previousRole === "assistant" && _leaveSnapshot !== null) {
      leaveAccounts = cloneLeaveAccounts(_leaveSnapshot);
      _leaveSnapshot = null;
      assistantSnapshotPending = false;
    }
    role = normalizedNextRole;
  }

  $: if (isAssistantRole) {
    weekly_hours = fmtDecimal(0, 2);
    flextime_opening_balance_hours = fmtDecimal(0, 2);
  }

  // The carry-in balance is only asked for on accounts that will actually have
  // a flextime ledger: not for assistants (no flextime account) and not for a
  // pure-admin account created with time tracking switched off.
  $: showOpeningBalanceField = isNew && !isAssistantRole && tracks_time;

  // Non-admin users always have tracks_time=true (backend enforces this too).
  $: if (normalizedRole !== "admin") tracks_time = true;

  // Error notifications are admin-only; clear the flag for any other role
  // (backend coerces this too).
  $: if (normalizedRole !== "admin") receives_error_notifications = false;

  // Password fields (only for new users)
  let password = "";
  let confirmPassword = "";
  let showTempPassword = null;
  let smtpEnabled = false;

  // Keep untouched start-date default aligned with timezone changes.
  $: todayIso = appTodayIsoDate($settings?.timezone);
  $: if (
    isNew &&
    !template.start_date &&
    start_date === lastTodayIso &&
    todayIso !== lastTodayIso
  ) {
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
    for (
      let currentIndex = shuffledCharacters.length - 1;
      currentIndex > 0;
      currentIndex--
    ) {
      const randomIndex = secureIndex(currentIndex + 1);
      [shuffledCharacters[currentIndex], shuffledCharacters[randomIndex]] = [
        shuffledCharacters[randomIndex],
        shuffledCharacters[currentIndex],
      ];
    }
    return shuffledCharacters.join("");
  }

  function generatePassword() {
    const lower = "abcdefghjkmnpqrstuvwxyz";
    const upper = "ABCDEFGHJKLMNPQRSTUVWXYZ";
    const digits = "23456789";
    const symbols = "!@#*-_+";
    const all = lower + upper + digits + symbols;
    let generatedPassword =
      pick(lower) + pick(upper) + pick(digits) + pick(symbols);
    while (generatedPassword.length < 16) generatedPassword += pick(all);
    generatedPassword = shuffle(generatedPassword);
    password = generatedPassword;
    confirmPassword = generatedPassword;
  }

  onMount(async () => {
    if (!lockedRole) {
      try {
        const allUsers = await api("/users");
        approvers = sortUsersByRoleThenName(
          allUsers.filter(
            (candidateUser) =>
              candidateUser.active &&
              (candidateUser.role === "team_lead" ||
                candidateUser.role === "admin") &&
              candidateUser.id !== template.id,
          ),
        );
      } catch {
        approvers = [];
      }
    }
    await loadLeaveAccounts();
    // Prefill global user defaults for new users. Leave-account defaults are
    // fetched separately from their category definitions above, because each
    // account owns its own standard entitlement.
    if (isNew && !lockedRole) {
      try {
        const settings = await api("/settings");
        if (settings.default_weekly_hours != null) {
          weekly_hours = fmtDecimal(Number(settings.default_weekly_hours), 2);
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
      error = $t(
        "At least one approver is required for employees and team leads.",
      );
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
    if (leaveAccountsLoading) {
      error = $t("Leave accounts are still loading.");
      return;
    }
    if (!leaveAccountsLoaded) {
      error =
        leaveAccountsLoadError || $t("Leave accounts could not be loaded.");
      return;
    }
    const leave_accounts = leaveAccountsPayload();
    if (leave_accounts === null) return;
    if (
      !isAssistantRole &&
      (Number(workdays_per_week) < 1 || Number(workdays_per_week) > 5)
    ) {
      error = $t("Workdays per week must be between 1 and 5.");
      return;
    }
    // Double-confirmation when disabling time tracking for an existing admin user.
    // All their time entries, absences, and edit requests will be permanently deleted.
    const wasTracksTime = template.tracks_time ?? true;
    if (!isNew && !tracks_time && wasTracksTime && normalizedRole === "admin") {
      const firstConfirmed = await confirmDialog(
        $t("Disable time tracking?"),
        $t(
          "Disabling time tracking will permanently delete all time entries, absences, and edit requests for this user. This cannot be undone.",
        ),
        { danger: true, confirm: $t("Disable time tracking") },
      );
      if (!firstConfirmed) return;
      const secondConfirmed = await confirmDialog(
        $t("Disable time tracking?"),
        $t(
          "Disabling time tracking will permanently delete all time entries, absences, and edit requests for this user. This cannot be undone.",
        ),
        {
          danger: true,
          confirm: $t("Disable time tracking"),
          requirePhrase: $t("I understand"),
        },
      );
      if (!secondConfirmed) return;
    }
    try {
      let normalizedWeeklyHours;
      if (isAssistantRole) {
        normalizedWeeklyHours = 0;
      } else {
        const parsed = parseDecimal(weekly_hours);
        if (!Number.isFinite(parsed)) {
          error = $t("Weekly hours are required.");
          return;
        }
        if (parsed <= 0) {
          error = $t("Weekly hours must be greater than 0 for this role.");
          return;
        }
        normalizedWeeklyHours = parsed;
      }
      const normalizedOpeningBalanceMin = showOpeningBalanceField
        ? Math.round(
            (Math.round(
              (parseDecimal(flextime_opening_balance_hours) || 0) * 100,
            ) /
              100) *
              60,
          )
        : 0;
      const body = {
        email,
        first_name,
        last_name,
        role: normalizedRole,
        weekly_hours: normalizedWeeklyHours,
        ...(isAssistantRole
          ? {}
          : { workdays_per_week: Number(workdays_per_week) }),
        leave_accounts,
        start_date,
        // Always send explicitly: `null` clears it back to the start_date
        // fallback on update, and is simply stored as unset on create.
        hire_date: hire_date || null,
      };
      // Only ever sent on create: the balance is a ledger booking, not a
      // setting, so an update must not be able to carry one.
      if (showOpeningBalanceField) {
        body.flextime_opening_balance_min = normalizedOpeningBalanceMin;
      }
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
      // Admin-only opt-in for technical error notifications; non-admins send false.
      body.receives_error_notifications =
        normalizedRole === "admin" ? receives_error_notifications : false;
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
  wide
  let:dlg
>
  {#if !showTempPassword}
    <div class="field-group">
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
          <select
            id="user-role"
            class="zf-select"
            value={role}
            on:change={(event) => changeRole(event.currentTarget.value)}
          >
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
            <label class="zf-label" for="user-start-date"
              >{$t("Start date")}</label
            >
          </div>
          <DatePicker
            id="user-start-date"
            bind:value={start_date}
            container={dlg}
          />
        </div>
        <div>
          <div class="field-label-row">
            <label class="zf-label" for="user-hire-date"
              >{$t("Hire date")}</label
            >
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
          <DatePicker
            id="user-hire-date"
            bind:value={hire_date}
            container={dlg}
          />
          <div class="field-hint">
            {$t(
              "Used to calculate the prorated leave-account entitlement for employees who already worked before they started using the application. Leave empty to use the start date.",
            )}
          </div>
        </div>
      </div>
      {#if !isAssistantRole}
        <div class="field-row">
          <div>
            <label class="zf-label" for="user-weekly-hours"
              >{$t("Weekly hours")}</label
            >
            <input
              id="user-weekly-hours"
              class="zf-input"
              type="text"
              inputmode="decimal"
              bind:value={weekly_hours}
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
              max="5"
              bind:value={workdays_per_week}
            />
          </div>
        </div>
        {#if showOpeningBalanceField}
          <div>
            <label class="zf-label" for="user-opening-balance"
              >{$t("Flextime hours brought along")}</label
            >
            <input
              id="user-opening-balance"
              class="zf-input"
              type="text"
              inputmode="decimal"
              bind:value={flextime_opening_balance_hours}
            />
            <div class="field-hint">
              {$t(
                "Flextime hours this person has already built up elsewhere. Booked once on their start date. Negative means they start with a shortfall.",
              )}
            </div>
          </div>
        {:else if !isNew}
          <div class="field-hint">
            {$t(
              "The flextime balance is managed on the user's flextime account, not here.",
            )}
          </div>
        {/if}
      {/if}
      <div>
        <div class="field-section-label">{$t("Leave accounts")}</div>
        {#if leaveAccountsLoading}
          <div class="field-hint">{$t("Loading leave accounts...")}</div>
        {:else if leaveAccountsLoadError}
          <div class="error-text">{leaveAccountsLoadError}</div>
        {:else if leaveAccounts.length === 0}
          <div class="field-hint">
            {$t("No leave accounts are configured.")}
          </div>
        {:else}
          <div class="leave-account-editor-list">
            {#each leaveAccounts as leaveAccount (leaveAccount.category_id)}
              <section class="leave-account-editor">
                <div class="leave-account-editor-title">
                  <span
                    class="leave-account-editor-dot"
                    style:background={leaveAccount.color || "#64748b"}
                  ></span>
                  <span>{$t(leaveAccount.category_name)}</span>
                </div>
                <div class="field-row">
                  <div>
                    <label
                      class="zf-label"
                      for={`leave-account-${leaveAccount.category_id}-base`}
                      >{$t("Base entitlement")}</label
                    >
                    <input
                      id={`leave-account-${leaveAccount.category_id}-base`}
                      class="zf-input"
                      type="number"
                      min="0"
                      max="366"
                      bind:value={leaveAccount.base_days}
                    />
                  </div>
                  <div>
                    <label
                      class="zf-label"
                      for={`leave-account-${leaveAccount.category_id}-current`}
                      >{$t("Override")} {leaveAccount.current_year}</label
                    >
                    <input
                      id={`leave-account-${leaveAccount.category_id}-current`}
                      class="zf-input"
                      type="number"
                      min="0"
                      max="366"
                      bind:value={leaveAccount.current_year_days}
                    />
                  </div>
                  <div>
                    <label
                      class="zf-label"
                      for={`leave-account-${leaveAccount.category_id}-next`}
                      >{$t("Override")} {leaveAccount.next_year}</label
                    >
                    <input
                      id={`leave-account-${leaveAccount.category_id}-next`}
                      class="zf-input"
                      type="number"
                      min="0"
                      max="366"
                      bind:value={leaveAccount.next_year_days}
                    />
                  </div>
                </div>
              </section>
            {/each}
          </div>
        {/if}
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
            <div class="field-toggle-row-title">
              {$t("Enable time tracking")}
            </div>
            <div class="field-toggle-row-hint">
              {$t(
                "When disabled, this admin works in management-only mode (no time entries or absences).",
              )}
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
        <div class="field-toggle-row">
          <div>
            <div class="field-toggle-row-title">
              {$t("Receives notifications about technical system errors")}
            </div>
            <div class="field-toggle-row-hint">
              {$t(
                "When enabled, this admin is alerted in the app and by email about technical errors.",
              )}
            </div>
          </div>
          <button
            class="zf-btn zf-btn-sm"
            class:zf-btn-danger={!receives_error_notifications}
            type="button"
            on:click={() =>
              (receives_error_notifications = !receives_error_notifications)}
          >
            {receives_error_notifications ? $t("Active") : $t("Inactive")}
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
            <div class="empty-note">
              {$t("No eligible approvers found.")}
            </div>
          {:else}
            <div class="check-list">
              {#each approvers as a (a.id)}
                <label class="zf-check-label">
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
            {$t(
              "At least one approver is required for employees and team leads.",
            )}
          </div>
        </div>
      {/if}
      {#if isNew && allCategories.length > 0}
        <div>
          <div class="zf-label">{$t("Time Categories")}</div>
          <div class="check-list">
            {#each allCategories as c (c.id)}
              <label class="zf-check-label">
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
          <div class="check-list">
            {#each allAbsenceCategories as c (c.id)}
              <label class="zf-check-label">
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
      <button class="zf-btn" on:click={() => dialog.close()}
        >{$t("Cancel")}</button
      >
      <button class="zf-btn zf-btn-primary" on:click={save}>
        {$t(isNew ? "Add User" : "Save")}
      </button>
    {/if}
  </svelte:fragment>
</Dialog>

<style>
  .empty-note {
    font-size: 0.875rem;
    color: var(--text-tertiary);
    padding: 6px 0;
  }

  /* Scrollable checkbox lists (permissions, team members). */
  .check-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 180px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px;
  }

  .leave-account-editor-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .leave-account-editor {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 12px;
  }

  .leave-account-editor-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    margin-bottom: 10px;
  }

  .leave-account-editor-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: 0 0 auto;
  }
</style>
