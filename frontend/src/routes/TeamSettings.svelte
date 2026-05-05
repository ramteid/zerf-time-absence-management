<script>
  import { api } from "../api.js";
  import { toast, currentUser } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";

  let rows = [];
  let loading = true;
  let saving = {};

  async function load() {
    loading = true;
    try {
      rows = await api("/team-settings");
    } finally {
      loading = false;
    }
  }
  load();

  async function toggle(row) {
    saving = { ...saving, [row.user_id]: true };
    try {
      await api(`/team-settings/${row.user_id}`, {
        method: "PUT",
        body: {
          allow_reopen_without_approval: row.allow_reopen_without_approval,
        },
      });
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
      row.allow_reopen_without_approval = !row.allow_reopen_without_approval;
      rows = rows;
    } finally {
      saving = { ...saving, [row.user_id]: false };
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Team Settings")}</h1>
  </div>
</div>

<div class="content-area" style="max-width:760px">
  {#if loading}
    <p>{$t("Loading...")}</p>
  {:else}
    <!-- Reopen Requests section -->
    <div class="kz-card" style="padding:20px;margin-bottom:16px">
      <div style="font-size:14px;font-weight:600;margin-bottom:6px">
        {$t("Reopen Requests")}
      </div>
      <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:14px">
        {$t(
          "When enabled for a user, their reopen requests are automatically approved. Their approver and all admins still receive a notification.",
        )}
      </div>

      {#each rows as row, i}
        <div
          class="team-setting-row"
          style={i < rows.length - 1
            ? "border-bottom:1px solid var(--border);"
            : ""}
        >
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {row.first_name}
              {row.last_name}
              {#if $currentUser?.id === row.user_id}
                <span style="color:var(--text-tertiary);font-weight:400"
                  >· {$t("you")}</span
                >
              {/if}
            </div>
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {roleLabel(row.role)} · {row.email}
            </div>
          </div>
          <label
            style="display:flex;align-items:center;gap:8px;font-size:12.5px;flex-shrink:0"
          >
            <input
              type="checkbox"
              bind:checked={row.allow_reopen_without_approval}
              on:change={() => toggle(row)}
              disabled={saving[row.user_id]}
            />
            <span class="team-setting-checkbox-label"
              >{$t("Auto-approve reopens")}</span
            >
          </label>
        </div>
      {/each}
      {#if rows.length === 0}
        <div style="padding:24px;text-align:center;color:var(--text-tertiary)">
          {$t("No data.")}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .team-setting-row {
    padding: 12px 0;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  @media (max-width: 640px) {
    .team-setting-row {
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
    }
    .team-setting-checkbox-label {
      font-size: 11.5px;
    }
  }
</style>
