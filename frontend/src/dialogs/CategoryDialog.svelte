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

  let allUsers = [];
  let enabledUserIds = [];

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
    error = "";
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
      let categoryId = template.id;
      if (isNew) {
        const created = await api("/categories", { method: "POST", body });
        categoryId = created.id;
      } else {
        await api("/categories/" + template.id, { method: "PUT", body });
      }
      await api("/categories/" + categoryId + "/users", {
        method: "PUT",
        body: { user_ids: enabledUserIds },
      });
      dialog.close(true);
      onClose(true);
    } catch (e) {
      error = $t(e?.message || "Error");
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
    <button class="zf-btn zf-btn-primary" on:click={save}>{$t("Save")}</button>
  </svelte:fragment>
</Dialog>
