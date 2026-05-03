<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";

  export let template;
  export let onClose;
  let dlg;
  $: isNew = !template.id;
  // Show translated name for system categories when editing
  let name = template.id ? $t(template.name) : template.name || "";
  let color = template.color || "#5b8def";
  let sort_order = template.sort_order || 0;
  let description = template.description || "";
  let error = "";

  onMount(() => dlg.showModal());

  async function save() {
    error = "";
    try {
      const body = {
        name,
        color,
        sort_order: Number(sort_order),
        description: description || null,
      };
      if (isNew) await api("/categories", { method: "POST", body });
      else await api("/categories/" + template.id, { method: "PUT", body });
      dlg.close();
      onClose(true);
    } catch (e) {
      error = e.message;
    }
  }

  function cancel() {
    dlg.close();
    onClose(false);
  }
</script>

<dialog bind:this={dlg}>
  <header>
    <span style="flex:1">{$t(isNew ? "Add Category" : "Edit Category")}</span>
    <button class="kz-btn-icon-sm kz-btn-ghost" on:click={cancel}>
      <Icon name="X" size={16} />
    </button>
  </header>
  <div class="dialog-body">
    <div>
      <label class="kz-label" for="cat-name">{$t("Name")}</label>
      <input id="cat-name" class="kz-input" bind:value={name} required />
    </div>
    <div>
      <label class="kz-label" for="cat-description">{$t("Description")}</label>
      <input id="cat-description" class="kz-input" bind:value={description} />
    </div>
    <div class="field-row">
      <div>
        <label class="kz-label" for="cat-color">{$t("Color")}</label>
        <input
          id="cat-color"
          class="kz-input"
          type="color"
          bind:value={color}
          style="height:36px;padding:4px"
        />
      </div>
      <div>
        <label class="kz-label" for="cat-order">{$t("Order")}</label>
        <input
          id="cat-order"
          class="kz-input"
          type="number"
          bind:value={sort_order}
        />
      </div>
    </div>
    <div class="error-text">{error}</div>
  </div>
  <footer>
    <button class="kz-btn" on:click={cancel}>{$t("Cancel")}</button>
    <button class="kz-btn kz-btn-primary" on:click={save}>{$t("Save")}</button>
  </footer>
</dialog>
