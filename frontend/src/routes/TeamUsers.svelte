<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import ArchiveUserDialog from "../dialogs/ArchiveUserDialog.svelte";
  import RestoreUserDialog from "../dialogs/RestoreUserDialog.svelte";

  // Active (non-archived) entries shown in the main list.
  let users = [];
  // Archived assistants the lead manages — shown in a separate list below.
  // A lead only ever sees assistants assigned (or formerly assigned) to them,
  // never anyone else. The backend enforces that scope.
  let archivedUsers = [];
  let showDialog = null;
  let archiveTarget = null;
  let restoreTarget = null;

  async function load() {
    const loaded = await api("/team-users");
    const sorted = (loaded || []).sort(
      (a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name),
    );
    // Split active rows from archived. Only manageable assistants ever carry
    // an archived_at; non-manageable colleagues are always active.
    users = sorted.filter((u) => !u.archived_at);
    archivedUsers = sorted.filter((u) => !!u.archived_at);
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
    <h1>{$t("Users")}</h1>
    <div class="top-bar-subtitle">{$t("You can only manage assistants assigned to you.")}</div>
  </div>
  <div class="top-bar-actions">
    <button
      class="zf-btn zf-btn-primary zf-btn-sm"
      on:click={() => (showDialog = { role: "assistant" })}
    >
      <Icon name="Plus" size={13} />{$t("Add User")}
    </button>
  </div>
</div>

<div class="content-area" style="max-width:760px">
  <div class="zf-card" style="overflow-x:auto">
    {#each users as u, i (u.id)}
      <div
        style="padding:10px 16px;{i < users.length - 1
          ? 'border-bottom:1px solid var(--border)'
          : ''};display:flex;align-items:center;gap:12px"
      >
        <div class="avatar" style="width:32px;height:32px;font-size:12px;opacity:{u.can_manage ? 1 : 0.5}">
          {initials(u)}
        </div>
        <div style="flex:1;min-width:0;opacity:{u.can_manage ? 1 : 0.5}">
          <div style="font-size:13px;font-weight:500">
            {u.first_name}
            {u.last_name}
          </div>
          {#if u.can_manage}
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {$t("Assistant")}
            </div>
          {/if}
        </div>
        {#if u.can_manage}
          <div style="display:flex;gap:4px">
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
          </div>
        {/if}
      </div>
    {/each}
  </div>

  {#if archivedUsers.length > 0}
    <h2 style="margin:24px 0 8px;font-size:14px;font-weight:600;color:var(--text-secondary)">
      {$t("Archived Users")}
    </h2>
    <div class="zf-card" style="overflow-x:auto">
      {#each archivedUsers as u, i (u.id)}
        <div
          style="padding:10px 16px;{i < archivedUsers.length - 1
            ? 'border-bottom:1px solid var(--border)'
            : ''};display:flex;align-items:center;gap:12px"
        >
          <div class="avatar" style="width:32px;height:32px;font-size:12px">
            {initials(u)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {u.first_name}
              {u.last_name}
            </div>
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {$t("Assistant")}
              · {$t("Archived on {date}", { date: fmtDate(u.archived_at) })}
            </div>
          </div>
          <div style="display:flex;gap:4px">
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
