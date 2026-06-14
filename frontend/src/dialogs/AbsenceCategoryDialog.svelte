<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import Icon from "../Icons.svelte";

  export let template;
  export let onClose;
  let dialog;
  $: isNew = !template.id;
  let name = template.name || "";
  let color = template.color || "#5b8def";
  let sort_order = template.sort_order ?? 0;
  let active = template.active ?? true;
  // cost_type collapses the former counts_as_vacation/keeps_work_target
  // booleans into a single 3-state enum ("none" | "vacation" | "flextime").
  // The two booleans were always mutually exclusive; the enum makes that
  // impossible to violate in either direction.
  let cost_type = template.cost_type ?? "none";
  let auto_approve_past = template.auto_approve_past ?? false;
  let error = "";

  // Which of the behavior options currently has its help text expanded.
  // Following the same toggle-on-click info-icon pattern used in the dashboard
  // and report cards, but applied per-option since each flag/choice has its
  // own independent explanation.
  let openHelp = null;
  function toggleHelp(key) {
    openHelp = openHelp === key ? null : key;
  }

  async function save() {
    error = "";
    try {
      const body = {
        name,
        color,
        sort_order: Number(sort_order),
        cost_type,
        auto_approve_past,
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
    <!--
      Each behavior option pairs its control (radio for cost_type, checkbox
      for auto_approve_past) with a small info button that toggles a help
      paragraph below the row. Mirrors the click-to-expand pattern in
      EmployeeReport/StatCards so users have one consistent mental model:
      click the (i) for context, click again to hide.
    -->
    <div>
      <label style="display:flex;align-items:center;gap:8px;font-size:13px">
        <input
          type="radio"
          name="cost_type"
          value="none"
          bind:group={cost_type}
        />
        <span>{$t("No cost (free day)")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost"
          aria-expanded={openHelp === "cost_type_none"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("cost_type_none")}
          style="color:var(--text-tertiary);cursor:help;margin-left:auto"
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "cost_type_none"}
        <div class="abscat-help">{$t("help_cost_type_none")}</div>
      {/if}
    </div>
    <div>
      <label style="display:flex;align-items:center;gap:8px;font-size:13px">
        <input
          type="radio"
          name="cost_type"
          value="vacation"
          bind:group={cost_type}
        />
        <span>{$t("Counts as vacation")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost"
          aria-expanded={openHelp === "cost_type_vacation"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("cost_type_vacation")}
          style="color:var(--text-tertiary);cursor:help;margin-left:auto"
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "cost_type_vacation"}
        <div class="abscat-help">{$t("help_counts_as_vacation")}</div>
      {/if}
    </div>
    <div>
      <label style="display:flex;align-items:center;gap:8px;font-size:13px">
        <input
          type="radio"
          name="cost_type"
          value="flextime"
          bind:group={cost_type}
        />
        <span>{$t("Keeps work target (flextime)")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost"
          aria-expanded={openHelp === "cost_type_flextime"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("cost_type_flextime")}
          style="color:var(--text-tertiary);cursor:help;margin-left:auto"
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "cost_type_flextime"}
        <div class="abscat-help">{$t("help_keeps_work_target")}</div>
      {/if}
    </div>
    <div>
      <label style="display:flex;align-items:center;gap:8px;font-size:13px">
        <input type="checkbox" bind:checked={auto_approve_past} />
        <span>{$t("Auto-approve past dates (sick-like)")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost"
          aria-expanded={openHelp === "auto_approve_past"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("auto_approve_past")}
          style="color:var(--text-tertiary);cursor:help;margin-left:auto"
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "auto_approve_past"}
        <div class="abscat-help">{$t("help_auto_approve_past")}</div>
      {/if}
    </div>
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

<style>
  /*
    Help text appears directly below its option, indented under the checkbox
    so it visually attaches to the option above. Muted color and reduced
    font size keep it secondary to the form itself.
  */
  .abscat-help {
    margin: 4px 0 4px 26px;
    padding: 8px 10px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-secondary, #475569);
    background: var(--surface-muted, #f1f5f9);
    border-left: 3px solid var(--border, #cbd5e1);
    border-radius: 4px;
  }
</style>
