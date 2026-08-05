<script>
  import { api, csrfToken } from "../api.js";
  import { currentUser, settings as appSettings, toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import TempPasswordDialog from "../dialogs/TempPasswordDialog.svelte";
  import ArchiveUserDialog from "../dialogs/ArchiveUserDialog.svelte";
  import RestoreUserDialog from "../dialogs/RestoreUserDialog.svelte";
  import { getArchivedUsers } from "../lib/api/usersApi.js";
  import {
    sortUsersByRoleThenName,
    userAvatarClass,
    userInitials,
  } from "../lib/domain/users.js";
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
  // The allow_team_lead_manage_assistants setting, shown above the user list.
  let allowTeamLeadManageAssistants = false;
  let savingAssistantSetting = false;

  async function load() {
    const loaded = await api("/users");
    users = sortUsersByRoleThenName(loaded);
    try {
      archivedUsers = sortUsersByRoleThenName(await getArchivedUsers());
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
    // Load settings once to populate the toggle and SMTP status.
    try {
      const loadedSettings = await api("/settings");
      smtpEnabled = !!loadedSettings.smtp_enabled;
      allowTeamLeadManageAssistants =
        !!loadedSettings.allow_team_lead_manage_assistants;
    } catch {}
  }
  load();

  async function saveAssistantSetting() {
    savingAssistantSetting = true;
    try {
      // Load the full current settings first so we only change this one field.
      const current = await api("/settings");
      const body = {
        ...current,
        allow_team_lead_manage_assistants: allowTeamLeadManageAssistants,
      };
      const saved = await api("/settings", { method: "PUT", body });
      allowTeamLeadManageAssistants = !!saved.allow_team_lead_manage_assistants;
      // Sync the global settings store so other views see the updated value.
      appSettings.set(saved);
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    } finally {
      savingAssistantSetting = false;
    }
  }

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
      const resetResponse = await api(`/users/${userId}/reset-password`, {
        method: "POST",
      });
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

<div class="content-area page-medium">
  <!-- Team leads setting shown above the user list so admins can toggle it inline. -->
  <div class="zf-card zf-card-section">
    <div class="field-row">
      <div class="flex-none">
        <label class="zf-label zf-check-label">
          <input type="checkbox" bind:checked={allowTeamLeadManageAssistants} />
          {$t("Allow team leads to create assistant users")}
        </label>
        <div class="field-hint">
          {$t(
            'When enabled, team leads get a restricted Users tab where they may only create and manage "Assistant" users assigned to them. No other role can be created there. Disabled by default.',
          )}
        </div>
      </div>
    </div>
    <div class="form-actions">
      <button
        class="zf-btn zf-btn-primary zf-btn-sm"
        on:click={saveAssistantSetting}
        disabled={savingAssistantSetting}
      >
        {savingAssistantSetting ? $t("Saving...") : $t("Save Changes")}
      </button>
    </div>
  </div>

  <div class="zf-card zf-table-wrap" data-testid="user-roster">
    {#each users as u (u.id)}
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
            {roleLabel(u.role)}
          </div>
        </div>
        <div class="zf-actions">
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
              {roleLabel(u.role)}
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

  .form-actions {
    display: flex;
    justify-content: flex-end;
    padding-top: 12px;
  }
</style>
