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

  // Mirrors AdminUsers.svelte's toggleActive: a single PUT flips the active
  // flag both ways. Unlike the admin Users tab, there is no delete action
  // here at all — team leads may deactivate/reactivate an assistant but
  // never delete one.
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
      await api(`/team-users/${u.id}`, { method: "PUT", body: { active: !u.active } });
      toast($t(u.active ? "User deactivated." : "User activated."), "ok");
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
              class="zf-btn zf-btn-ghost zf-btn-sm"
              class:zf-btn-danger={u.active}
              title={u.active ? $t("Deactivate") : $t("Activate")}
              on:click={() => toggleActive(u)}
            >
              <Icon name={u.active ? "X" : "Check"} size={13} />
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
