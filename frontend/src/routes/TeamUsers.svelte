<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import ArchiveUserDialog from "../dialogs/ArchiveUserDialog.svelte";
  import RestoreUserDialog from "../dialogs/RestoreUserDialog.svelte";

  let users = [];
  let showDialog = null;
  // The assistant selected for archive or restore — triggers the respective dialog.
  let archiveTarget = null;
  let restoreTarget = null;

  async function load() {
    const loaded = await api("/team-users");
    users = (loaded || []).sort(
      (a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name),
    );
  }
  load();

  async function editUser(u) {
    try {
      showDialog = await api(`/team-users/${u.id}`);
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  function initials(u) {
    return ((u.first_name?.[0] || "") + (u.last_name?.[0] || "")).toUpperCase();
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Team Members")}</h1>
    <div class="top-bar-subtitle">{$t("You can only manage assistants assigned to you.")}</div>
  </div>
  <div class="top-bar-actions">
    <button
      class="zf-btn zf-btn-primary zf-btn-sm"
      on:click={() => (showDialog = { role: "assistant" })}
    >
      <Icon name="Plus" size={13} />{$t("Add Member")}
    </button>
  </div>
</div>

<div class="content-area" style="max-width:760px">
  <div class="zf-card" style="overflow-x:auto">
    {#each users as u (u.id)}
      <div style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:12px">
        <div class="avatar" style="width:32px;height:32px;font-size:12px;opacity:{u.can_manage ? (u.archived_at ? 0.5 : 1) : 0.5}">
          {initials(u)}
        </div>
        <div style="flex:1;min-width:0;opacity:{u.can_manage ? (u.archived_at ? 0.5 : 1) : 0.5}">
          <div style="font-size:13px;font-weight:500">
            {u.first_name}
            {u.last_name}
          </div>
          {#if u.can_manage}
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {$t("Assistant")}
              {#if u.archived_at}
                · <span style="color:var(--danger-text)">{$t("Archived on {date}", { date: new Date(u.archived_at).toLocaleDateString() })}</span>
              {/if}
            </div>
          {/if}
        </div>
        {#if u.can_manage}
          <div style="display:flex;gap:4px">
            {#if !u.archived_at}
              <button class="zf-btn zf-btn-ghost zf-btn-sm" on:click={() => editUser(u)}>
                <Icon name="Edit" size={13} />
              </button>
              <button
                class="zf-btn zf-btn-ghost zf-btn-sm zf-btn-danger"
                title={$t("Archive")}
                on:click={() => (archiveTarget = u)}
              >
                <Icon name="Archive" size={13} />
              </button>
            {:else}
              <button
                class="zf-btn zf-btn-ghost zf-btn-sm"
                title={$t("Restore")}
                on:click={() => (restoreTarget = u)}
              >
                <Icon name="Check" size={13} />
              </button>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>

{#if showDialog}
  <UserDialog
    template={showDialog}
    lockedRole="assistant"
    apiBase="/team-users"
    onClose={(changed) => {
      showDialog = null;
      if (changed) load();
    }}
  />
{/if}

{#if archiveTarget}
  <ArchiveUserDialog
    user={archiveTarget}
    archiveApiPath={`/team-users/${archiveTarget.id}/archive`}
    onClose={(changed) => {
      archiveTarget = null;
      if (changed) load();
    }}
  />
{/if}

{#if restoreTarget}
  <RestoreUserDialog
    user={restoreTarget}
    restoreApiPath={`/team-users/${restoreTarget.id}/restore`}
    onClose={(changed) => {
      restoreTarget = null;
      if (changed) load();
    }}
  />
{/if}
