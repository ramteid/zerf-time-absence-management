<script>
  import { onDestroy } from "svelte";
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
  import {
    yearsBetweenDates,
    isReportRangeTooLong,
  } from "../../lib/domain/dates.js";
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
  import { findUserById, userWorkdaysPerWeek } from "../../lib/domain/users.js";

  export let users = [];
  export let periodMode = "month";
  export let month = "";
  export let from = "";
  export let to = "";

  let mounted = true;

  onDestroy(() => {
    // All three loaders use these generations as cancellation tokens. A late
    // completion must not update detached component state or surface a toast.
    mounted = false;
    latestTeamRequestId += 1;
    latestCategoryRequestId += 1;
    latestAbsenceRequestId += 1;
  });

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
  let latestTeamRequestId = 0;

  // Group rank for the team report: employees and team leads share rank 0 (one
  // combined group at the top), assistants rank 1, admins rank 2. Unknown roles
  // sort last. This differs from the global ROLE_ORDER where team_lead < employee;
  // here they are intentionally merged into a single group.
  function teamReportRoleRank(userId) {
    const role = users.find((u) => u.id === userId)?.role;
    if (role === "employee" || role === "team_lead") return 0;
    if (role === "assistant") return 1;
    if (role === "admin") return 2;
    return 3;
  }

  $: sortedTeamReport = teamReport
    ? [...teamReport].sort((a, b) => {
        const rankDiff =
          teamReportRoleRank(a.user_id) - teamReportRoleRank(b.user_id);
        if (rankDiff !== 0) return rankDiff;
        return a.name.localeCompare(b.name);
      })
    : null;

  function isCurrentTeamRequest(key, requestId) {
    return mounted && key === lastTeamKey && requestId === latestTeamRequestId;
  }

  async function loadTeam(key, requestId, requestedMonth) {
    try {
      const loaded = await getTeamReport({ month: requestedMonth });
      if (isCurrentTeamRequest(key, requestId)) {
        teamReport = loaded?.rows || [];
        teamLeaveAccountColumns = loaded?.leave_account_categories || [];
      }
    } catch (e) {
      if (isCurrentTeamRequest(key, requestId)) {
        teamReport = null;
        teamLeaveAccountColumns = [];
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (isCurrentTeamRequest(key, requestId)) teamLoading = false;
    }
  }

  function startTeamLoad(key, requestedMonth) {
    lastTeamKey = key;
    latestTeamRequestId += 1;
    teamReport = null;
    teamLeaveAccountColumns = [];
    teamLoading = true;
    loadTeam(key, latestTeamRequestId, requestedMonth);
  }

  function clearTeamLoad() {
    lastTeamKey = "";
    latestTeamRequestId += 1;
    teamReport = null;
    teamLeaveAccountColumns = [];
    teamLoading = false;
  }

  $: {
    const key = periodMode === "month" && month ? `month:${month}` : "";
    if (key && key !== lastTeamKey) {
      startTeamLoad(key, month);
    } else if (!key && lastTeamKey) {
      clearTeamLoad();
    }
  }

  // --- Section 2: category matrix ---
  let teamCatReport = null;
  let catFilteredCategories = [];
  let catShowFilter = false;
  let catLoading = false;
  let lastCatKey = "";
  let latestCategoryRequestId = 0;

  function isCurrentCategoryRequest(key, requestId) {
    return (
      mounted && key === lastCatKey && requestId === latestCategoryRequestId
    );
  }

  async function loadCategories(key, requestId, catFrom, catTo) {
    try {
      const loaded = await getTeamCategoryReport({ from: catFrom, to: catTo });
      if (isCurrentCategoryRequest(key, requestId)) {
        teamCatReport = (loaded || []).sort((a, b) =>
          a.name.localeCompare(b.name),
        );
        catFilteredCategories = categoryColumnsFromTeamReport(
          teamCatReport,
        ).map((c) => c.category);
        catShowFilter = false;
      }
    } catch (e) {
      if (isCurrentCategoryRequest(key, requestId)) {
        teamCatReport = null;
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (isCurrentCategoryRequest(key, requestId)) catLoading = false;
    }
  }

  function startCategoryLoad(key, catFrom, catTo) {
    lastCatKey = key;
    latestCategoryRequestId += 1;
    teamCatReport = null;
    catFilteredCategories = [];
    catShowFilter = false;
    catLoading = true;
    loadCategories(key, latestCategoryRequestId, catFrom, catTo);
  }

  function clearCategoryLoad() {
    lastCatKey = "";
    latestCategoryRequestId += 1;
    teamCatReport = null;
    catFilteredCategories = [];
    catShowFilter = false;
    catLoading = false;
  }

  $: catBounds = periodBounds({ mode: periodMode, month, from, to });
  $: {
    const key = `${catBounds.from}:${catBounds.to}`;
    if (catBounds.from && catBounds.to && key !== lastCatKey) {
      startCategoryLoad(key, catBounds.from, catBounds.to);
    } else if ((!catBounds.from || !catBounds.to) && lastCatKey) {
      clearCategoryLoad();
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
  // Derived from the raw response plus the roster (see absenceRowsForRoster).
  let teamAbsences;
  let teamAbsenceData = null;
  let absencesLoading = false;
  let lastAbsenceKey = "";
  let latestAbsenceRequestId = 0;

  function isCurrentAbsenceRequest(key, requestId) {
    return (
      mounted && key === lastAbsenceKey && requestId === latestAbsenceRequestId
    );
  }

  function absenceRowsForRoster(data, roster) {
    if (!data) return null;
    if (data.raw.length === 0) return [];

    const matchedUsers = data.raw.map((absence) =>
      findUserById(roster, absence.user_id),
    );
    // The API response and roster are loaded independently. Rendering with a
    // five-day fallback before the roster arrives produces incorrect leave
    // totals for part-time staff, so wait until every row has its metadata.
    if (matchedUsers.some((user) => !user)) return null;

    return data.raw.map((absence, index) => {
      const clampedFrom =
        absence.start_date > data.from ? absence.start_date : data.from;
      const clampedTo = absence.end_date < data.to ? absence.end_date : data.to;
      const days =
        clampedTo < clampedFrom
          ? 0
          : countWorkdays(
              clampedFrom,
              clampedTo,
              data.holidayDates,
              userWorkdaysPerWeek(matchedUsers[index]),
            );
      return { ...absence, days };
    });
  }

  $: teamAbsences = absenceRowsForRoster(teamAbsenceData, users);
  $: waitingForAbsenceRoster =
    !!teamAbsenceData &&
    teamAbsenceData.raw.length > 0 &&
    teamAbsences === null;

  async function loadAbsences(key, requestId, absenceFrom, absenceTo) {
    // See PersonReport's identical guard: an unbounded custom range would
    // otherwise expand into one API call per calendar year further down.
    if (isReportRangeTooLong(absenceFrom, absenceTo)) {
      if (isCurrentAbsenceRequest(key, requestId)) {
        teamAbsenceData = {
          raw: [],
          holidayDates: new Set(),
          from: absenceFrom,
          to: absenceTo,
        };
        toast($t("report_range_too_long"), "error");
        absencesLoading = false;
      }
      return;
    }
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
        if (isCurrentAbsenceRequest(key, requestId)) {
          teamAbsenceData = {
            raw: [],
            holidayDates: new Set(),
            from: absenceFrom,
            to: absenceTo,
          };
        }
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
      if (isCurrentAbsenceRequest(key, requestId)) {
        teamAbsenceData = {
          raw,
          holidayDates,
          from: absenceFrom,
          to: absenceTo,
        };
      }
    } catch (e) {
      if (isCurrentAbsenceRequest(key, requestId)) {
        teamAbsenceData = null;
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (isCurrentAbsenceRequest(key, requestId)) absencesLoading = false;
    }
  }

  function startAbsenceLoad(key, absenceFrom, absenceTo) {
    lastAbsenceKey = key;
    latestAbsenceRequestId += 1;
    teamAbsenceData = null;
    absencesLoading = true;
    loadAbsences(key, latestAbsenceRequestId, absenceFrom, absenceTo);
  }

  function clearAbsenceLoad() {
    lastAbsenceKey = "";
    latestAbsenceRequestId += 1;
    teamAbsenceData = null;
    absencesLoading = false;
  }

  $: {
    const key = `${catBounds.from}:${catBounds.to}`;
    if (catBounds.from && catBounds.to && key !== lastAbsenceKey) {
      startAbsenceLoad(key, catBounds.from, catBounds.to);
    } else if ((!catBounds.from || !catBounds.to) && lastAbsenceKey) {
      clearAbsenceLoad();
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
  {:else if teamLoading && !sortedTeamReport}
    <LoadingState />
  {:else if sortedTeamReport}
    <div class="team-report-table">
      <DataTable fit>
        <thead>
          <tr>
            <th class="col-employee team-report-header">{$t("Employee")}</th>
            <th class="text-right team-report-header"
              >{$t("Flextime balance")}</th
            >
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
            <th class="text-center team-report-header"
              >{$t("All weeks submitted")}</th
            >
          </tr>
        </thead>
        <tbody>
          {#each sortedTeamReport as r (r.user_id)}
            <!-- The monthly diff sits next to the flextime balance, so it has
                 to describe the same movement: worked-minus-target plus any
                 admin booking dated in this month. -->
            {@const monthlyDiffMin =
              r.diff_min == null ? null : r.diff_min + (r.adjustment_min || 0)}
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
                  <!-- The balance stops at the end of this person's last fully
                     approved week, which is rarely the month's last day. -->
                  {#if r.flextime_balance_as_of}
                    <span class="balance-as-of"
                      >{fmtDate(r.flextime_balance_as_of)}</span
                    >
                  {/if}
                {/if}
              </td>
              <td
                class="tab-num text-right"
                style:color={monthlyDiffMin == null
                  ? "var(--text-tertiary)"
                  : monthlyDiffMin < 0
                    ? "var(--danger-text)"
                    : "var(--success-text)"}
              >
                {#if monthlyDiffMin == null}
                  -
                {:else}
                  {monthlyDiffMin >= 0 ? "+" : ""}{minToHM(monthlyDiffMin)}
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
  {#if (absencesLoading || waitingForAbsenceRoster) && !teamAbsences}
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
  /* Date under a flextime balance: which day the number is stated as of. */
  .balance-as-of {
    display: block;
    font-size: 0.8125rem;
    font-weight: 400;
    color: var(--text-tertiary);
  }

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
