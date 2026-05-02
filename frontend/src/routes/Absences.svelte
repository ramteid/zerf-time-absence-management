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
    } catch {}

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
    return countWorkdays(
      absence.start_date,
      absence.end_date,
      absence.half_day,
      holidayDates,
    );
  }

  async function cancel(id) {
    const reason = await confirmDialog(
      $t("Cancel?"),
      $t("Cancel this absence request?"),
      {
        danger: true,
        confirm: $t("Cancel"),
        reason: true,
      },
    );
    if (!reason) return;
    await api("/absences/" + id, {
      method: "DELETE",
      body: { reason },
    });
    toast($t("Absence cancelled."), "ok");
    load();
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

<div class="content-area">
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

  <div class="kz-card" style="overflow:hidden">
    <div class="card-header">
      <span class="card-header-title">{$t("Absence History")}</span>
    </div>
    <table class="kz-table">
      <thead>
        <tr>
          <th>{$t("Type")}</th>
          <th>{$t("From")}</th>
          <th>{$t("To")}</th>
          <th>{$t("Days")}</th>
          <th>{$t("Status")}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each absences as a}
          <tr>
            <td style="font-weight:500">{absenceKindLabel(a.kind)}</td>
            <td class="tab-num">{fmtDate(a.start_date)}</td>
            <td class="tab-num">{fmtDate(a.end_date)}</td>
            <td class="tab-num">{absenceDays(a) || "–"}</td>
            <td
              ><span class="kz-chip kz-chip-{a.status}"
                >{statusLabel(a.status)}</span
              ></td
            >
            <td style="text-align:right">
              {#if a.status === "requested" || a.status === "draft"}
                <button
                  class="kz-btn kz-btn-ghost kz-btn-sm kz-btn-danger"
                  on:click={() => cancel(a.id)}
                >
                  {$t("Cancel")}
                </button>
              {/if}
              {#if a.status === "draft"}
                <button
                  class="kz-btn kz-btn-ghost kz-btn-sm"
                  on:click={() => (showDialog = a)}
                >
                  <Icon name="Edit" size={13} />
                </button>
              {/if}
            </td>
          </tr>
        {/each}
        {#if absences.length === 0}
          <tr>
            <td
              colspan="6"
              style="text-align:center;padding:32px;color:var(--text-tertiary)"
            >
              {$t("No absences yet.")}
            </td>
          </tr>
        {/if}
      </tbody>
    </table>
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
