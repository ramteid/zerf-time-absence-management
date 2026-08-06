<script>
  import { api } from "../api.js";
  import { currentUser, theme, toast } from "../stores.js";
  import { loadPostAuthData } from "../appData.js";
  import { t, roleLabel, formatHours } from "../i18n.js";
  import { fmtDate } from "../format.js";
  import { isAssistantUser } from "../rolePolicy.js";
  import { userAvatarClass, userInitials } from "../lib/domain/users.js";

  let cur = "",
    nw = "",
    nw2 = "",
    error = "";
  let savingTheme = false;
  $: isAssistantCurrentUser = isAssistantUser($currentUser);

  async function toggleDarkMode() {
    if (savingTheme) return;
    savingTheme = true;
    const next = $theme === "dark" ? false : true;
    try {
      await api("/auth/preferences", {
        method: "PUT",
        body: { dark_mode: next },
      });
      theme.set(next ? "dark" : "light");
      currentUser.update((u) => ({ ...u, dark_mode: next }));
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    } finally {
      savingTheme = false;
    }
  }

  async function changePassword() {
    error = "";
    if (nw !== nw2) {
      error = $t("Passwords do not match.");
      return;
    }
    try {
      await api("/auth/password", {
        method: "PUT",
        body: {
          current_password: $currentUser.must_change_password ? undefined : cur,
          new_password: nw,
        },
      });
      currentUser.update((u) => ({ ...u, must_change_password: false }));
      // A first-login password change lifts the must_change_password gate that
      // made boot skip the per-user data loads. Run the same shared loader now
      // so the session is fully populated (notably the absence-request
      // dropdown) without requiring a manual page reload. Fire-and-forget: the
      // confirmation toast must not wait on these background fetches.
      loadPostAuthData();
      toast($t("Password changed."), "ok");
      cur = "";
      nw = "";
      nw2 = "";
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }
</script>

<div class="top-bar page-narrow">
  <div class="top-bar-title">
    <h1>{$t("Account")}</h1>
    <div class="top-bar-subtitle">{$t("Your profile & preferences")}</div>
  </div>
</div>

<div class="content-area page-narrow">
  {#if $currentUser.must_change_password}
    <div class="zf-card zf-card-warning">
      <strong class="text-warning">{$t("Please change your password.")}</strong>
      <p class="fs-14 text-tertiary mt-4">
        {$t("You are using a temporary password.")}
      </p>
    </div>
  {/if}

  <!-- Profile card -->
  <div class="zf-card zf-card-section">
    <div class="profile-head">
      <div class="avatar avatar-lg {userAvatarClass($currentUser)}">
        {userInitials($currentUser)}
      </div>
      <div>
        <div class="profile-name">
          {$currentUser.first_name}
          {$currentUser.last_name}
        </div>
        <div class="fs-14 text-tertiary">
          {roleLabel($currentUser.role)}
        </div>
      </div>
    </div>
    <div class="field-row">
      <div>
        <label class="zf-label" for="account-email">{$t("Email")}</label>
        <input
          id="account-email"
          class="zf-input text-secondary"
          value={$currentUser.email}
          readonly
        />
      </div>
      {#if !isAssistantCurrentUser}
        <div>
          <label class="zf-label" for="account-weekly-hours"
            >{$t("Weekly hours")}</label
          >
          <input
            id="account-weekly-hours"
            class="zf-input text-secondary"
            value={$t("{hours} / week", {
              hours: formatHours($currentUser.weekly_hours),
            })}
            readonly
          />
        </div>
        <div>
          <label class="zf-label" for="account-workdays-per-week"
            >{$t("Workdays per week")}</label
          >
          <input
            id="account-workdays-per-week"
            class="zf-input text-secondary"
            value={$currentUser.workdays_per_week}
            readonly
          />
        </div>
      {/if}
      <div>
        <label class="zf-label" for="account-start-date"
          >{$t("Start date")}</label
        >
        <input
          id="account-start-date"
          class="zf-input text-secondary"
          value={fmtDate($currentUser.start_date)}
          readonly
        />
      </div>
      {#if $currentUser.approvers && $currentUser.approvers.length > 0}
        <div>
          <div class="zf-label">{$t("Approvers")}</div>
          <div class="text-note value-line">
            {$currentUser.approvers
              .map((a) => `${a.first_name} ${a.last_name}`)
              .join(", ")}
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Password -->
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Change password")}</div>
    <div class="field-group">
      {#if !$currentUser.must_change_password}
        <div>
          <label class="zf-label" for="account-current-password"
            >{$t("Current password")}</label
          >
          <input
            id="account-current-password"
            class="zf-input"
            type="password"
            bind:value={cur}
            autocomplete="current-password"
          />
        </div>
      {/if}
      <div class="field-row">
        <div>
          <label class="zf-label" for="account-new-password"
            >{$t("New password (min 12 chars)")}</label
          >
          <input
            id="account-new-password"
            class="zf-input"
            type="password"
            bind:value={nw}
            minlength="12"
            autocomplete="new-password"
          />
        </div>
        <div>
          <label class="zf-label" for="account-confirm-password"
            >{$t("Confirm new password")}</label
          >
          <input
            id="account-confirm-password"
            class="zf-input"
            type="password"
            bind:value={nw2}
            minlength="12"
            autocomplete="new-password"
          />
        </div>
      </div>
      <div class="error-text">{error}</div>
      <div class="form-actions">
        <button class="zf-btn zf-btn-primary" on:click={changePassword}
          >{$t("Save")}</button
        >
      </div>
    </div>
  </div>

  <!-- Appearance -->
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Appearance")}</div>
    <div class="field-toggle-row">
      <div>
        <div class="field-toggle-row-title">{$t("Dark mode")}</div>
        <div class="field-toggle-row-hint">{$t("Use dark colour scheme")}</div>
      </div>
      <button
        class="zf-btn"
        on:click={toggleDarkMode}
        aria-pressed={$theme === "dark"}
        disabled={savingTheme}
      >
        {$theme === "dark" ? $t("Enabled") : $t("Disabled")}
      </button>
    </div>
  </div>
</div>

<style>
  .profile-head {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 20px;
  }

  .profile-name {
    font-size: 1.1875rem;
    font-weight: 400;
  }

  /* Read-only preference value aligned with the input rows around it. */
  .value-line {
    padding: 6px 0;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
  }

</style>
