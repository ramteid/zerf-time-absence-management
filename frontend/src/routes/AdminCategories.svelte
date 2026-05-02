<script>
  import { api } from "../api.js";
  import { categories, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import CategoryDialog from "../dialogs/CategoryDialog.svelte";

  let showDialog = null;

  async function load() {
    categories.set(await api("/categories"));
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Time Categories")}</h1>
  </div>
  <div class="top-bar-actions">
    <button class="kz-btn kz-btn-sm" on:click={() => (showDialog = {})}>
      <Icon name="Plus" size={13} />{$t("Add Category")}
    </button>
  </div>
</div>

<div class="content-area" style="max-width:600px">
  <div class="kz-card" style="overflow:hidden">
    {#each $categories as cat, i}
      <div
        style="padding:10px 16px;{i < $categories.length - 1
          ? 'border-bottom:1px solid var(--border)'
          : ''};display:flex;align-items:center;gap:10px"
      >
        <span
          class="cat-dot"
          style="width:10px;height:10px;background:{cat.color}"
        ></span>
        <span style="font-size:13px;font-weight:500;flex:1">{$t(cat.name)}</span
        >
        <button
          class="kz-btn kz-btn-ghost kz-btn-sm"
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
