<script>
  import { api } from "../api.js";
  import { settings, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { fmtDate, appTodayDate, parseDate } from "../format.js";
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
  let recurring = false;
  let hasEnd = false;
  let recurrenceEndYear = null;

  // The end-year dropdown only offers years after the holiday's own year (a
  // recurring holiday whose end year equals its start year would never
  // actually recur, which the backend permits but is confusing to offer).
  $: selectedYear = newDate ? parseDate(newDate).getFullYear() : year;
  $: endYearOptions = Array.from(
    { length: 30 },
    (_, i) => selectedYear + 1 + i,
  );

  // Unchecking "repeats every year" clears the now-meaningless end option.
  $: if (!recurring && (hasEnd || recurrenceEndYear !== null)) {
    hasEnd = false;
    recurrenceEndYear = null;
  }
  // Pick a sensible default once "End" is checked, and re-clamp it if the
  // chosen date's year later moves past the previously picked end year.
  $: if (
    hasEnd &&
    (recurrenceEndYear === null || recurrenceEndYear < selectedYear + 1)
  ) {
    recurrenceEndYear = selectedYear + 10;
  }

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
      body: {
        holiday_date: newDate,
        name: newName,
        recurring,
        recurrence_end_year: recurring && hasEnd ? recurrenceEndYear : null,
      },
    });
    newDate = "";
    newName = "";
    recurring = false;
    hasEnd = false;
    recurrenceEndYear = null;
    toast($t("Holiday added."), "ok");
    load();
  }

  async function del(holiday) {
    const message = holiday.recurring
      ? $t("Delete this holiday?") +
        " " +
        $t(
          "This holiday repeats every year. Deleting it removes it for every year, not only {year}.",
          { year },
        )
      : $t("Delete this holiday?");
    if (
      !(await confirmDialog($t("Delete?"), message, {
        danger: true,
        confirm: $t("Delete"),
      }))
    )
      return;
    await api("/holidays/" + holiday.id, { method: "DELETE" });
    load();
  }
</script>

<div class="top-bar page-narrow">
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
      <span class="nav-label tab-num year-label">{year}</span>
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

<div class="content-area page-narrow">
  <!-- Add form -->
  <div class="zf-card form-card mb-16">
    <div class="zf-toolbar-row">
      <div class="flex-1">
        <label class="zf-label" for="holiday-date">{$t("Date")}</label>
        <DatePicker id="holiday-date" bind:value={newDate} />
      </div>
      <div class="grow-2">
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
    <div class="recurrence-row mt-8">
      <label class="zf-check-label">
        <input type="checkbox" bind:checked={recurring} />
        <span>{$t("Repeats every year")}</span>
      </label>
      <label class="zf-check-label">
        <input type="checkbox" bind:checked={hasEnd} disabled={!recurring} />
        <span>{$t("End")}</span>
      </label>
      {#if recurring && hasEnd}
        <select class="zf-input end-year-select" bind:value={recurrenceEndYear}>
          {#each endYearOptions as optionYear (optionYear)}
            <option value={optionYear}>{optionYear}</option>
          {/each}
        </select>
      {/if}
    </div>
  </div>

  <div class="zf-card zf-table-wrap">
    {#each holidays as h (h.id)}
      <div class="holiday-row">
        <span class="tab-num holiday-date">{fmtDate(h.holiday_date)}</span>
        <span class="holiday-name">{h.name}</span>
        {#if h.is_auto}
          <span class="holiday-source">API</span>
        {/if}
        {#if h.recurring}
          <span
            class="holiday-source"
            title={h.recurrence_end_year
              ? $t("Recurs until {year}.", { year: h.recurrence_end_year })
              : $t("Recurs every year.")}
          >
            {$t("Recurring")}
          </span>
        {/if}
        <button
          class="zf-btn zf-btn-ghost zf-btn-sm zf-btn-danger"
          on:click={() => del(h)}
        >
          <Icon name="Trash" size={13} />
        </button>
      </div>
    {/each}
    {#if holidays.length === 0}
      <div class="zf-empty fs-14">
        {$t("No holidays for {year}.", { year })}
      </div>
    {/if}
  </div>
</div>

<style>
  /* Fixed width so the year does not shift the arrow buttons around. */
  .year-label {
    min-width: 60px;
  }

  .form-card {
    padding: 16px;
  }

  /* Name field gets twice the width of the date field in the add-row. */
  .grow-2 {
    flex: 2;
  }

  .recurrence-row {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }

  .end-year-select {
    width: auto;
  }

  .holiday-row {
    padding: 10px 16px;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .holiday-row:not(:last-child) {
    border-bottom: 1px solid var(--border);
  }

  .holiday-date {
    font-size: 0.875rem;
    min-width: 100px;
  }

  .holiday-name {
    font-size: 0.875rem;
    font-weight: 500;
    flex: 1;
  }

  /* Small "API" pill marking holidays imported from the holiday service. */
  .holiday-source {
    font-size: 0.6875rem;
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--bg-muted);
    color: var(--text-tertiary);
  }
</style>
