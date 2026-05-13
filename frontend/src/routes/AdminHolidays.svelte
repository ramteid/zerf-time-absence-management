<script>
  import { api } from "../api.js";
  import { settings, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { fmtDate, appTodayDate } from "../format.js";
  import Icon from "../Icons.svelte";
  import { confirmDialog } from "../confirm.js";
  import DatePicker from "../DatePicker.svelte";

  let holidays = [];
  let year = appTodayDate($settings?.timezone).getFullYear();
  let yearTouched = false;
  $: baseYear = appTodayDate($settings?.timezone).getFullYear();
  $: if (!yearTouched && year !== baseYear) {
    year = baseYear;
  }
  let newDate = "";
  let newName = "";

  async function load() {
    holidays = await api(`/holidays?year=${year}`);
  }
  load();

  async function add() {
    if (!newDate || !newName) {
      toast($t("Date and name required"), "error");
      return;
    }
    await api("/holidays", {
      method: "POST",
      body: { holiday_date: newDate, name: newName },
    });
    newDate = "";
    newName = "";
    toast($t("Holiday added."), "ok");
    load();
  }

  async function del(id) {
    if (
      !(await confirmDialog($t("Delete?"), $t("Delete this holiday?"), {
        danger: true,
        confirm: $t("Delete"),
      }))
    )
      return;
    await api("/holidays/" + id, { method: "DELETE" });
    load();
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Holidays")}</h1>
  </div>
  <div class="top-bar-actions">
    <div class="zf-nav-slider">
      <button
        class="zf-btn zf-btn-ghost"
        on:click={() => {
          yearTouched = true;
          year--;
          load();
        }}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <span class="nav-label tab-num" style="min-width:60px">{year}</span>
      <button
        class="zf-btn zf-btn-ghost"
        on:click={() => {
          yearTouched = true;
          year++;
          load();
        }}
      >
        <Icon name="ChevRight" size={16} />
      </button>
    </div>
  </div>
</div>

<div class="content-area" style="max-width:600px">
  <!-- Add form -->
  <div class="zf-card" style="padding:16px;margin-bottom:16px">
    <div style="display:flex;gap:12px;align-items:flex-end;flex-wrap:wrap">
      <div style="flex:1">
        <label class="zf-label" for="holiday-date">{$t("Date")}</label>
        <DatePicker id="holiday-date" bind:value={newDate} />
      </div>
      <div style="flex:2">
        <label class="zf-label" for="holiday-name">{$t("Name")}</label>
        <input
          id="holiday-name"
          class="zf-input"
          bind:value={newName}
          placeholder={$t("Holiday name")}
        />
      </div>
      <button class="zf-btn zf-btn-primary zf-btn-sm" on:click={add}>
        <Icon name="Plus" size={13} />{$t("Add")}
      </button>
    </div>
  </div>

  <div class="zf-card" style="overflow-x:auto">
    {#each holidays as h, i}
      <div
        style="padding:10px 16px;{i < holidays.length - 1
          ? 'border-bottom:1px solid var(--border)'
          : ''};display:flex;align-items:center;gap:10px"
      >
        <span class="tab-num" style="font-size:13px;min-width:100px"
          >{fmtDate(h.holiday_date)}</span
        >
        <span style="font-size:13px;font-weight:500;flex:1">{h.name}</span>
        {#if h.is_auto}
          <span
            style="font-size:10px;padding:1px 6px;border-radius:8px;background:var(--bg-muted);color:var(--text-tertiary)"
            >API</span
          >
        {/if}
        <button
          class="zf-btn zf-btn-ghost zf-btn-sm zf-btn-danger"
          on:click={() => del(h.id)}
        >
          <Icon name="Trash" size={13} />
        </button>
      </div>
    {/each}
    {#if holidays.length === 0}
      <div
        style="padding:32px;text-align:center;color:var(--text-tertiary);font-size:13px"
      >
        {$t("No holidays for {year}.", { year })}
      </div>
    {/if}
  </div>
</div>
