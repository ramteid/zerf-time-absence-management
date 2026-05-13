<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import CategoryDialog from "../dialogs/CategoryDialog.svelte";

  let showDialog = null;
  let adminCategories = [];

  async function load() {
    adminCategories = await api("/categories/all");
  }
  load();
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Time Categories")}</h1>
  </div>
  <div class="top-bar-actions">
    <button class="zf-btn zf-btn-sm" on:click={() => (showDialog = {})}>
      <Icon name="Plus" size={13} />{$t("Add Category")}
    </button>
  </div>
</div>

<div class="content-area" style="max-width:600px">
  <div class="zf-card" style="overflow-x:auto">
    {#each adminCategories as cat, i}
      <div
        style="padding:10px 16px;{i < adminCategories.length - 1
          ? 'border-bottom:1px solid var(--border)'
          : ''};display:flex;align-items:center;gap:10px;opacity:{cat.active
          ? 1
          : 0.55}"
      >
        <span
          class="cat-dot"
          style="width:10px;height:10px;background:{cat.color}"
        ></span>
        <span style="font-size:13px;font-weight:500;flex:1">{$t(cat.name)}</span
        >
        {#if !cat.active}
          <span class="zf-chip">{$t("Inactive")}</span>
        {/if}
        <button
          class="zf-btn zf-btn-ghost zf-btn-sm"
          on:click={() => (showDialog = cat)}
        >
          <Icon name="Edit" size={13} />
        </button>
      </div>
    {/each}
  </div>
</div>

{#if showDialog}
  <CategoryDialog
    template={showDialog}
    onClose={(changed) => {
      showDialog = null;
      if (changed) load();
    }}
  />
{/if}
