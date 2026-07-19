<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import CategoryDialog from "../dialogs/CategoryDialog.svelte";
  import AbsenceCategoryDialog from "../dialogs/AbsenceCategoryDialog.svelte";

  let showDialog = null;
  let showAbsenceDialog = null;
  let adminCategories = [];
  let adminAbsenceCategories = [];

  async function load() {
    adminCategories = await api("/categories/all");
  }
  async function loadAbsence() {
    adminAbsenceCategories = await api("/absence-categories/all");
  }
  load();
  loadAbsence();
</script>

<div class="top-bar page-narrow">
  <div class="top-bar-title">
    <h1>{$t("Categories")}</h1>
  </div>
</div>

<div class="content-area page-narrow">
  <div class="section-head">
    <h2 class="section-title">
      {$t("Time Categories")}
    </h2>
    <button class="zf-btn zf-btn-sm" on:click={() => (showDialog = {})}>
      <Icon name="Plus" size={13} />{$t("Add")}
    </button>
  </div>
  <!--
    Match the absence-category card layout: drop overflow-x:auto and let
    the name truncate with an ellipsis so the row stays inside the mobile
    viewport even when the category name is very long.
  -->
  <div class="zf-card mb-24">
    {#each adminCategories as cat (cat.id)}
      <div class="cat-row" class:inactive={!cat.active}>
        <span class="cat-dot" style:background={cat.color}></span>
        <span class="cat-name">{$t(cat.name)}</span>
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

  <div class="section-head">
    <h2 class="section-title">
      {$t("Absence Categories")}
    </h2>
    <button class="zf-btn zf-btn-sm" on:click={() => (showAbsenceDialog = {})}>
      <Icon name="Plus" size={13} />{$t("Add")}
    </button>
  </div>
  <!--
    Each absence category carries up to four behavior flags that, when
    rendered as inline chips, push the row past mobile viewport width.
    They are intentionally NOT surfaced in this overview — the edit dialog
    shows them in full. The list keeps the row narrow with just the color
    swatch, name, optional "Inactive" status badge, and edit button so it
    fits on a phone screen without horizontal scrolling.
  -->
  <div class="zf-card">
    {#each adminAbsenceCategories as cat (cat.id)}
      <div class="cat-row" class:inactive={!cat.active}>
        <span class="cat-dot" style:background={cat.color}></span>
        <span class="cat-name">{$t(cat.name)}</span>
        {#if !cat.active}
          <span class="zf-chip">{$t("Inactive")}</span>
        {/if}
        <button
          class="zf-btn zf-btn-ghost zf-btn-sm"
          on:click={() => (showAbsenceDialog = cat)}
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

{#if showAbsenceDialog}
  <AbsenceCategoryDialog
    template={showAbsenceDialog}
    onClose={(changed) => {
      showAbsenceDialog = null;
      if (changed) loadAbsence();
    }}
  />
{/if}

<style>
  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .section-title {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
  }

  .cat-row {
    padding: 10px 16px;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .cat-row:not(:last-child) {
    border-bottom: 1px solid var(--border);
  }

  /* Deactivated categories stay listed but visually recede. */
  .cat-row.inactive {
    opacity: 0.55;
  }

  /* Slightly larger dot than the global 6px .cat-dot used in tables. */
  .cat-row .cat-dot {
    width: 10px;
    height: 10px;
  }

  .cat-name {
    font-size: 0.875rem;
    font-weight: 500;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
