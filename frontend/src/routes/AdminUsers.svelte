<script>
  import { api, csrfToken } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import TempPasswordDialog from "../dialogs/TempPasswordDialog.svelte";
  import ArchiveUserDialog from "../dialogs/ArchiveUserDialog.svelte";
  import RestoreUserDialog from "../dialogs/RestoreUserDialog.svelte";
  import { getArchivedUsers } from "../lib/api/usersApi.js";
  import { confirmDialog } from "../confirm.js";

  let users = [];
  // Archived users are shown in a separate list below the active roster.
  // They are never mixed into the main list (no greyed-out rows).
  let archivedUsers = [];
  let showDialog = null;
  let resetPwData = null;
  // The user object selected for archiving — triggers ArchiveUserDialog.
  let archiveTarget = null;
  // The archived user object selected for restoring — triggers RestoreUserDialog.
  let restoreTarget = null;
  // Whether SMTP is configured — controls the warning shown in TempPasswordDialog.
  let smtpEnabled = false;

  async function load() {
    const loaded = await api("/users");
    users = (loaded || []).sort((a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name));
    try {
      archivedUsers = await getArchivedUsers();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
    // Load SMTP status once to show correct email hint after password reset.
    try {
      const settings = await api("/settings");
      smtpEnabled = !!settings.smtp_enabled;
    } catch {}
  }
  load();

  async function refreshCurrentUser() {
    const refreshedUser = await api("/auth/me");
    currentUser.set(refreshedUser);
    csrfToken.set(refreshedUser.csrf_token || null);
  }

  async function resetPw(userId) {
    if (
      !(await confirmDialog(
        $t("Reset password?"),
        $t("A temporary password will be generated."),
        { confirm: $t("Reset PW") },
      ))
    )
      return;
    try {
      const resetResponse = await api(`/users/${userId}/reset-password`, { method: "POST" });
      resetPwData = { password: resetResponse.temporary_password };
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function editUser(u) {
    try {
      showDialog = await api(`/users/${u.id}`);
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
    <div class="top-bar-subtitle">{$t("Manage your team")}</div>
  </div>
  <div class="top-bar-actions">
    <button
      class="zf-btn zf-btn-primary zf-btn-sm"
      on:click={() => (showDialog = {})}
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
        <div class="avatar" style="width:32px;height:32px;font-size:12px">
          {initials(u)}
        </div>
        <div style="flex:1;min-width:0">
          <div style="font-size:13px;font-weight:500">
            {u.first_name}
            {u.last_name}
          </div>
          <div style="font-size:11.5px;color:var(--text-tertiary)">
            {roleLabel(u.role)}
          </div>
        </div>
        <div style="display:flex;gap:4px">
          <button
            class="zf-btn zf-btn-ghost zf-btn-sm"
            on:click={() => editUser(u)}
          >
            <Icon name="Edit" size={13} />
          </button>
          <button
            class="zf-btn zf-btn-ghost zf-btn-sm"
            on:click={() => resetPw(u.id)}
          >
            <Icon name="Shield" size={13} />
          </button>
          <!-- Archive: data is preserved and restorable from the Archived list. -->
          <button
            class="zf-btn zf-btn-ghost zf-btn-sm zf-btn-danger"
            title={$t("Archive")}
            on:click={() => (archiveTarget = u)}
          >
            <Icon name="Archive" size={13} />
          </button>
        </div>
      </div>
    {/each}
  </div>

  {#if archivedUsers.length > 0}
    <!-- Archived users live below the active roster, never mixed in. -->
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
              {roleLabel(u.role)}
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

{#if resetPwData}
  <TempPasswordDialog
    password={resetPwData.password}
    {smtpEnabled}
    mode="reset"
    title={$t("Password reset.")}
    onDismiss={() => (resetPwData = null)}
  />
{/if}

{#if showDialog}
  <UserDialog
    template={showDialog}
    onClose={async (changed) => {
      const editedUserId = showDialog?.id;
      showDialog = null;
      if (changed) {
        if (editedUserId === $currentUser?.id) {
          try {
            await refreshCurrentUser();
          } catch (e) {
            toast($t(e?.message || "Error"), "error");
          }
        }
        load();
      }
    }}
  />
{/if}

{#if archiveTarget}
  <ArchiveUserDialog
    user={archiveTarget}
    onClose={(changed) => {
      archiveTarget = null;
      if (changed) load();
    }}
  />
{/if}

{#if restoreTarget}
  <RestoreUserDialog
    user={restoreTarget}
    onClose={(changed) => {
      restoreTarget = null;
      if (changed) load();
    }}
  />
{/if}
