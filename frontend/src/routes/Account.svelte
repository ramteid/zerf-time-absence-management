<script>
  import { api } from "../api.js";
  import { currentUser, theme, toast } from "../stores.js";
  import { t, roleLabel, formatHours } from "../i18n.js";
  import { fmtDate } from "../format.js";

  let cur = "",
    nw = "",
    nw2 = "",
    error = "";
  let savingTheme = false;
  let leaveOverrides = [];
  const thisYear = new Date().getFullYear();
  const nextYear = thisYear + 1;

  // Re-fetch whenever currentUser becomes available (store starts as null)
  $: if ($currentUser?.id) {
    api(`/users/${$currentUser.id}/leave-overrides`)
      .then((r) => (leaveOverrides = r))
      .catch(() => (leaveOverrides = []));
  }

  function leaveDaysForYear(year) {
    const override = leaveOverrides.find((o) => o.year === year);
    return override != null ? override.days : $currentUser.annual_leave_days;
  }
  function initials(u) {
    return ((u.first_name?.[0] || "") + (u.last_name?.[0] || "")).toUpperCase();
  }

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
      // Update the browser password manager with the new credential so it
      // does not keep offering the old one as an autofill suggestion.
      try {
        if (window.PasswordCredential && $currentUser?.email) {
          const cred = new window.PasswordCredential({
            id: $currentUser.email,
            password: nw,
            name:
              `${$currentUser.first_name || ""} ${$currentUser.last_name || ""}`.trim() ||
              $currentUser.email,
          });
          await navigator.credentials.store(cred);
        }
      } catch (_) {
        // Storing credentials is best-effort; ignore failures.
      }
      currentUser.update((u) => ({ ...u, must_change_password: false }));
      toast($t("Password changed."), "ok");
      cur = "";
      nw = "";
      nw2 = "";
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Account")}</h1>
  </div>
  <div class="top-bar-subtitle">{$t("Your profile & preferences")}</div>
</div>

<div class="content-area" style="max-width:640px">
  {#if $currentUser.must_change_password}
    <div
      class="kz-card"
      style="padding:16px 20px;margin-bottom:16px;border-color:var(--warning)"
    >
      <strong style="color:var(--warning-text)"
        >{$t("Please change your password.")}</strong
      >
      <p style="font-size:13px;color:var(--text-tertiary);margin-top:4px">
        {$t("You are using a temporary password.")}
      </p>
    </div>
  {/if}

  <!-- Profile card -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:16px;margin-bottom:20px">
      <div class="avatar" style="width:56px;height:56px;font-size:21px">
        {initials($currentUser)}
      </div>
      <div>
        <div style="font-size:18px;font-weight:600">
          {$currentUser.first_name}
          {$currentUser.last_name}
        </div>
        <div style="font-size:13px;color:var(--text-tertiary)">
          {roleLabel($currentUser.role)}
        </div>
      </div>
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="account-email">{$t("Email")}</label>
        <input
          id="account-email"
          class="kz-input"
          value={$currentUser.email}
          readonly
          style="color:var(--text-secondary)"
        />
      </div>
      <div>
        <label class="kz-label" for="account-weekly-hours"
          >{$t("Weekly hours")}</label
        >
        <input
          id="account-weekly-hours"
          class="kz-input"
          value={$t("{hours} / week", {
            hours: formatHours($currentUser.weekly_hours),
          })}
          readonly
          style="color:var(--text-secondary)"
        />
      </div>
      <div>
        <label class="kz-label" for="account-annual-leave-this"
          >{$t("Annual leave days")} {thisYear}</label
        >
        <input
          id="account-annual-leave-this"
          class="kz-input"
          value={leaveDaysForYear(thisYear)}
          readonly
          style="color:var(--text-secondary)"
        />
      </div>
      <div>
        <label class="kz-label" for="account-annual-leave-next"
          >{$t("Annual leave days")} {nextYear}</label
        >
        <input
          id="account-annual-leave-next"
          class="kz-input"
          value={leaveDaysForYear(nextYear)}
          readonly
          style="color:var(--text-secondary)"
        />
      </div>
      <div>
        <label class="kz-label" for="account-start-date"
          >{$t("Start date")}</label
        >
        <input
          id="account-start-date"
          class="kz-input"
          value={fmtDate($currentUser.start_date)}
          readonly
          style="color:var(--text-secondary)"
        />
      </div>
    </div>
  </div>

  <!-- Password -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="font-size:14px;font-weight:600;margin-bottom:14px">
      {$t("Change password")}
    </div>
    <div class="field-group">
      {#if !$currentUser.must_change_password}
        <div>
          <label class="kz-label" for="account-current-password"
            >{$t("Current password")}</label
          >
          <input
            id="account-current-password"
            class="kz-input"
            type="password"
            bind:value={cur}
            autocomplete="current-password"
          />
        </div>
      {/if}
      <div class="field-row">
        <div>
          <label class="kz-label" for="account-new-password"
            >{$t("New password (min 12 chars)")}</label
          >
          <input
            id="account-new-password"
            class="kz-input"
            type="password"
            bind:value={nw}
            minlength="12"
            autocomplete="new-password"
          />
        </div>
        <div>
          <label class="kz-label" for="account-confirm-password"
            >{$t("Confirm new password")}</label
          >
          <input
            id="account-confirm-password"
            class="kz-input"
            type="password"
            bind:value={nw2}
            minlength="12"
            autocomplete="new-password"
          />
        </div>
      </div>
      <div class="error-text">{error}</div>
      <div style="display:flex;justify-content:flex-end">
        <button class="kz-btn kz-btn-primary" on:click={changePassword}
          >{$t("Save")}</button
        >
      </div>
    </div>
  </div>

  <!-- Appearance -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="font-size:14px;font-weight:600;margin-bottom:14px">
      {$t("Appearance")}
    </div>
    <div style="display:flex;align-items:center;justify-content:space-between">
      <div>
        <div style="font-size:14px">{$t("Dark mode")}</div>
        <div style="font-size:12px;color:var(--text-tertiary);margin-top:2px">
          {$t("Use dark colour scheme")}
        </div>
      </div>
      <button
        class="kz-btn"
        on:click={toggleDarkMode}
        aria-pressed={$theme === "dark"}
        disabled={savingTheme}
      >
        {$theme === "dark" ? $t("Enabled") : $t("Disabled")}
      </button>
    </div>
  </div>
</div>
