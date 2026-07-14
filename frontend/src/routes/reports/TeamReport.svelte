<script>
  import { earliestStartDate, settings, toast } from "../../stores.js";
  import { t, fmtDecimal } from "../../i18n.js";
  import { appTodayDate, minToHM } from "../../format.js";
  import DatePicker from "../../DatePicker.svelte";
  import SectionCard from "../../lib/ui/SectionCard.svelte";
  import DataTable from "../../lib/ui/DataTable.svelte";
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
      const loaded = await getTeamReport({ month: teamMonth });
      teamReport = (loaded || []).sort((a, b) => a.name.localeCompare(b.name));
    } catch (e) {
      teamReport = null;
      toast($t(e?.message || "Error"), "error");
    }
  }
</script>

<SectionCard
  title={$t("Team report")}
  helpText={$t("help_team_report")}
  helpOpen={activeHelp === "team"}
  onHelpToggle={() => toggleHelp("team")}
>
  <div class="zf-toolbar-row mb-12">
    <div class="flex-1">
      <label class="zf-label" for="team-month">{$t("Month")}</label>
      <DatePicker
        id="team-month"
        mode="month"
        bind:value={teamMonth}
        min={earliestStartMonth}
        max={currentMonthStr}
      />
    </div>
    <button class="zf-btn zf-btn-primary" on:click={showTeam}
      >{$t("Show")}</button
    >
  </div>

  {#if teamReport}
    <DataTable fit>
      <thead>
        <tr>
          <th class="col-employee">{$t("Employee")}</th>
          <th class="text-right nowrap">{$t("Current flextime balance")}</th>
          <th class="text-right nowrap">{$t("Monthly diff")}</th>
          <th class="text-right nowrap">{$t("Sick days")}</th>
          <th class="text-right nowrap">{$t("Vacation taken")}</th>
          <th class="text-right nowrap">{$t("Vacation planned")}</th>
          <th class="text-center nowrap">{$t("All weeks submitted")}</th>
        </tr>
      </thead>
      <tbody>
        {#each teamReport as r (r.user_id)}
          <tr>
            <td class="fw-500">{r.name}</td>
            <td
              class="tab-num text-right fw-500"
              style:color={r.flextime_balance_min == null
                ? "var(--text-tertiary)"
                : r.flextime_balance_min < 0
                  ? "var(--danger-text)"
                  : "var(--success-text)"}
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
              class="tab-num text-right"
              style:color={r.diff_min == null
                ? "var(--text-tertiary)"
                : r.diff_min < 0
                  ? "var(--danger-text)"
                  : "var(--success-text)"}
            >
              {#if r.diff_min == null}
                -
              {:else}
                {r.diff_min >= 0 ? "+" : ""}{minToHM(r.diff_min)}
              {/if}
            </td>
            <td class="tab-num text-right text-tertiary">
              {r.sick_days > 0
                ? fmtDecimal(r.sick_days, r.sick_days % 1 === 0 ? 0 : 1)
                : "-"}
            </td>
            <td class="tab-num text-right text-tertiary">
              {r.vacation_days > 0
                ? fmtDecimal(r.vacation_days, r.vacation_days % 1 === 0 ? 0 : 1)
                : "-"}
            </td>
            <td class="tab-num text-right text-tertiary">
              {r.vacation_planned_days > 0
                ? fmtDecimal(
                    r.vacation_planned_days,
                    r.vacation_planned_days % 1 === 0 ? 0 : 1,
                  )
                : "-"}
            </td>
            <td class="text-center">
              {#if r.weeks_all_submitted}
                <span class="text-success">{$t("Yes")}</span>
              {:else}
                <span class="text-danger">{$t("No")}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </DataTable>
  {/if}
</SectionCard>

<style>
  .col-employee {
    min-width: 120px;
  }
</style>
