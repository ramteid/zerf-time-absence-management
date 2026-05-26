<script>
  import { earliestStartDate, settings, toast } from "../../stores.js";
  import { t, fmtDecimal } from "../../i18n.js";
  import { appTodayDate, minToHM } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";
  import { getTeamReport } from "../../lib/api/reportsApi.js";

  let today = appTodayDate();
  let currentYear = today.getFullYear();
  // eslint-disable-next-line no-useless-assignment
  let currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: today = appTodayDate($settings?.timezone);
  $: currentYear = today.getFullYear();
  $: currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: earliestStartMonth = $earliestStartDate?.slice(0, 7) ?? null;

  let teamMonth = currentMonthStr;
  let teamReport = null;
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // Clamp teamMonth to the earliest start month.
  $: if (earliestStartMonth && teamMonth < earliestStartMonth) {
    teamMonth = earliestStartMonth;
  }

  // Keep teamMonth aligned with app-timezone date changes if still on default.
  let previousCurrentMonthStr = "";
  $: {
    if (!previousCurrentMonthStr) {
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
    } else {
      if (teamMonth === previousCurrentMonthStr) teamMonth = currentMonthStr;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
    }
  }

  async function showTeam() {
    teamReport = null;
    try {
      teamReport = await getTeamReport({ month: teamMonth });
    } catch (e) {
      teamReport = null;
      toast($t(e?.message || "Error"), "error");
    }
  }

</script>

<div class="zf-card" style="padding:20px;margin-bottom:16px">
  <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
    <span style="font-size:14px;font-weight:400">{$t("Team report")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost"
      title={$t("help_team_report")}
      on:click={() => toggleHelp("team")}
      style="color:var(--text-tertiary);font-size:14px;cursor:help"
    >
      <Icon name="Info" size={14} />
    </button>
  </div>
  {#if activeHelp === "team"}
    <div
      style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
    >
      {$t("help_team_report")}
    </div>
  {/if}

  <div
    style="display:flex;gap:12px;align-items:flex-end;margin-bottom:12px;flex-wrap:wrap"
  >
    <div style="flex:1">
      <label class="zf-label" for="team-month">{$t("Month")}</label>
      <DatePicker id="team-month" mode="month" bind:value={teamMonth} min={earliestStartMonth} max={currentMonthStr} />
    </div>
    <button class="zf-btn zf-btn-primary" on:click={showTeam}>{$t("Show")}</button>
  </div>

  {#if teamReport}
    <div class="zf-table-wrap">
      <table class="zf-table zf-table--fit">
        <thead>
          <tr>
            <th style="min-width:120px">{$t("Employee")}</th>
            <th style="text-align:right;white-space:nowrap">{$t("Current flextime balance")}</th>
            <th style="text-align:right;white-space:nowrap">{$t("Monthly diff")}</th>
            <th style="text-align:right;white-space:nowrap">{$t("Sick days")}</th>
            <th style="text-align:right;white-space:nowrap">{$t("Vacation taken")}</th>
            <th style="text-align:right;white-space:nowrap">{$t("Vacation planned")}</th>
            <th style="text-align:center;white-space:nowrap">{$t("All weeks submitted")}</th>
          </tr>
        </thead>
        <tbody>
          {#each teamReport as r (r.user_id)}
            <tr>
              <td style="font-weight:500">{r.name}</td>
              <td
                class="tab-num"
                style="text-align:right;font-weight:500;color:{r.flextime_balance_min == null
                  ? 'var(--text-tertiary)'
                  : r.flextime_balance_min < 0
                    ? 'var(--danger-text)'
                    : 'var(--success-text)'}"
              >
                {#if r.flextime_balance_min == null}
                  -
                {:else}
                  {r.flextime_balance_min >= 0 ? "+" : ""}{minToHM(
                    r.flextime_balance_min,
                  )}
                {/if}
              </td>
              <td
                class="tab-num"
                style="text-align:right;color:{r.diff_min == null
                  ? 'var(--text-tertiary)'
                  : r.diff_min < 0
                    ? 'var(--danger-text)'
                    : 'var(--success-text)'}"
              >
                {#if r.diff_min == null}
                  -
                {:else}
                  {r.diff_min >= 0 ? "+" : ""}{minToHM(r.diff_min)}
                {/if}
              </td>
              <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                {r.sick_days > 0
                  ? fmtDecimal(r.sick_days, r.sick_days % 1 === 0 ? 0 : 1)
                  : "-"}
              </td>
              <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                {r.vacation_days > 0
                  ? fmtDecimal(r.vacation_days, r.vacation_days % 1 === 0 ? 0 : 1)
                  : "-"}
              </td>
              <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                {r.vacation_planned_days > 0
                  ? fmtDecimal(r.vacation_planned_days, r.vacation_planned_days % 1 === 0 ? 0 : 1)
                  : "-"}
              </td>
              <td style="text-align:center">
                {#if r.weeks_all_submitted}
                  <span style="color:var(--success-text)">{$t("Yes")}</span>
                {:else}
                  <span style="color:var(--danger-text)">{$t("No")}</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
