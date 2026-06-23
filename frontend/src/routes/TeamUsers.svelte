<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import { confirmDialog } from "../confirm.js";

  let users = [];
  let showDialog = null;

  async function load() {
    const loaded = await api("/team-users");
    users = (loaded || []).sort(
      (a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name),
    );
  }
  load();

  // Deactivated assistants drop out of `find_for_approver` server-side, so
  // there is no in-place reactivate toggle here — only deactivate.
  async function deactivate(u) {
    if (
      !(await confirmDialog($t("Deactivate?"), $t("Deactivate this user?"), {
        danger: true,
        confirm: $t("Deactivate"),
      }))
    )
      return;
    try {
      await api(`/team-users/${u.id}/deactivate`, { method: "POST" });
      toast($t("User deactivated."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function editUser(u) {
    try {
      showDialog = await api(`/team-users/${u.id}`);
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function deleteUser(u) {
    if (
      !(await confirmDialog(
        $t("Delete user?"),
        $t("Delete user permanently? All data of this user will be deleted. This cannot be undone."),
        { danger: true, confirm: $t("Delete permanently") },
      ))
    )
      return;
    try {
      await api(`/team-users/${u.id}`, { method: "DELETE" });
      toast($t("User deleted."), "ok");
      load();
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
              {#if !u.active}
                · <span style="color:var(--danger-text)">{$t("Inactive")}</span>
              {/if}
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
              title={$t("Deactivate")}
              on:click={() => deactivate(u)}
            >
              <Icon name="X" size={13} />
            </button>
            <button
              class="zf-btn zf-btn-ghost zf-btn-sm zf-btn-danger"
              title={$t("Delete permanently")}
              on:click={() => deleteUser(u)}
            >
              <Icon name="Trash" size={13} />
            </button>
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
