<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";

  export let template;
  export let onClose;
  let dialog;
  $: isNew = !template.id;
  let name = template.name || "";
  let color = template.color || "#5b8def";
  let sort_order = template.sort_order ?? 0;
  let active = template.active ?? true;
  let counts_as_vacation = template.counts_as_vacation ?? false;
  let keeps_work_target = template.keeps_work_target ?? false;
  let auto_approve_past = template.auto_approve_past ?? false;
  let team_visible = template.team_visible ?? false;
  let error = "";

  async function save() {
    error = "";
    try {
      const body = {
        name,
        color,
        sort_order: Number(sort_order),
        counts_as_vacation,
        keeps_work_target,
        auto_approve_past,
        team_visible,
      };
      if (!isNew) {
        body.active = active;
      }
      if (isNew) await api("/absence-categories", { method: "POST", body });
      else await api("/absence-categories/" + template.id, { method: "PUT", body });
      dialog.close(true);
      onClose(true);
    } catch (e) {
      error = $t(e?.message || "Error");
    }
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t(isNew ? "Add Absence Category" : "Edit Absence Category")}
  onClose={() => onClose(false)}
>
  <div>
    <label class="zf-label" for="abscat-name">{$t("Name")}</label>
    <input id="abscat-name" class="zf-input" bind:value={name} required />
  </div>
  <div class="field-row">
    <div>
      <label class="zf-label" for="abscat-color">{$t("Color")}</label>
      <input
        id="abscat-color"
        class="zf-input"
        type="color"
        bind:value={color}
        style="height:36px;padding:4px"
      />
    </div>
    <div>
      <label class="zf-label" for="abscat-order">{$t("Order")}</label>
      <input
        id="abscat-order"
        class="zf-input"
        type="number"
        bind:value={sort_order}
      />
    </div>
  </div>
  <div style="margin-top:10px;display:flex;flex-direction:column;gap:6px">
    <label style="display:flex;align-items:center;gap:8px;font-size:13px">
      <input type="checkbox" bind:checked={counts_as_vacation} />
      <span>{$t("Counts as vacation")}</span>
    </label>
    <label style="display:flex;align-items:center;gap:8px;font-size:13px">
      <input type="checkbox" bind:checked={keeps_work_target} />
      <span>{$t("Keeps work target (flextime)")}</span>
    </label>
    <label style="display:flex;align-items:center;gap:8px;font-size:13px">
      <input type="checkbox" bind:checked={auto_approve_past} />
      <span>{$t("Auto-approve past dates (sick-like)")}</span>
    </label>
    <label style="display:flex;align-items:center;gap:8px;font-size:13px">
      <input type="checkbox" bind:checked={team_visible} />
      <span>{$t("Visible to teammates in team calendar")}</span>
    </label>
  </div>
  {#if !isNew}
    <label
      style="display:flex;align-items:center;gap:8px;font-size:13px;margin-top:8px"
    >
      <input type="checkbox" bind:checked={active} />
      <span>{$t("Active")}</span>
    </label>
  {/if}
  <div class="error-text">{error}</div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={() => dialog.close()}>{$t("Cancel")}</button>
    <button class="zf-btn zf-btn-primary" on:click={save}>{$t("Save")}</button>
  </svelte:fragment>
</Dialog>
