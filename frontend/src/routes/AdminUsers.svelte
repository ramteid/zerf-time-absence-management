<script>
  import { api, csrfToken } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import TempPasswordDialog from "../dialogs/TempPasswordDialog.svelte";
  import ArchiveUserDialog from "../dialogs/ArchiveUserDialog.svelte";
  import { confirmDialog } from "../confirm.js";

  let users = [];
  let showDialog = null;
  let resetPwData = null;
  // The user object selected for archiving — triggers ArchiveUserDialog.
  let archiveTarget = null;

  async function load() {
    const loaded = await api("/users");
    users = (loaded || []).sort((a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name));
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

  async function toggleActive(u) {
    if (u.active) {
      if (
        !(await confirmDialog($t("Deactivate?"), $t("Deactivate this user?"), {
          danger: true,
          confirm: $t("Deactivate"),
        }))
      )
        return;
    }
    try {
      await api(`/users/${u.id}`, { method: "PUT", body: { active: !u.active } });
      toast($t(u.active ? "User deactivated." : "User activated."), "ok");
      load();
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
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Team Members")}</h1>
    <div class="top-bar-subtitle">{$t("Manage your team")}</div>
  </div>
  <div class="top-bar-actions">
    <button
      class="zf-btn zf-btn-primary zf-btn-sm"
      on:click={() => (showDialog = {})}
    >
      <Icon name="Plus" size={13} />{$t("Add Member")}
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
            {#if !u.active}
              · <span style="color:var(--danger-text)">{$t("Inactive")}</span>
            {/if}
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
          <button
            class="zf-btn zf-btn-ghost zf-btn-sm"
            class:zf-btn-danger={u.active}
            title={u.active ? $t("Deactivate") : $t("Activate")}
            on:click={() => toggleActive(u)}
          >
            <Icon name={u.active ? "X" : "Check"} size={13} />
          </button>
          <!-- Archive replaces hard-delete: data is preserved and restorable. -->
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
</div>

{#if resetPwData}
  <TempPasswordDialog
    password={resetPwData.password}
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
