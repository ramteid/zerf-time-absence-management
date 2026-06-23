<script>
  // Archived-users page reachable from /settings/archived-users.
  // Lists all archived accounts and lets admins restore them via a dialog.
  import { toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import RestoreUserDialog from "../dialogs/RestoreUserDialog.svelte";
  import { getArchivedUsers } from "../lib/api/usersApi.js";

  let archivedUsers = [];
  let restoreTarget = null;

  async function load() {
    try {
      archivedUsers = await getArchivedUsers();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  load();

  function initials(u) {
    return ((u.first_name?.[0] || "") + (u.last_name?.[0] || "")).toUpperCase();
  }

  function fmtDate(isoString) {
    if (!isoString) return "";
    try {
      return new Date(isoString).toLocaleDateString();
    } catch {
      return isoString;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Archived Users")}</h1>
  </div>
</div>

<div class="content-area" style="max-width:760px">
  {#if archivedUsers.length === 0}
    <div class="zf-card" style="padding:32px;text-align:center;color:var(--text-tertiary);font-size:13px">
      {$t("No archived users.")}
    </div>
  {:else}
    <div class="zf-card" style="overflow-x:auto">
      {#each archivedUsers as u, i (u.id)}
        <div
          style="padding:10px 16px;{i < archivedUsers.length - 1
            ? 'border-bottom:1px solid var(--border)'
            : ''};display:flex;align-items:center;gap:12px"
        >
          <div class="avatar" style="width:32px;height:32px;font-size:12px;opacity:0.5">
            {initials(u)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500;color:var(--text-secondary)">
              {u.first_name}
              {u.last_name}
            </div>
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {roleLabel(u.role)}
              · {$t("Archived on {date}", { date: fmtDate(u.archived_at) })}
            </div>
          </div>
          <div style="display:flex;gap:4px">
            <!-- Restore the archived account. -->
            <button
              class="zf-btn zf-btn-ghost zf-btn-sm"
              title={$t("Restore")}
              on:click={() => (restoreTarget = u)}
            >
              <Icon name="Check" size={13} />
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if restoreTarget}
  <RestoreUserDialog
    user={restoreTarget}
    onClose={(changed) => {
      restoreTarget = null;
      if (changed) load();
    }}
  />
{/if}
