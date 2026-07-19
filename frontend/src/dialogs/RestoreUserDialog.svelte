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
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

  export let user;
  export let onClose;
  /** Optional custom API path for restore (e.g. /team-users/{id}/restore).
   *  When omitted the standard /users/{id}/restore path is used. */
  export let restoreApiPath = null;

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
      // Only team leads and admins can be approvers (same rule the backend
      // enforces and that UserDialog/ArchiveUserDialog apply); grouped by role
      // then name to match every other user list in the app.
      eligibleApprovers = sortUsersByRoleThenName(
        (all || []).filter(
          (u) =>
            u.active &&
            u.id !== user.id &&
            (u.role === "team_lead" || u.role === "admin"),
        ),
      );
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
    if (requiresApprover && !restoreApiPath && approverIds.length === 0) {
      error = $t("Approver required for non-admin users.");
      return;
    }
    if (resetStartDate && !newStartDate) {
      error = $t("Invalid date.");
      return;
    }
    saving = true;
    try {
      if (restoreApiPath) {
        await api(restoreApiPath, {
          method: "POST",
          body: {
            start_date: resetStartDate ? newStartDate : null,
          },
        });
      } else {
        await restoreUser(
          user.id,
          resetStartDate ? newStartDate : null,
          approverIds,
        );
      }
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
  <div class="text-note mb-12">
    <strong>{user.first_name} {user.last_name}</strong>
    <span class="dot-sep">·</span>
    <span class="dot-sep">{roleLabel(user.role)}</span>
  </div>
  <p class="text-note">
    {$t(
      "Restore this archived account? The user will receive a temporary password and must change it on first login.",
    )}
  </p>

  <!-- Start date reset section -->
  <div class="zf-info-box">
    <p class="text-hint mb-8">
      {$t(
        "If the account was archived for an extended period, resetting the start date prevents a large negative flextime balance from accumulating during the absence.",
      )}
    </p>
    <div class="choice-list">
      <label class="zf-check-label">
        <input
          type="radio"
          name="start-date-mode"
          value={false}
          bind:group={resetStartDate}
        />
        {$t("Keep original start date")}
      </label>
      <label class="zf-check-label">
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
      <div class="mt-10">
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

  <!-- Approver assignment (required for non-admin users when using the admin path).
       Hidden when using a custom path (e.g. team-lead restore) since the lead
       is already the approver and no reassignment is needed. -->
  {#if requiresApprover && !restoreApiPath}
    <div class="mt-14">
      <span class="zf-label">
        {$t("Approver")}
        {#if approverIds.length === 0}
          <span class="text-danger"> *</span>
        {/if}
      </span>
      <div class="user-list">
        {#each eligibleApprovers as approver (approver.id)}
          <label class="user-option">
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
          <div class="user-empty">
            {$t("No active users available.")}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  {#if error}
    <p class="error-text mt-8">
      {error}
    </p>
  {/if}

  <svelte:fragment slot="footer">
    <button
      class="zf-btn"
      type="button"
      on:click={() => {
        dialog.close();
        onClose(false);
      }}
    >
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

<style>
  /* "·" separator between name, e-mail and role in the intro line. */
  .dot-sep {
    margin-left: 6px;
    color: var(--text-tertiary);
  }

  .choice-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  /* Scrollable pick list of users to restore. */
  .user-list {
    border: 1px solid var(--border);
    border-radius: 6px;
    max-height: 180px;
    overflow-y: auto;
  }

  .user-option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    font-size: 0.875rem;
    cursor: pointer;
    border-bottom: 1px solid var(--border);
  }

  .user-empty {
    padding: 10px;
    font-size: 0.875rem;
    color: var(--text-tertiary);
  }
</style>
