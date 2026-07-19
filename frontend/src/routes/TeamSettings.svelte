<script>
  import { api } from "../api.js";
  import { toast, currentUser } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

  let rows = [];
  let loading = true;
  let saving = {};

  async function load() {
    loading = true;
    try {
      const loaded = await api("/team-settings");
      rows = sortUsersByRoleThenName(loaded);
    } catch (e) {
      rows = [];
      toast($t(e?.message || "Error"), "error");
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
          allow_submission_without_approval:
            row.allow_submission_without_approval,
        },
      });
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
      // Re-fetch server state to ensure UI is in sync.
      await load();
    } finally {
      saving = { ...saving, [row.user_id]: false };
    }
  }

  function rowDisabled(row) {
    return (
      saving[row.user_id] ||
      (!$currentUser?.permissions?.is_admin && $currentUser?.id === row.user_id)
    );
  }
</script>

<div class="top-bar page-medium">
  <div class="top-bar-title">
    <h1>{$t("Team Settings")}</h1>
  </div>
</div>

<div class="content-area page-medium">
  {#if loading}
    <p>{$t("Loading...")}</p>
  {:else}
    <!-- Submissions section -->
    <div class="zf-card zf-card-section">
      <div class="zf-card-title mb-6">
        {$t("Time Submissions")}
      </div>
      <div class="text-hint mb-14">
        {$t(
          "When enabled for a user, their submitted weeks are automatically approved. No one is notified and no emails are sent.",
        )}
      </div>

      {#each rows as row (row.user_id)}
        <div class="team-setting-row">
          <div class="flex-min0">
            <div class="zf-item-title">
              {row.first_name}
              {row.last_name}
              {#if $currentUser?.id === row.user_id}
                <span class="text-tertiary fw-400">· {$t("you")}</span>
              {/if}
            </div>
            <div class="text-hint">
              {roleLabel(row.role)} · {row.email}
            </div>
          </div>
          <label class="row-controls">
            <input
              type="checkbox"
              bind:checked={row.allow_submission_without_approval}
              on:change={() => toggle(row)}
              disabled={rowDisabled(row)}
            />
            <span class="team-setting-checkbox-label"
              >{$t("Auto-approve submissions")}</span
            >
          </label>
        </div>
      {/each}
      {#if rows.length === 0}
        <div class="zf-empty">
          {$t("No data.")}
        </div>
      {/if}
    </div>

    <!-- Edit Requests section -->
    <div class="zf-card zf-card-section">
      <div class="zf-card-title mb-6">
        {$t("Edit Requests")}
      </div>
      <div class="text-hint mb-14">
        {$t(
          "When enabled for a user, their edit requests are automatically approved. No one is notified and no emails are sent.",
        )}
      </div>

      {#each rows as row (row.user_id)}
        <div class="team-setting-row">
          <div class="flex-min0">
            <div class="zf-item-title">
              {row.first_name}
              {row.last_name}
              {#if $currentUser?.id === row.user_id}
                <span class="text-tertiary fw-400">· {$t("you")}</span>
              {/if}
            </div>
            <div class="text-hint">
              {roleLabel(row.role)} · {row.email}
            </div>
          </div>
          <label class="row-controls">
            <input
              type="checkbox"
              bind:checked={row.allow_reopen_without_approval}
              on:change={() => toggle(row)}
              disabled={rowDisabled(row)}
            />
            <span class="team-setting-checkbox-label"
              >{$t("Auto-approve edit requests")}</span
            >
          </label>
        </div>
      {/each}
      {#if rows.length === 0}
        <div class="zf-empty">
          {$t("No data.")}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Right side of a member row: approver select + controls. */
  .row-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.84375rem;
    flex-shrink: 0;
  }

  .team-setting-row {
    padding: 12px 0;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .team-setting-row:not(:last-child) {
    border-bottom: 1px solid var(--border);
  }

  @media (max-width: 640px) {
    .team-setting-row {
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
    }
    .team-setting-checkbox-label {
      font-size: 0.78125rem;
    }
  }
</style>
