<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import Icon from "../Icons.svelte";
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

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
  // Only meaningful when cost_type is "none" — see help_unpaid. Switching
  // cost_type away from "none" would otherwise leave a stale, contradictory
  // value the backend has to reject on save; reset it here instead so the
  // checkbox (hidden once cost_type changes) doesn't silently carry a value
  // the admin can no longer see or intend.
  let unpaid = template.unpaid ?? false;
  $: if (cost_type !== "none") unpaid = false;
  let error = "";
  let saving = false;

  let allUsers = [];
  let enabledUserIds = [];
  // Set once a new category's initial POST succeeds. If the follow-up
  // users-PUT then fails, retrying save() must reuse this id instead of
  // POSTing again — the category's slug is unique, so a second POST would
  // fail with a confusing "already exists" error for a category that in
  // fact was already created by the first attempt.
  let createdId = null;

  onMount(async () => {
    try {
      const [loadedUsers, loadedEnabled] = await Promise.all([
        api("/users"),
        isNew
          ? Promise.resolve(null)
          : api("/absence-categories/" + template.id + "/users"),
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

  // Which of the behavior options currently has its help text expanded.
  // Following the same toggle-on-click info-icon pattern used in the dashboard
  // and report cards, but applied per-option since each flag/choice has its
  // own independent explanation.
  let openHelp = null;
  function toggleHelp(key) {
    openHelp = openHelp === key ? null : key;
  }

  async function save() {
    if (saving) return;
    error = "";
    saving = true;
    try {
      const body = {
        name,
        color,
        sort_order: Number(sort_order),
        cost_type,
        auto_approve_past,
        unpaid,
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
        const created = await api("/absence-categories", {
          method: "POST",
          body,
        });
        categoryId = created.id;
        createdId = categoryId;
      } else {
        await api("/absence-categories/" + categoryId, {
          method: "PUT",
          body,
        });
      }
      // New categories already default to enabled for everyone on the
      // backend (see repository::absence_categories::create); only push an
      // explicit list if the admin actually narrowed it down. This also
      // means a failed or not-yet-finished /users fetch can't wipe the
      // default down to "nobody" via an empty enabledUserIds.
      if (!isNew || enabledUserIds.length < allUsers.length) {
        await api("/absence-categories/" + categoryId + "/users", {
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
        class="zf-input zf-color-input"
        type="color"
        bind:value={color}
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
  <div class="mt-10 choice-list">
    <!--
      Each behavior option pairs its control (radio for cost_type, checkbox
      for auto_approve_past) with a small info button that toggles a help
      paragraph below the row. Mirrors the click-to-expand pattern in
      EmployeeReport/StatCards so users have one consistent mental model:
      click the (i) for context, click again to hide.
    -->
    <div>
      <label class="zf-check-label">
        <input
          type="radio"
          name="cost_type"
          value="none"
          bind:group={cost_type}
        />
        <span>{$t("label_cost_type_none")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost zf-help-btn"
          aria-expanded={openHelp === "cost_type_none"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("cost_type_none")}
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "cost_type_none"}
        <div class="abscat-help">{$t("help_cost_type_none")}</div>
      {/if}
      {#if cost_type === "none"}
        <label class="zf-check-label abscat-suboption">
          <input type="checkbox" bind:checked={unpaid} />
          <span>{$t("label_unpaid")}</span>
          <button
            type="button"
            class="zf-btn-icon-sm zf-btn-ghost zf-help-btn"
            aria-expanded={openHelp === "unpaid"}
            aria-label={$t("Show explanation")}
            on:click={() => toggleHelp("unpaid")}
          >
            <Icon name="Info" size={14} />
          </button>
        </label>
        {#if openHelp === "unpaid"}
          <div class="abscat-help">{$t("help_unpaid")}</div>
        {/if}
      {/if}
    </div>
    <div>
      <label class="zf-check-label">
        <input
          type="radio"
          name="cost_type"
          value="vacation"
          bind:group={cost_type}
        />
        <span>{$t("label_cost_type_vacation")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost zf-help-btn"
          aria-expanded={openHelp === "cost_type_vacation"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("cost_type_vacation")}
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "cost_type_vacation"}
        <div class="abscat-help">{$t("help_cost_type_vacation")}</div>
      {/if}
    </div>
    <div>
      <label class="zf-check-label">
        <input
          type="radio"
          name="cost_type"
          value="flextime"
          bind:group={cost_type}
        />
        <span>{$t("label_cost_type_flextime")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost zf-help-btn"
          aria-expanded={openHelp === "cost_type_flextime"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("cost_type_flextime")}
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "cost_type_flextime"}
        <div class="abscat-help">{$t("help_cost_type_flextime")}</div>
      {/if}
    </div>
    <div>
      <label class="zf-check-label">
        <input type="checkbox" bind:checked={auto_approve_past} />
        <span>{$t("Auto-approve past dates")}</span>
        <button
          type="button"
          class="zf-btn-icon-sm zf-btn-ghost zf-help-btn"
          aria-expanded={openHelp === "auto_approve_past"}
          aria-label={$t("Show explanation")}
          on:click={() => toggleHelp("auto_approve_past")}
        >
          <Icon name="Info" size={14} />
        </button>
      </label>
      {#if openHelp === "auto_approve_past"}
        <div class="abscat-help">{$t("help_auto_approve_past")}</div>
      {/if}
    </div>
    <!--
      "Active" is kept inside the same flex column as the cost_type radios
      and auto_approve_past so the vertical gap between every option is the
      shared `gap:6px` rather than the row-specific `margin-top` that used to
      add an extra ~2px above "Active".
    -->
    {#if !isNew}
      <div>
        <label class="zf-check-label">
          <input type="checkbox" bind:checked={active} />
          <span>{$t("Active")}</span>
        </label>
      </div>
    {/if}
  </div>
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

<style>
  .choice-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  /* "Unpaid" only applies to the "none" cost type, so it's indented to read
     as a sub-option nested under it rather than a sibling choice. */
  .abscat-suboption {
    margin-left: 26px;
  }

  /*
    Help text appears directly below its option, indented under the checkbox
    so it visually attaches to the option above. Muted color and reduced
    font size keep it secondary to the form itself.
  */
  .abscat-help {
    margin: 4px 0 4px 26px;
    padding: 8px 10px;
    font-size: 0.8125rem;
    line-height: 1.4;
    color: var(--text-secondary, #475569);
    background: var(--surface-muted, #f1f5f9);
    border-left: 3px solid var(--border, #cbd5e1);
    border-radius: 4px;
  }
</style>
