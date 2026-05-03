<script>
  // Unified Team-Policy page used by both team leads (own row only) and
  // admins (matrix of all approvers).  The backend filters the list
  // automatically based on the caller's role.
  import { api } from "../api.js";
  import { toast, currentUser } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";

  let rows = [];
  let loading = true;
  let saving = {};

  async function load() {
    loading = true;
    try {
      rows = await api("/team-policy");
    } finally {
      loading = false;
    }
  }
  load();

  async function toggle(row) {
    saving[row.approver_id] = true;
    try {
      await api(`/team-policy/${row.approver_id}`, {
        method: "PUT",
        body: {
          allow_reopen_without_approval: row.allow_reopen_without_approval,
        },
      });
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast(e.message || $t("Error"), "error");
      // Revert visual state on failure.
      row.allow_reopen_without_approval = !row.allow_reopen_without_approval;
      rows = rows;
    } finally {
      saving[row.approver_id] = false;
    }
  }

  $: isAdmin = $currentUser?.permissions?.is_admin;
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Team Policy")}</h1>
    <div class="top-bar-subtitle"></div>
  </div>
</div>

<div class="content-area" style="max-width:760px">
  {#if loading}
    <p>{$t("Loading...")}</p>
  {:else}
    <div class="kz-card" style="overflow-x:auto">
      {#each rows as row, i}
        <div
          style="padding:14px 16px;{i < rows.length - 1
            ? 'border-bottom:1px solid var(--border)'
            : ''};display:flex;align-items:center;gap:12px"
        >
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {row.first_name}
              {row.last_name}
              {#if !isAdmin}
                <span style="color:var(--text-tertiary);font-weight:400"
                  >· {$t("you")}</span
                >
              {/if}
            </div>
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {row.email}
            </div>
          </div>
          <label
            style="display:flex;align-items:center;gap:8px;font-size:12.5px"
          >
            <input
              type="checkbox"
              bind:checked={row.allow_reopen_without_approval}
              on:change={() => toggle(row)}
              disabled={saving[row.approver_id]}
            />
            {$t("Auto-approve reopens")}
          </label>
        </div>
      {/each}
      {#if rows.length === 0}
        <div style="padding:32px;text-align:center;color:var(--text-tertiary)">
          {$t("No data.")}
        </div>
      {/if}
    </div>
    <div style="font-size:11.5px;color:var(--text-tertiary);margin-top:12px">
      {$t("Allow employees to reopen weeks without approval")}
    </div>
  {/if}
</div>
