<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import {
    compareUsersByName,
    userAvatarClass,
    userInitials,
  } from "../lib/domain/users.js";
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
    // Role grouping is deliberately NOT applied here: the /team-users endpoint
    // omits `role` for non-manageable colleagues (only manageable assistants
    // carry it), so a role sort would float assistants above everyone else
    // instead of grouping. A plain alphabetical order is the honest choice when
    // roles are unavailable.
    const sorted = [...(loaded || [])].sort(compareUsersByName);
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

  function fmtDate(isoString) {
    if (!isoString) return "";
    try {
      return new Date(isoString).toLocaleDateString();
    } catch {
      return isoString;
    }
  }
</script>

<div class="top-bar page-medium">
  <div class="top-bar-title">
    <h1>{$t("Users")}</h1>
    <div class="top-bar-subtitle">
      {$t("You can only manage assistants assigned to you.")}
    </div>
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

<div class="content-area page-medium">
  <div class="zf-card zf-table-wrap">
    {#each users as u (u.id)}
      <div class="user-row">
        <div
          class="avatar avatar-sm {userAvatarClass(u)}"
          class:dimmed={!u.can_manage}
        >
          {userInitials(u)}
        </div>
        <div class="flex-min0" class:dimmed={!u.can_manage}>
          <div class="zf-item-title">
            {u.first_name}
            {u.last_name}
          </div>
          {#if u.can_manage}
            <div class="text-hint">
              {$t("Assistant")}
            </div>
          {/if}
        </div>
        {#if u.can_manage}
          <div class="zf-actions">
            <button
              class="zf-btn zf-btn-ghost zf-btn-sm"
              on:click={() => editUser(u)}
            >
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
    <h2 class="zf-list-heading">
      {$t("Archived Users")}
    </h2>
    <div class="zf-card zf-table-wrap">
      {#each archivedUsers as u (u.id)}
        <div class="user-row">
          <div class="avatar {userAvatarClass(u)} avatar-sm">
            {userInitials(u)}
          </div>
          <div class="flex-min0">
            <div class="zf-item-title">
              {u.first_name}
              {u.last_name}
            </div>
            <div class="text-hint">
              {$t("Assistant")}
              · {$t("Archived on {date}", { date: fmtDate(u.archived_at) })}
            </div>
          </div>
          <div class="zf-actions">
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

<style>
  .user-row {
    padding: 10px 16px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .user-row:not(:last-child) {
    border-bottom: 1px solid var(--border);
  }

  /* Members this team lead cannot manage stay listed but visually recede. */
  .dimmed {
    opacity: 0.5;
  }
</style>
