<script>
  // Dialog for archiving a user. If the target user currently approves active
  // team members, an approver-replacement section is shown so every affected
  // member gets a new approver before the archive is committed.
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import { archiveUser } from "../lib/api/usersApi.js";
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

  export let user;
  export let onClose;
  /** Optional custom API path for archive (e.g. /team-users/{id}/archive).
   *  When omitted the standard /users/{id}/archive path is used. */
  export let archiveApiPath = null;

  let dialog;
  let saving = false;
  let error = "";

  // Active users approved by the target — populated on mount.
  let approvedUsers = [];
  // All users eligible to become a replacement approver (excludes the target).
  let eligibleApprovers = [];
  // Map: approvedUser.id (number) -> chosen replacement approver id (number|null)
  let replacements = {};

  onMount(async () => {
    try {
      // Load the full user list to build eligible-approver and approved-users lists.
      const all = await api("/users");
      // Only team leads and admins can be approvers — employees are excluded.
      // Grouped by role, then name, to match every other user list in the app.
      eligibleApprovers = sortUsersByRoleThenName(
        (all || []).filter(
          (u) =>
            u.active &&
            u.id !== user.id &&
            (u.role === "team_lead" || u.role === "admin"),
        ),
      );
      // Find users whose approver_ids list contains the target user's id.
      // GET /users (admin path) includes approver_ids per user so this works directly.
      approvedUsers = sortUsersByRoleThenName(
        (all || []).filter(
          (u) =>
            u.active &&
            u.id !== user.id &&
            Array.isArray(u.approver_ids) &&
            u.approver_ids.includes(user.id),
        ),
      );
      // Initialise replacements with null so the UI shows them.
      replacements = Object.fromEntries(approvedUsers.map((u) => [u.id, null]));
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  });

  async function submit() {
    error = "";
    // Validate that every approved user has a replacement assigned.
    if (approvedUsers.some((u) => !replacements[u.id])) {
      error = $t("All users must have a replacement approver assigned.");
      return;
    }
    saving = true;
    try {
      // Convert replacements map to string-keyed object as required by the API.
      const approverReplacements = Object.fromEntries(
        Object.entries(replacements).map(([k, v]) => [String(k), v]),
      );
      if (archiveApiPath) {
        await api(archiveApiPath, {
          method: "POST",
          body: { approver_replacements: approverReplacements },
        });
      } else {
        await archiveUser(user.id, approverReplacements);
      }
      toast($t("User archived."), "ok");
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
  title={$t("Archive user?")}
  onClose={() => onClose(false)}
>
  <div class="text-note mb-12">
    <strong>{user.first_name} {user.last_name}</strong>
  </div>
  <p class="text-note">
    {$t(
      "This account will be deactivated and the user will no longer be able to log in. All data is preserved and the account can be restored later.",
    )}
  </p>

  {#if approvedUsers.length > 0}
    <div class="zf-info-box">
      <p class="zf-item-title text-warning mb-10">
        {$t(
          "This user approves {n} active user(s). Choose a replacement approver for each.",
          { n: approvedUsers.length },
        )}
      </p>
      {#each approvedUsers as member (member.id)}
        <div class="mb-10">
          <label class="zf-label" for="replacement-{member.id}">
            {$t("Replacement approver for {name}", {
              name: `${member.first_name} ${member.last_name}`,
            })}
          </label>
          <select
            id="replacement-{member.id}"
            class="zf-select"
            bind:value={replacements[member.id]}
          >
            <option value={null}>{$t("Select approver")}</option>
            {#each eligibleApprovers as approver (approver.id)}
              <option value={approver.id}>
                {approver.first_name}
                {approver.last_name}
              </option>
            {/each}
          </select>
        </div>
      {/each}
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
      class="zf-btn zf-btn-danger"
      type="button"
      disabled={saving}
      on:click={submit}
    >
      {saving ? $t("Saving...") : $t("Archive")}
    </button>
  </svelte:fragment>
</Dialog>
