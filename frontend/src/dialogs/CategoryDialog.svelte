<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

  export let template;
  export let onClose;
  let dialog;
  $: isNew = !template.id;
  let canonicalName = template.name || "";
  let name = template.id ? $t(canonicalName) : canonicalName;
  let nameChanged = false;
  let color = template.color || "#5b8def";
  let sort_order = template.sort_order || 0;
  let description = template.description || "";
  let counts_as_work = template.counts_as_work ?? true;
  let active = template.active ?? true;
  let error = "";
  let saving = false;

  let allUsers = [];
  let enabledUserIds = [];
  // Set once a new category's initial POST succeeds. If the follow-up
  // users-PUT then fails, retrying save() must reuse this id instead of
  // POSTing again — the category's name is unique, so a second POST would
  // fail with a confusing "already exists" error for a category that in
  // fact was already created by the first attempt.
  let createdId = null;

  onMount(async () => {
    try {
      const [loadedUsers, loadedEnabled] = await Promise.all([
        api("/users"),
        isNew
          ? Promise.resolve(null)
          : api("/categories/" + template.id + "/users"),
      ]);
      allUsers = sortUsersByRoleThenName(loadedUsers);
      // New categories default to visible for everyone, matching the
      // backend's default when no explicit user list is saved yet.
      enabledUserIds = isNew ? allUsers.map((u) => u.id) : loadedEnabled;
    } catch {
      allUsers = [];
      enabledUserIds = [];
    }
  });

  async function save() {
    if (saving) return;
    error = "";
    saving = true;
    try {
      const body = {
        name: !isNew && !nameChanged ? canonicalName : name,
        color,
        sort_order: Number(sort_order),
        description: description || null,
        counts_as_work,
      };
      if (!isNew) {
        body.active = active;
      }
      // categoryId is falsy only for a category that has never been
      // POSTed yet. It stays set across a failed retry (via createdId) so
      // a second Save always PUTs the existing row instead of POSTing a
      // duplicate — and that PUT also (re-)persists any field edits made
      // since the first attempt, rather than silently dropping them.
      let categoryId = isNew ? createdId : template.id;
      if (!categoryId) {
        const created = await api("/categories", { method: "POST", body });
        categoryId = created.id;
        createdId = categoryId;
      } else {
        await api("/categories/" + categoryId, { method: "PUT", body });
      }
      // New categories already default to enabled for everyone on the
      // backend (see repository::categories::create); only push an explicit
      // list if the admin actually narrowed it down. This also means a
      // failed or not-yet-finished /users fetch can't wipe the default down
      // to "nobody" via an empty enabledUserIds.
      if (!isNew || enabledUserIds.length < allUsers.length) {
        await api("/categories/" + categoryId + "/users", {
          method: "PUT",
          body: { user_ids: enabledUserIds },
        });
      }
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
  title={$t(isNew ? "Add Category" : "Edit Category")}
  onClose={() => onClose(false)}
>
  <div>
    <label class="zf-label" for="cat-name">{$t("Name")}</label>
    <input
      id="cat-name"
      class="zf-input"
      bind:value={name}
      on:input={() => (nameChanged = true)}
      required
    />
  </div>
  <div>
    <label class="zf-label" for="cat-description">{$t("Description")}</label>
    <input id="cat-description" class="zf-input" bind:value={description} />
  </div>
  <div class="field-row">
    <div>
      <label class="zf-label" for="cat-color">{$t("Color")}</label>
      <input
        id="cat-color"
        class="zf-input zf-color-input"
        type="color"
        bind:value={color}
      />
    </div>
    <div>
      <label class="zf-label" for="cat-order">{$t("Order")}</label>
      <input
        id="cat-order"
        class="zf-input"
        type="number"
        bind:value={sort_order}
      />
    </div>
  </div>
  <label class="zf-check-label mt-8">
    <input type="checkbox" bind:checked={counts_as_work} />
    <span>{$t("Counts as work")}</span>
  </label>
  {#if !isNew}
    <label class="zf-check-label mt-8">
      <input type="checkbox" bind:checked={active} />
      <span>{$t("Active")}</span>
    </label>
  {/if}
  {#if allUsers.length > 0}
    <div class="mt-12">
      <div class="zf-label">{$t("Available to employees")}</div>
      <div class="zf-scroll-box">
        <table class="zf-table">
          <tbody>
            {#each allUsers as employee (employee.id)}
              <tr class="zf-divider-row">
                <td class="zf-td-compact">
                  {employee.first_name}
                  {employee.last_name}
                </td>
                <td class="zf-td-action">
                  <input
                    type="checkbox"
                    value={employee.id}
                    bind:group={enabledUserIds}
                  />
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}
  <div class="error-text">{error}</div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={() => dialog.close()}
      >{$t("Cancel")}</button
    >
    <button class="zf-btn zf-btn-primary" disabled={saving} on:click={save}>
      {saving ? $t("Saving...") : $t("Save")}
    </button>
  </svelte:fragment>
</Dialog>
