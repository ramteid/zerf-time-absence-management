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
  let balance = null;
  let holidayDates = new Set();
  let showDialog = null;

  async function load() {
    absences = await api("/absences");
    try {
      balance = await api("/leave-balance/" + $currentUser.id);
    } catch (e) {
      toast(e.message || $t("Leave balance unavailable."), "error");
    }

    const years = [
      ...new Set(
        absences.flatMap((absence) => [
          parseDate(absence.start_date).getFullYear(),
          parseDate(absence.end_date).getFullYear(),
        ]),
      ),
    ];
    holidayDates = holidayDateSet(
      (
        await Promise.all(years.map((year) => api(`/holidays?year=${year}`)))
      ).flat(),
    );
  }
  load();

  function absenceDays(absence) {
    return countWorkdays(absence.start_date, absence.end_date, holidayDates);
  }

  function canEdit(absence) {
    return (
      absence.status === "requested" ||
      (absence.kind === "sick" && absence.status === "approved")
    );
  }

  async function cancel(id) {
    const ok = await confirmDialog(
      $t("Cancel?"),
      $t("Cancel this absence request?"),
      {
        danger: true,
        confirm: $t("Cancel"),
      },
    );
    if (!ok) return;
    try {
      await api("/absences/" + id, { method: "DELETE" });
      toast($t("Absence cancelled."), "ok");
      load();
    } catch (e) {
      toast(e.message || $t("Error"), "error");
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Absences")}</h1>
    <div class="top-bar-subtitle">
      {$t("Vacation, sick leave & training days")}
    </div>
  </div>
  <div class="top-bar-actions">
    <button class="kz-btn kz-btn-primary" on:click={() => (showDialog = {})}>
      <Icon name="Plus" size={14} />{$t("Request Absence")}
    </button>
  </div>
</div>

<div class="content-area" style="overflow-x:hidden">
  {#if balance}
    <div class="stat-cards">
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Total Days")}</div>
        <div class="stat-card-value tab-num">{balance.annual_entitlement}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Used")}</div>
        <div class="stat-card-value tab-num">{balance.already_taken}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Pending")}</div>
        <div class="stat-card-value tab-num">{balance.requested || 0}</div>
        <div class="stat-card-sub">{$t("awaiting approval")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Remaining")}</div>
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
        {#each absences as a}
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
              <span class="absence-entry-value tab-num"
                >{absenceDays(a) || "–"}</span
              >
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
              {#if a.status === "requested"}
                <button
                  class="kz-btn kz-btn-ghost kz-btn-sm kz-btn-danger"
                  on:click={() => cancel(a.id)}
                >
                  {$t("Cancel")}
                </button>
              {/if}
              {#if canEdit(a)}
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
  <AbsenceDialog
    template={showDialog}
    onClose={(changed) => {
      showDialog = null;
      if (changed) load();
    }}
  />
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
