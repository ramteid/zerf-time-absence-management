<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import UserDialog from "../dialogs/UserDialog.svelte";
  import { confirmDialog } from "../confirm.js";

  let users = [];
  let showDialog = null;

  async function load() {
    users = await api("/users");
  }
  load();

  async function resetPw(id) {
    if (
      !(await confirmDialog(
        $t("Reset password?"),
        $t("A temporary password will be generated."),
        { confirm: $t("Reset PW") },
      ))
    )
      return;
    try {
      const r = await api(`/users/${id}/reset-password`, { method: "POST" });
      toast(
        $t("Temporary password: {password}", {
          password: r.temporary_password,
        }),
        "info",
      );
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function deactivate(id) {
    if (
      !(await confirmDialog($t("Deactivate?"), $t("Deactivate this user?"), {
        danger: true,
        confirm: $t("Deactivate"),
      }))
    )
      return;
    try {
      await api(`/users/${id}/deactivate`, { method: "POST" });
      toast($t("User deactivated."), "ok");
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
  </div>
  <div class="top-bar-subtitle">{$t("Manage your team")}</div>
  <div class="top-bar-actions">
    <button
      class="kz-btn kz-btn-primary kz-btn-sm"
      on:click={() => (showDialog = {})}
    >
      <Icon name="Plus" size={13} />{$t("Add Member")}
    </button>
  </div>
</div>

<div class="content-area" style="max-width:760px">
  <div class="kz-card" style="overflow-x:auto">
    {#each users as u, i}
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
            class="kz-btn kz-btn-ghost kz-btn-sm"
            on:click={() => (showDialog = u)}
          >
            <Icon name="Edit" size={13} />
          </button>
          <button
            class="kz-btn kz-btn-ghost kz-btn-sm"
            on:click={() => resetPw(u.id)}
          >
            <Icon name="Shield" size={13} />
          </button>
          {#if u.active}
            <button
              class="kz-btn kz-btn-ghost kz-btn-sm kz-btn-danger"
              on:click={() => deactivate(u.id)}
            >
              <Icon name="X" size={13} />
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
</div>

{#if showDialog}
  <UserDialog
    template={showDialog}
    onClose={(changed) => {
      showDialog = null;
      if (changed) load();
    }}
  />
{/if}
