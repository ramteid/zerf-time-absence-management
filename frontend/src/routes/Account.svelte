<script>
  import { api } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import { fmtDate, minToHM } from "../format.js";

  let cur = "",
    nw = "",
    nw2 = "",
    error = "";
  let overtime = [];
  $: cumulative = overtime.reduce((s, m) => s + m.diff_min, 0);

  function initials(u) {
    return ((u.first_name?.[0] || "") + (u.last_name?.[0] || "")).toUpperCase();
  }

  async function loadOvertime() {
    try {
      overtime = await api(
        `/reports/overtime?year=${new Date().getFullYear()}`,
      );
    } catch (e) {
      toast(e.message || $t("Overtime data unavailable."), "error");
    }
  }
  loadOvertime();

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
      toast($t("Password changed."), "ok");
      cur = "";
      nw = "";
      nw2 = "";
    } catch (e) {
      error = e.message;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Account")}</h1>
    <div class="top-bar-subtitle">{$t("Your profile & preferences")}</div>
  </div>
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
          value="{$currentUser.weekly_hours}h / week"
          readonly
          style="color:var(--text-secondary)"
        />
      </div>
      <div>
        <label class="kz-label" for="account-annual-leave"
          >{$t("Annual leave days")}</label
        >
        <input
          id="account-annual-leave"
          class="kz-input"
          value={$currentUser.annual_leave_days}
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

  <!-- Overtime -->
  <div class="kz-card" style="overflow-x:auto">
    <div class="card-header">
      <span class="card-header-title">
        {$t("Overtime balance {year}", { year: new Date().getFullYear() })}
      </span>
      <span
        class="kz-chip"
        class:kz-chip-approved={cumulative >= 0}
        class:kz-chip-rejected={cumulative < 0}
      >
        {minToHM(cumulative)}
      </span>
    </div>
    <div class="kz-table-wrap">
      <table class="kz-table">
        <thead>
          <tr>
            {#each ["Month", "Target", "Actual", "Diff", "Cumulative"] as c}
              <th>{$t(c)}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each overtime as m, i}
            {@const cum = overtime
              .slice(0, i + 1)
              .reduce((s, x) => s + x.diff_min, 0)}
            <tr>
              <td class="tab-num">{m.month}</td>
              <td class="tab-num">{minToHM(m.target_min)}</td>
              <td class="tab-num">{minToHM(m.actual_min)}</td>
              <td
                class="tab-num"
                style="color:{m.diff_min < 0
                  ? 'var(--danger-text)'
                  : 'var(--success-text)'}"
              >
                {minToHM(m.diff_min)}
              </td>
              <td
                class="tab-num"
                style="color:{cum < 0
                  ? 'var(--danger-text)'
                  : 'var(--success-text)'}"
              >
                {minToHM(cum)}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <!-- end kz-table-wrap -->
  </div>
</div>
