<script>
  // Dialog for restoring an archived user. Lets admins optionally reset the
  // user's start date (to avoid a negative flextime gap from the archived
  // period) and assign approvers before reactivation.
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";
  import { restoreUser } from "../lib/api/usersApi.js";
  import { roleLabel } from "../i18n.js";

  export let user;
  export let onClose;

  let dialog;
  let saving = false;
  let error = "";

  // Whether the admin wants to reset the start date.
  let resetStartDate = false;
  // New start date when resetStartDate is true.
  let newStartDate = "";
  // Available approvers (active non-target users).
  let eligibleApprovers = [];
  // Approver IDs selected for this user (required when role != admin).
  let approverIds = [];

  $: isAdminRole = user?.role === "admin";
  $: requiresApprover = !isAdminRole;

  onMount(async () => {
    try {
      const all = await api("/users");
      eligibleApprovers = (all || []).filter((u) => u.active && u.id !== user.id);
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  });

  function toggleApprover(id) {
    if (approverIds.includes(id)) {
      approverIds = approverIds.filter((a) => a !== id);
    } else {
      approverIds = [...approverIds, id];
    }
  }

  async function submit() {
    error = "";
    if (requiresApprover && approverIds.length === 0) {
      error = $t("Approver required for non-admin users.");
      return;
    }
    if (resetStartDate && !newStartDate) {
      error = $t("Invalid date.");
      return;
    }
    saving = true;
    try {
      await restoreUser(
        user.id,
        resetStartDate ? newStartDate : null,
        approverIds,
      );
      toast($t("User restored."), "ok");
      dialog.close(true);
      onClose(true);
    } catch (e) {
      error = $t(e?.message || "Error");
    } finally {
      saving = false;
    }
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t("Restore user?")}
  onClose={() => onClose(false)}
>
  <div style="font-size:13px;color:var(--text-secondary);margin-bottom:12px">
    <strong>{user.first_name} {user.last_name}</strong>
    <span style="margin-left:6px;color:var(--text-tertiary)">·</span>
    <span style="margin-left:6px;color:var(--text-tertiary)">{roleLabel(user.role)}</span>
  </div>
  <p style="font-size:13px;color:var(--text-secondary)">
    {$t(
      "Restore this archived account? The user will receive a temporary password and must change it on first login.",
    )}
  </p>

  <!-- Start date reset section -->
  <div style="margin-top:14px;padding:12px;background:var(--bg-subtle,var(--bg-surface));border:1px solid var(--border);border-radius:6px">
    <p style="font-size:12px;color:var(--text-tertiary);margin-bottom:8px">
      {$t(
        "If the account was archived for an extended period, resetting the start date prevents a large negative flextime balance from accumulating during the absence.",
      )}
    </p>
    <div style="display:flex;flex-direction:column;gap:6px">
      <label style="font-size:13px;display:flex;align-items:center;gap:8px;cursor:pointer">
        <input
          type="radio"
          name="start-date-mode"
          value={false}
          bind:group={resetStartDate}
        />
        {$t("Keep original start date")}
      </label>
      <label style="font-size:13px;display:flex;align-items:center;gap:8px;cursor:pointer">
        <input
          type="radio"
          name="start-date-mode"
          value={true}
          bind:group={resetStartDate}
        />
        {$t("Reset start date to avoid flextime gap")}
      </label>
    </div>
    {#if resetStartDate}
      <div style="margin-top:10px">
        <label class="zf-label" for="restore-start-date">
          {$t("New start date (optional)")}
        </label>
        <DatePicker
          id="restore-start-date"
          bind:value={newStartDate}
          placeholder="YYYY-MM-DD"
        />
      </div>
    {/if}
  </div>

  <!-- Approver assignment (required for non-admin users) -->
  {#if requiresApprover}
    <div style="margin-top:14px">
      <span class="zf-label">
        {$t("Approver")}
        {#if approverIds.length === 0}
          <span style="color:var(--danger-text)"> *</span>
        {/if}
      </span>
      <div
        style="border:1px solid var(--border);border-radius:6px;max-height:180px;overflow-y:auto"
      >
        {#each eligibleApprovers as approver (approver.id)}
          <label
            style="display:flex;align-items:center;gap:8px;padding:7px 10px;font-size:13px;cursor:pointer;border-bottom:1px solid var(--border)"
          >
            <input
              type="checkbox"
              checked={approverIds.includes(approver.id)}
              on:change={() => toggleApprover(approver.id)}
            />
            {approver.first_name}
            {approver.last_name}
          </label>
        {/each}
        {#if eligibleApprovers.length === 0}
          <div style="padding:10px;font-size:13px;color:var(--text-tertiary)">
            {$t("No active users available.")}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  {#if error}
    <p style="font-size:12px;color:var(--danger-text);margin-top:8px">{error}</p>
  {/if}

  <svelte:fragment slot="footer">
    <button class="zf-btn" type="button" on:click={() => { dialog.close(); onClose(false); }}>
      {$t("Cancel")}
    </button>
    <button
      class="zf-btn zf-btn-primary"
      type="button"
      disabled={saving}
      on:click={submit}
    >
      {saving ? $t("Saving...") : $t("Restore")}
    </button>
  </svelte:fragment>
</Dialog>
