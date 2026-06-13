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

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Categories")}</h1>
  </div>
</div>

<div class="content-area" style="max-width:600px">
  <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px">
    <h2 style="margin:0;font-size:15px;font-weight:600">{$t("Time Categories")}</h2>
    <button class="zf-btn zf-btn-sm" on:click={() => (showDialog = {})}>
      <Icon name="Plus" size={13} />{$t("Add Category")}
    </button>
  </div>
  <div class="zf-card" style="overflow-x:auto;margin-bottom:24px">
    {#each adminCategories as cat, i (cat.id)}
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

  <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px">
    <h2 style="margin:0;font-size:15px;font-weight:600">{$t("Absence Categories")}</h2>
    <button class="zf-btn zf-btn-sm" on:click={() => (showAbsenceDialog = {})}>
      <Icon name="Plus" size={13} />{$t("Add Absence Category")}
    </button>
  </div>
  <div class="zf-card" style="overflow-x:auto">
    {#each adminAbsenceCategories as cat, i (cat.id)}
      <div
        style="padding:10px 16px;{i < adminAbsenceCategories.length - 1
          ? 'border-bottom:1px solid var(--border)'
          : ''};display:flex;align-items:center;gap:10px;opacity:{cat.active
          ? 1
          : 0.55}"
      >
        <span
          class="cat-dot"
          style="width:10px;height:10px;background:{cat.color}"
        ></span>
        <span style="font-size:13px;font-weight:500;flex:1">{cat.name}</span>
        {#if cat.counts_as_vacation}
          <span class="zf-chip">{$t("Counts as vacation")}</span>
        {/if}
        {#if cat.keeps_work_target}
          <span class="zf-chip">{$t("Keeps work target (flextime)")}</span>
        {/if}
        {#if cat.auto_approve_past}
          <span class="zf-chip">{$t("Auto-approve past dates (sick-like)")}</span>
        {/if}
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
