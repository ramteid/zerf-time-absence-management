<script>
  // The "Team" tab of the Reports page: three related views over the same
  // shared toolbar period — per-person month totals, a category matrix, and
  // team absences — instead of three separately-filtered cards.
  import { currentUser, toast } from "../../stores.js";
  import {
    t,
    fmtDecimal,
    absenceKindLabel,
    statusLabel,
    formatDayCount,
  } from "../../i18n.js";
  import { minToHM, fmtDate } from "../../format.js";
  import SectionCard from "../../lib/ui/SectionCard.svelte";
  import DataTable from "../../lib/ui/DataTable.svelte";
  import LoadingState from "../../lib/ui/LoadingState.svelte";
  import {
    getTeamReport,
    getTeamCategoryReport,
    getAbsenceReport,
    getUserAbsencesByYear,
    getHolidaysByYear,
  } from "../../lib/api/reportsApi.js";
  import { periodBounds } from "../../lib/domain/reportPeriod.js";
  import { yearsBetweenDates } from "../../lib/domain/dates.js";
  import {
    categoryColumnsFromTeamReport,
    filterTeamCategoryColumns,
    leaveAccountUsage,
    teamCategoryMinutes,
    teamCategoryRowTotal,
    dedupeAbsences,
  } from "../../lib/domain/reports.js";
  import { countWorkdays, holidayDateSet } from "../../apiMappers.js";
  import { tracksOwnTime } from "../../rolePolicy.js";
  import { userWorkdaysPerWeekById } from "../../lib/domain/users.js";

  export let users = [];
  export let periodMode = "month";
  export let month = "";
  export let from = "";
  export let to = "";

  let activeHelp = null;
  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // --- Section 1: team month table (month mode only) ---
  let teamReport = null;
  // One entry per leave-account category that has started by the report
  // month (see handlers::reports::team) — independent leave accounts (e.g.
  // Vacation, Bildungsurlaub) each get their own column, bound to row data
  // by category_id via leaveAccountUsage().
  let teamLeaveAccountColumns = [];
  let teamLoading = false;
  let lastTeamKey = "";
  async function loadTeam(key) {
    teamLoading = true;
    try {
      const loaded = await getTeamReport({ month });
      if (key === lastTeamKey) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- teamReport isn't read by the triggering $: block, so there's no cycle.
        teamReport = (loaded?.rows || []).sort((a, b) =>
          a.name.localeCompare(b.name),
        );
        teamLeaveAccountColumns = loaded?.leave_account_categories || [];
      }
    } catch (e) {
      if (key === lastTeamKey) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- see above.
        teamReport = null;
        teamLeaveAccountColumns = [];
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (key === lastTeamKey) teamLoading = false;
    }
  }
  $: {
    if (periodMode === "month" && month) {
      const key = `month:${month}`;
      if (key !== lastTeamKey) {
        lastTeamKey = key;
        // eslint-disable-next-line svelte/infinite-reactive-loop -- loadTeam only writes teamReport, which this block never reads.
        loadTeam(key);
      }
    } else {
      lastTeamKey = "";
      teamReport = null;
    }
  }

  // --- Section 2: category matrix ---
  let teamCatReport = null;
  let catFilteredCategories = [];
  let catShowFilter = false;
  let catLoading = false;
  let lastCatKey = "";
  async function loadCategories(key, catFrom, catTo) {
    catLoading = true;
    try {
      const loaded = await getTeamCategoryReport({ from: catFrom, to: catTo });
      if (key === lastCatKey) {
        teamCatReport = (loaded || []).sort((a, b) =>
          a.name.localeCompare(b.name),
        );
        catFilteredCategories = categoryColumnsFromTeamReport(
          teamCatReport,
        ).map((c) => c.category);
        catShowFilter = false;
      }
    } catch (e) {
      if (key === lastCatKey) {
        teamCatReport = null;
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (key === lastCatKey) catLoading = false;
    }
  }
  $: catBounds = periodBounds({ mode: periodMode, month, from, to });
  $: {
    const key = `${catBounds.from}:${catBounds.to}`;
    if (catBounds.from && catBounds.to && key !== lastCatKey) {
      lastCatKey = key;
      loadCategories(key, catBounds.from, catBounds.to);
    }
  }

  function toggleCategoryFilter(categoryName) {
    catFilteredCategories = catFilteredCategories.includes(categoryName)
      ? catFilteredCategories.filter((name) => name !== categoryName)
      : [...catFilteredCategories, categoryName];
  }
  $: allTeamCatColumns = teamCatReport
    ? categoryColumnsFromTeamReport(teamCatReport)
    : [];
  $: visibleTeamCatColumns = filterTeamCategoryColumns(
    allTeamCatColumns,
    catFilteredCategories,
  );

  // --- Section 3: team absences (full period — planned absences look forward) ---
  let teamAbsences = null;
  let absencesLoading = false;
  let lastAbsenceKey = "";
  async function loadAbsences(key, absenceFrom, absenceTo) {
    absencesLoading = true;
    try {
      const [teamRaw, ownRaw] = await Promise.all([
        getAbsenceReport({ from: absenceFrom, to: absenceTo }),
        tracksOwnTime($currentUser)
          ? Promise.all(
              yearsBetweenDates(absenceFrom, absenceTo).map((year) =>
                getUserAbsencesByYear(year),
              ),
            ).then((lists) =>
              lists
                .flat()
                .filter(
                  (a) => a.end_date >= absenceFrom && a.start_date <= absenceTo,
                ),
            )
          : Promise.resolve([]),
      ]);
      let raw = dedupeAbsences([...(teamRaw || []), ...ownRaw]).filter(
        (a) => a.status !== "rejected" && a.status !== "cancelled",
      );
      if (raw.length === 0) {
        if (key === lastAbsenceKey) teamAbsences = [];
        return;
      }
      const years = [
        ...new Set(
          raw.flatMap((a) => [
            parseInt(a.start_date.slice(0, 4), 10),
            parseInt(a.end_date.slice(0, 4), 10),
          ]),
        ),
      ];
      const holidayLists = await Promise.all(
        years.map((y) => getHolidaysByYear(y)),
      );
      const holidayDates = holidayDateSet(holidayLists.flat());
      const withDays = raw.map((a) => {
        const clampedFrom =
          a.start_date > absenceFrom ? a.start_date : absenceFrom;
        const clampedTo = a.end_date < absenceTo ? a.end_date : absenceTo;
        const workdaysPerWeek = userWorkdaysPerWeekById(users, a.user_id, 5);
        const days =
          clampedTo < clampedFrom
            ? 0
            : countWorkdays(
                clampedFrom,
                clampedTo,
                holidayDates,
                workdaysPerWeek,
              );
        return { ...a, days };
      });
      if (key === lastAbsenceKey) teamAbsences = withDays;
    } catch (e) {
      if (key === lastAbsenceKey) {
        teamAbsences = null;
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (key === lastAbsenceKey) absencesLoading = false;
    }
  }
  $: {
    const key = `${catBounds.from}:${catBounds.to}`;
    if (catBounds.from && catBounds.to && key !== lastAbsenceKey) {
      lastAbsenceKey = key;
      loadAbsences(key, catBounds.from, catBounds.to);
    }
  }

  function userName(userId) {
    const u = users.find((user) => user.id === userId);
    return u ? `${u.first_name} ${u.last_name}` : `#${userId}`;
  }
</script>

<SectionCard
  title={$t("Team report")}
  helpText={$t("help_team_report")}
  helpOpen={activeHelp === "team"}
  onHelpToggle={() => toggleHelp("team")}
>
  {#if periodMode !== "month"}
    <div class="report-note">{$t("team_table_month_only")}</div>
  {:else if teamLoading && !teamReport}
    <LoadingState />
  {:else if teamReport}
    <div class="team-report-table">
      <DataTable fit>
        <thead>
          <tr>
            <th class="col-employee team-report-header">{$t("Employee")}</th>
            <th class="text-right team-report-header">{$t("Current flextime balance")}</th>
            <th class="text-right team-report-header">{$t("Monthly diff")}</th>
            <th class="text-right team-report-header">{$t("Sick days")}</th>
            {#each teamLeaveAccountColumns as col (col.category_id)}
              <th
                class="text-right team-report-header"
                data-testid={`team-leave-account-column-${col.category_id}`}
              >
                <span class="th-cat">
                  <span class="cat-dot" style:background={col.color || "#999"}
                  ></span>
                  {$t(col.name)}
                </span>
              </th>
            {/each}
            <th class="text-center team-report-header">{$t("All weeks submitted")}</th>
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
            {#each teamLeaveAccountColumns as col (col.category_id)}
              {@const usage = leaveAccountUsage(r, col.category_id)}
              <td
                class="tab-num text-right text-tertiary"
                data-testid={`team-leave-account-${r.user_id}-${col.category_id}`}
                title={`${$t("Taken")}: ${formatDayCount(usage.taken_days)} · ${$t("Approved planned")}: ${formatDayCount(usage.planned_days)}`}
              >
                {usage.taken_days > 0 || usage.planned_days > 0
                  ? `${formatDayCount(usage.taken_days)} / ${formatDayCount(usage.planned_days)}`
                  : "-"}
              </td>
            {/each}
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
    </div>
  {/if}
</SectionCard>

<SectionCard
  title={$t("Category breakdown")}
  helpText={$t("help_category_breakdown")}
  helpOpen={activeHelp === "cat"}
  onHelpToggle={() => toggleHelp("cat")}
>
  {#if allTeamCatColumns.length > 0}
    <div class="report-toolbar">
      <button class="zf-btn" on:click={() => (catShowFilter = !catShowFilter)}>
        {$t("Filter")}
        {#if catFilteredCategories.length > 0 && catFilteredCategories.length < allTeamCatColumns.length}
          ({catFilteredCategories.length})
        {/if}
      </button>
    </div>
    {#if catShowFilter}
      <div class="filter-panel">
        <div class="filter-options">
          {#each allTeamCatColumns as col (col.category)}
            <label class="filter-check">
              <input
                type="checkbox"
                checked={catFilteredCategories.includes(col.category)}
                on:change={() => toggleCategoryFilter(col.category)}
              />
              <span class="cat-dot" style:background={col.color || "#999"}
              ></span>
              <span>{$t(col.category)}</span>
            </label>
          {/each}
        </div>
      </div>
    {/if}
  {/if}

  {#if catLoading && !teamCatReport}
    <LoadingState />
  {:else if teamCatReport}
    {#if teamCatReport.length === 0 || visibleTeamCatColumns.length === 0}
      <div class="zf-card-empty">{$t("No data.")}</div>
    {:else}
      <DataTable fit>
        <thead>
          <tr>
            <th>{$t("Employee")}</th>
            {#each visibleTeamCatColumns as col (col.category)}
              <th class="text-right">
                <span class="th-cat">
                  <span class="cat-dot" style:background={col.color || "#999"}
                  ></span>
                  {$t(col.category)}
                </span>
              </th>
            {/each}
            <th class="text-right">{$t("Total")}</th>
          </tr>
        </thead>
        <tbody>
          {#each teamCatReport as row (row.user_id)}
            {@const rowTotal = teamCategoryRowTotal(row, catFilteredCategories)}
            <tr>
              <td class="fw-500">{row.name}</td>
              {#each visibleTeamCatColumns as col (col.category)}
                {@const cellMin = teamCategoryMinutes(row, col.category)}
                <td class="tab-num text-right text-tertiary">
                  {cellMin > 0 ? minToHM(cellMin) : "-"}
                </td>
              {/each}
              <td class="tab-num text-right"
                >{rowTotal > 0 ? minToHM(rowTotal) : "-"}</td
              >
            </tr>
          {/each}
        </tbody>
      </DataTable>
    {/if}
  {/if}
</SectionCard>

<SectionCard
  title={$t("Absences")}
  padded={false}
  helpText={$t("help_absence_report")}
  helpOpen={activeHelp === "absence"}
  onHelpToggle={() => toggleHelp("absence")}
>
  {#if absencesLoading && !teamAbsences}
    <LoadingState />
  {:else if teamAbsences}
    {#if teamAbsences.length === 0}
      <div class="zf-card-empty">{$t("No data.")}</div>
    {:else}
      <DataTable>
        <thead>
          <tr>
            <th>{$t("Employee")}</th>
            <th>{$t("Type")}</th>
            <th class="text-right">{$t("From")}</th>
            <th class="text-right">{$t("To")}</th>
            <th class="text-right">{$t("Days")}</th>
            <th>{$t("Status")}</th>
          </tr>
        </thead>
        <tbody>
          {#each teamAbsences as a (a.id)}
            <tr class:entry-rejected={a.status === "rejected"}>
              <td class="fw-500">{userName(a.user_id)}</td>
              <td>{absenceKindLabel(a.kind)}</td>
              <td class="tab-num text-right">{fmtDate(a.start_date)}</td>
              <td class="tab-num text-right">{fmtDate(a.end_date)}</td>
              <td class="tab-num text-right">{formatDayCount(a.days)}</td>
              <td>
                <span class="zf-chip zf-chip-{a.status}"
                  >{statusLabel(a.status)}</span
                >
              </td>
            </tr>
          {/each}
        </tbody>
      </DataTable>
    {/if}
  {/if}
</SectionCard>

<style>
  .team-report-table {
    width: 100%;
  }

  .team-report-table :global(.zf-table) {
    table-layout: auto;
    min-width: 0;
  }

  .team-report-header {
    white-space: normal;
    min-width: 110px;
    max-width: 180px;
    vertical-align: top;
  }

  .col-employee {
    min-width: 140px;
    white-space: nowrap;
  }

  .report-note {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .report-toolbar {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
    flex-wrap: wrap;
  }

  .filter-panel {
    padding: 12px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
    margin-bottom: 12px;
  }

  .filter-options {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .filter-check {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    font-size: 14px;
  }

  .th-cat {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-content: flex-end;
  }

  .cat-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    display: inline-block;
    flex-shrink: 0;
  }
</style>
