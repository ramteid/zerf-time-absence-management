<script>
  import { api } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { countWorkdays, holidayDateSet } from "../apiMappers.js";
  import { t, absenceKindLabel, statusLabel } from "../i18n.js";
  import { fmtDate, parseDate } from "../format.js";
  import Icon from "../Icons.svelte";
  import AbsenceDialog from "../dialogs/AbsenceDialog.svelte";
  import { confirmDialog } from "../confirm.js";

  let absences = [];
  let absenceRows = [];
  let balance = null;
  let holidayDates = new Set();
  let showDialog = null;
  let loadToken = 0;
  const baseYear = new Date().getFullYear();
  let selectedYear = baseYear;

  $: yearOptions = [
    ...new Set([
      baseYear - 1,
      baseYear,
      baseYear + 1,
      selectedYear,
      ...absences.flatMap((absence) => [
        parseDate(absence.start_date).getFullYear(),
        parseDate(absence.end_date).getFullYear(),
      ]),
    ]),
  ].sort((a, b) => b - a);

  async function load() {
    const token = ++loadToken;
    const year = selectedYear;
    const nextAbsences = await api(`/absences?year=${year}`);
    if (token !== loadToken) return;
    absences = nextAbsences;

    try {
      const nextBalance = await api(
        `/leave-balance/${$currentUser.id}?year=${year}`,
      );
      if (token !== loadToken) return;
      balance = nextBalance;
    } catch (e) {
      if (token !== loadToken) return;
      toast($t(e?.message || "Leave balance unavailable."), "error");
    }

    const years = [
      ...new Set(
        absences.flatMap((absence) => [
          parseDate(absence.start_date).getFullYear(),
          parseDate(absence.end_date).getFullYear(),
        ]),
      ),
    ];
    const holidayLists = await Promise.all(
      years.map((year) => api(`/holidays?year=${year}`)),
    );
    if (token !== loadToken) return;
    holidayDates = holidayDateSet(holidayLists.flat());
  }

  $: if (selectedYear) {
    load();
  }

  function handleDialogClose(changed, savedAbsence = null) {
    showDialog = null;
    if (!changed) return;

    const savedYear = savedAbsence?.start_date
      ? parseDate(savedAbsence.start_date).getFullYear()
      : null;

    if (savedYear && savedYear !== selectedYear) {
      selectedYear = savedYear;
      return;
    }

    load();
  }

  function canEdit(absence) {
    return absence.status === "requested";
  }

  function canCancel(absence) {
    return (
      absence.status === "requested" ||
      (absence.kind === "sick" &&
        absence.status === "approved" &&
        absence.reviewed_by == null &&
        absence.reviewed_at == null)
    );
  }

  $: absenceRows = absences.map((absence) => ({
    ...absence,
    days: countWorkdays(absence.start_date, absence.end_date, holidayDates),
    editable: canEdit(absence),
    cancellable: canCancel(absence),
  }));

  async function cancel(id) {
    const ok = await confirmDialog(
      $t("Cancel?"),
      $t("Cancel this absence request?"),
      {
        danger: true,
        confirm: $t("Yes, cancel absence"),
      },
    );
    if (!ok) return;
    try {
      await api("/absences/" + id, { method: "DELETE" });
      toast($t("Absence cancelled."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Absences")}</h1>
  </div>
  <div class="top-bar-subtitle">
    {$t("Vacation, sick leave & training days")}
  </div>
  <div class="top-bar-actions">
    <select
      class="kz-select absence-year-select"
      aria-label={$t("Year")}
      value={selectedYear}
      on:change={(event) =>
        (selectedYear = Number(event.currentTarget.value))}
    >
      {#each yearOptions as yearOption}
        <option value={yearOption}>{yearOption}</option>
      {/each}
    </select>
    <button class="kz-btn kz-btn-primary" on:click={() => (showDialog = {})}>
      <Icon name="Plus" size={14} />{$t("Request Absence")}
    </button>
  </div>
</div>

<div class="content-area" style="overflow-x:hidden">
  {#if balance}
    <div class="stat-cards">
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Vacation days ({year})", { year: selectedYear })}</div>
        <div class="stat-card-value tab-num">{balance.annual_entitlement}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Vacation used ({year})", { year: selectedYear })}</div>
        <div class="stat-card-value tab-num">{balance.already_taken}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Approved upcoming ({year})", { year: selectedYear })}</div>
        <div class="stat-card-value tab-num">{balance.approved_upcoming || 0}</div>
        <div class="stat-card-sub">{$t("Approved days not yet taken")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Vacation pending ({year})", { year: selectedYear })}</div>
        <div class="stat-card-value tab-num">{balance.requested || 0}</div>
        <div class="stat-card-sub">{$t("Vacation requests awaiting approval")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Vacation remaining ({year})", { year: selectedYear })}</div>
        <div class="stat-card-value accent tab-num">
          {balance.available}
        </div>
      </div>
    </div>
  {/if}

  <div class="kz-card">
    <div class="card-header">
      <span class="card-header-title">{$t("Absence History")}</span>
    </div>
    {#if absences.length === 0}
      <div style="padding:32px;text-align:center;color:var(--text-tertiary)">
        {$t("No absences yet.")}
      </div>
    {:else}
      <div class="absence-list">
        {#each absenceRows as a}
          <div class="absence-entry">
            <div class="absence-entry-row">
              <span class="absence-entry-label">{$t("Type")}</span>
              <span class="absence-entry-value" style="font-weight:500"
                >{absenceKindLabel(a.kind)}</span
              >
            </div>
            <div class="absence-entry-row">
              <span class="absence-entry-label">{$t("From")}</span>
              <span class="absence-entry-value tab-num"
                >{fmtDate(a.start_date)}</span
              >
            </div>
            <div class="absence-entry-row">
              <span class="absence-entry-label">{$t("To")}</span>
              <span class="absence-entry-value tab-num"
                >{fmtDate(a.end_date)}</span
              >
            </div>
            <div class="absence-entry-row">
              <span class="absence-entry-label">{$t("Days")}</span>
              <span class="absence-entry-value tab-num">{a.days || "–"}</span>
            </div>
            <div class="absence-entry-row">
              <span class="absence-entry-label">{$t("Status")}</span>
              <span class="absence-entry-value">
                <span class="kz-chip kz-chip-{a.status}"
                  >{statusLabel(a.status)}</span
                >
              </span>
            </div>
            <div class="absence-entry-actions">
              {#if a.cancellable}
                <button
                  class="kz-btn kz-btn-ghost kz-btn-sm kz-btn-danger"
                  on:click={() => cancel(a.id)}
                >
                  {$t("Cancel")}
                </button>
              {/if}
              {#if a.editable}
                <button
                  class="kz-btn kz-btn-ghost kz-btn-sm"
                  on:click={() => (showDialog = a)}
                >
                  <Icon name="Edit" size={13} />
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if showDialog}
  <AbsenceDialog template={showDialog} onClose={handleDialogClose} />
{/if}

<style>
  .absence-list {
    display: flex;
    flex-direction: column;
  }

  .absence-entry {
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
    align-items: center;
  }

  .absence-entry:last-child {
    border-bottom: none;
  }

  .absence-entry-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .absence-entry-label {
    font-size: 11px;
    color: var(--text-tertiary);
    min-width: 40px;
  }

  .absence-entry-value {
    font-size: 13px;
    text-align: left;
  }

  .absence-entry-actions {
    margin-left: auto;
    display: flex;
    gap: 4px;
  }

  .absence-year-select {
    min-width: 92px;
  }

  @media (max-width: 640px) {
    .absence-entry {
      flex-direction: column;
      align-items: flex-start;
      gap: 4px;
    }

    .absence-entry-row {
      width: 100%;
      align-items: flex-start;
      flex-direction: column;
      gap: 1px;
    }

    .absence-entry-actions {
      margin-left: 0;
      padding-top: 4px;
    }
  }
</style>
