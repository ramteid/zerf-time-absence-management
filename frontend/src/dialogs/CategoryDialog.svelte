<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";

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
    if (isNew) return;
    try {
      [allUsers, enabledUserIds] = await Promise.all([
        api("/users"),
        api("/categories/" + template.id + "/users"),
      ]);
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
      if (isNew) await api("/categories", { method: "POST", body });
      else await api("/categories/" + template.id, { method: "PUT", body });
      if (!isNew) {
        await api("/categories/" + template.id + "/users", {
          method: "PUT",
          body: { user_ids: enabledUserIds },
        });
      }
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
        class="zf-input"
        type="color"
        bind:value={color}
        style="height:36px;padding:4px"
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
  <label
    style="display:flex;align-items:center;gap:8px;font-size:13px;margin-top:8px"
  >
    <input type="checkbox" bind:checked={counts_as_work} />
    <span>{$t("Counts as work")}</span>
  </label>
  {#if !isNew}
    <label
      style="display:flex;align-items:center;gap:8px;font-size:13px;margin-top:8px"
    >
      <input type="checkbox" bind:checked={active} />
      <span>{$t("Active")}</span>
    </label>
    {#if allUsers.length > 0}
      <div style="margin-top:12px">
        <div class="zf-label">{$t("Available to employees")}</div>
        <div
          style="max-height:200px;overflow-y:auto;border:1px solid var(--border);border-radius:var(--radius-sm)"
        >
          <table style="width:100%;border-collapse:collapse;font-size:13px">
            <tbody>
              {#each allUsers as employee (employee.id)}
                <tr style="border-bottom:1px solid var(--border)">
                  <td style="padding:6px 8px">
                    {employee.first_name} {employee.last_name}
                  </td>
                  <td style="padding:6px 8px;text-align:right;width:32px">
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
  {/if}
  <div class="error-text">{error}</div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={() => dialog.close()}>{$t("Cancel")}</button>
    <button class="zf-btn zf-btn-primary" on:click={save}>{$t("Save")}</button>
  </svelte:fragment>
</Dialog>
