<script>
  // The "Employee" tab of the Reports page: one person's balance, category
  // breakdown, entries, absences and flextime chart for the shared toolbar
  // period (month or custom range). Absorbs what used to be three separate
  // cards (Employee report, Category breakdown, Absences) so the same numbers
  // aren't shown twice on the page.
  import { currentUser, settings, toast } from "../../stores.js";
  import {
    t,
    absenceKindLabel,
    statusLabel,
    formatHours,
    formatDayCount,
  } from "../../i18n.js";
  import { isoDate, appTodayDate, minToHM, fmtDate } from "../../format.js";
  import {
    normalizeMonthReport,
    countWorkdays,
    holidayDateSet,
  } from "../../apiMappers.js";
  import Icon from "../../Icons.svelte";
  import FlextimeChart from "../../FlextimeChart.svelte";
  import SectionCard from "../../lib/ui/SectionCard.svelte";
  import StatCard from "../../lib/ui/StatCard.svelte";
  import DataTable from "../../lib/ui/DataTable.svelte";
  import LoadingState from "../../lib/ui/LoadingState.svelte";
  import LeaveAccountCard from "../../lib/ui/LeaveAccountCard.svelte";
  import { hasFlextimeAccount, isAssistantUser } from "../../rolePolicy.js";
  import {
    getFlextimeReport,
    getLeaveBalances,
    getMonthReport,
    getRangeReport,
    getAbsenceReport,
    getUserAbsencesByYear,
    getHolidaysByYear,
  } from "../../lib/api/reportsApi.js";
  import {
    monthEnd,
    monthStart,
    yearsBetweenDates,
  } from "../../lib/domain/dates.js";
  import {
    leaveYearForPeriod,
    timeQueryRange,
  } from "../../lib/domain/reportPeriod.js";
  import {
    absenceKindTotals,
    dedupeAbsences,
  } from "../../lib/domain/reports.js";
  import {
    findUserById,
    userWorkdaysPerWeekById,
  } from "../../lib/domain/users.js";

  export let userId = null;
  export let users = [];
  export let periodMode = "month"; // "month" | "range"
  export let month = "";
  export let from = "";
  export let to = "";

  let today = appTodayDate();
  let todayIso = isoDate(today);
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);

  $: selectedUser = findUserById(users, userId, $currentUser);
  $: isOwnReport = selectedUser?.id === $currentUser?.id;

  let reportData = null;
  let loading = false;
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // --- Absences (always the full selected period — unlike hours/flextime,
  // planned absences are shown even when they fall in the future). ---
  async function loadAbsencesFor(targetUserId, absenceFrom, absenceTo) {
    let raw;
    if (targetUserId === $currentUser?.id) {
      const years = yearsBetweenDates(absenceFrom, absenceTo);
      const lists = await Promise.all(
        years.map((year) => getUserAbsencesByYear(year)),
      );
      raw = lists
        .flat()
        .filter((a) => a.end_date >= absenceFrom && a.start_date <= absenceTo);
    } else {
      // Only reachable when a lead/admin picked another employee — the
      // /absences/all endpoint they call here is lead-only server-side.
      const teamAbsences = await getAbsenceReport({
        from: absenceFrom,
        to: absenceTo,
      });
      raw = (teamAbsences || []).filter((a) => a.user_id === targetUserId);
    }
    raw = dedupeAbsences(raw).filter(
      (a) => a.status !== "rejected" && a.status !== "cancelled",
    );
    if (raw.length === 0) return [];

    const years = [
      ...new Set(
        raw.flatMap((a) => [
          parseInt(a.start_date.slice(0, 4), 10),
          parseInt(a.end_date.slice(0, 4), 10),
        ]),
      ),
    ];
    const holidayLists = await Promise.all(
      years.map((year) => getHolidaysByYear(year)),
    );
    const holidayDates = holidayDateSet(holidayLists.flat());
    const workdaysPerWeek = userWorkdaysPerWeekById(users, targetUserId, 5);

    return raw.map((a) => {
      const clampedFrom =
        a.start_date > absenceFrom ? a.start_date : absenceFrom;
      const clampedTo = a.end_date < absenceTo ? a.end_date : absenceTo;
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
  }

  // --- Time-based data (month report or range report + flextime). ---
  async function loadReportData(id, mode, m, f, t2) {
    const user = findUserById(users, id, $currentUser);
    const isAssist = isAssistantUser(user);
    const flexAccount = hasFlextimeAccount(user);
    const workdaysPerWeek = userWorkdaysPerWeekById(users, id);
    const period = { mode, month: m, from: f, to: t2 };

    if (mode === "month") {
      const absenceFrom = monthStart(m);
      const absenceTo = monthEnd(m);
      const isCurrentMonth =
        m ===
        `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}`;
      const chartFrom = monthStart(m);
      const chartTo = isCurrentMonth ? todayIso : monthEnd(m);
      const canFetchChart = chartFrom <= todayIso;
      const leaveYear = leaveYearForPeriod(period);

      const [monthRaw, leaveRaw, flextimeRaw, absences] = await Promise.all([
        getMonthReport({ userId: id, month: m }),
        getLeaveBalances({ userId: id, year: leaveYear }).catch(() => []),
        canFetchChart && flexAccount
          ? getFlextimeReport({
              userId: id,
              from: chartFrom,
              to: chartTo,
            }).catch(() => [])
          : Promise.resolve([]),
        loadAbsencesFor(id, absenceFrom, absenceTo),
      ]);

      const monthReport = normalizeMonthReport(monthRaw, workdaysPerWeek);
      return {
        periodMode: mode,
        monthReport,
        leaveBalances: Array.isArray(leaveRaw) ? leaveRaw : [],
        flextimeBalance: flextimeRaw.length
          ? flextimeRaw[flextimeRaw.length - 1].cumulative_min
          : null,
        flextimeChartData: flextimeRaw || [],
        absences,
        targetForSub: monthReport.full_month_target_min,
        isFutureOnly: false,
        isAssistant: isAssist,
        hasFlextime: flexAccount,
      };
    }

    // Custom range: hours/flextime are capped at today (no future work data);
    // absences use the full, uncapped range so planned time off still shows.
    const {
      from: rangeFrom,
      to: cappedTo,
      active,
    } = timeQueryRange(period, todayIso);
    const leaveYear = leaveYearForPeriod(period);

    const [rangeRaw, leaveRaw, flextimeRaw, absences] = await Promise.all([
      active
        ? getRangeReport({ userId: id, from: rangeFrom, to: cappedTo })
        : Promise.resolve(null),
      leaveYear
        ? getLeaveBalances({ userId: id, year: leaveYear }).catch(() => [])
        : Promise.resolve([]),
      active && flexAccount
        ? getFlextimeReport({
            userId: id,
            from: rangeFrom,
            to: cappedTo,
          }).catch(() => [])
        : Promise.resolve([]),
      loadAbsencesFor(id, f, t2),
    ]);

    const monthReport = rangeRaw
      ? normalizeMonthReport(rangeRaw, workdaysPerWeek)
      : null;
    return {
      periodMode: mode,
      monthReport,
      leaveBalances: Array.isArray(leaveRaw) ? leaveRaw : [],
      flextimeBalance: flextimeRaw.length
        ? flextimeRaw[flextimeRaw.length - 1].cumulative_min
        : null,
      flextimeChartData: flextimeRaw || [],
      absences,
      targetForSub: monthReport?.target_min ?? null,
      isFutureOnly: !active,
      isAssistant: isAssist,
      hasFlextime: flexAccount,
    };
  }

  // --- Auto-load with a race guard: only the most recent (userId, period)
  // combination's response is ever committed to `reportData`. ---
  function loadKey(id, mode, m, f, t2) {
    if (id == null) return "";
    return mode === "month" ? `${id}:month:${m}` : `${id}:range:${f}:${t2}`;
  }

  let lastLoadKey = "";
  let latestRequestId = 0;

  async function runLoad(key, requestId, id, mode, m, f, t2) {
    loading = true;
    try {
      const data = await loadReportData(id, mode, m, f, t2);
      if (key === lastLoadKey && requestId === latestRequestId) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- reportData isn't read by the triggering $: block, so there's no cycle.
        reportData = data;
      }
    } catch (e) {
      if (key === lastLoadKey && requestId === latestRequestId) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- see above.
        reportData = null;
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (key === lastLoadKey && requestId === latestRequestId) {
        loading = false;
      }
    }
  }

  $: {
    const key = loadKey(userId, periodMode, month, from, to);
    if (key && key !== lastLoadKey) {
      lastLoadKey = key;
      latestRequestId += 1;
      // eslint-disable-next-line svelte/infinite-reactive-loop -- runLoad only writes reportData, which this block never reads.
      runLoad(key, latestRequestId, userId, periodMode, month, from, to);
    } else if (!key && lastLoadKey) {
      lastLoadKey = "";
      reportData = null;
    }
  }

  $: reportAbsenceSummary = reportData
    ? absenceKindTotals(reportData.absences)
    : {};

  // Assistants (Aushilfen) normally have no weekly target, so avoid an
  // artificial target subtext in their time summary.
  $: hideTargetSub =
    reportData?.isAssistant && (reportData.targetForSub || 0) === 0;
</script>

<SectionCard>
  {#if userId == null}
    <div class="zf-card-empty">{$t("No data.")}</div>
  {:else if loading && !reportData}
    <LoadingState />
  {:else if reportData}
    {#if reportData.isFutureOnly}
      <div class="report-note">{$t("future_period_no_time_data")}</div>
    {:else if reportData.monthReport}
      <div class="report-subheading report-subheading-help">
        <span>{isOwnReport ? $t("My Balance") : $t("Balance")}</span>
        <button
          class="zf-btn-icon-sm zf-btn-ghost help-icon"
          title={$t("help_employee_details")}
          on:click={() => toggleHelp("report")}
        >
          <Icon name="Info" size={12} />
        </button>
      </div>
      {#if activeHelp === "report"}
        <div class="report-note">{$t("help_employee_details")}</div>
      {/if}
      <div class="stat-cards mb-16">
        <StatCard
          color={reportData.isAssistant
            ? "var(--text-primary)"
            : reportData.monthReport.submitted_min >=
                (reportData.targetForSub || 0)
              ? "var(--accent)"
              : "var(--warning-text)"}
          sub={hideTargetSub
            ? ""
            : $t("of {target} target", {
                target: formatHours((reportData.targetForSub || 0) / 60),
              })}
        >
          <span slot="label" class="stat-card-label-help">
            <span>{$t("Logged")}</span>
            <button
              class="zf-btn-icon-sm zf-btn-ghost help-icon"
              title={$t("help_logged")}
              on:click={() => toggleHelp("logged")}
            >
              <Icon name="Info" size={12} />
            </button>
          </span>
          {formatHours((reportData.monthReport.submitted_min || 0) / 60)}
        </StatCard>

        {#if reportData.hasFlextime}
          <StatCard
            label={$t("Flextime balance")}
            color={reportData.flextimeBalance === null
              ? "var(--text-tertiary)"
              : reportData.flextimeBalance < 0
                ? "var(--danger-text)"
                : "var(--success-text)"}
          >
            {#if reportData.flextimeBalance !== null}
              {reportData.flextimeBalance >= 0 ? "+" : ""}{minToHM(
                reportData.flextimeBalance,
              )}
            {:else}
              –
            {/if}
          </StatCard>
        {/if}

        {#if periodMode === "month" && !reportData.isAssistant}
          {@const currentWeekStatus =
            reportData.monthReport.current_week_status}
          {@const currentWeekSub =
            currentWeekStatus === "draft"
              ? $t("Current week: draft")
              : currentWeekStatus === "partial"
                ? $t("Current week: partially submitted")
                : currentWeekStatus === "rejected"
                  ? $t("Current week: needs revision")
                  : ""}
          <StatCard
            color={reportData.monthReport.weeks_all_submitted
              ? "var(--success-text)"
              : "var(--warning-text)"}
            sub={currentWeekSub}
          >
            <span slot="label" class="stat-card-label-help">
              <span>{$t("Submissions")}</span>
              <button
                class="zf-btn-icon-sm zf-btn-ghost help-icon"
                title={$t("help_submission_status")}
                on:click={() => toggleHelp("approvals")}
              >
                <Icon name="Info" size={12} />
              </button>
            </span>
            {reportData.monthReport.weeks_all_submitted
              ? $t("All submitted")
              : $t("Weeks missing")}
          </StatCard>
        {/if}
      </div>

      {#if activeHelp === "logged"}
        <div class="report-note">{$t("help_logged")}</div>
      {/if}
      {#if activeHelp === "approvals" && periodMode === "month" && !reportData.isAssistant}
        <div class="report-note">{$t("help_submission_status")}</div>
      {/if}
    {/if}

    {#if reportData.leaveBalances.length > 0}
      <div class="report-subheading">{$t("Leave accounts")}</div>
      <div class="leave-account-cards mb-16">
        {#each reportData.leaveBalances as leaveBalance (leaveBalance.category_id)}
          <LeaveAccountCard
            balance={leaveBalance}
            year={leaveYearForPeriod({ mode: periodMode, month, from, to })}
          />
        {/each}
      </div>
    {/if}

    {#if Object.keys(reportAbsenceSummary).length > 0}
      <div class="report-subheading">{$t("Absences")}</div>
      <div class="stat-cards mb-16">
        {#each Object.entries(reportAbsenceSummary) as [kind, days] (kind)}
          <StatCard
            label={absenceKindLabel(kind)}
            value={formatDayCount(days)}
            sub={$t("days")}
          />
        {/each}
      </div>
    {/if}

    {#if reportData.monthReport?.category_totals && Object.keys(reportData.monthReport.category_totals).length > 0}
      {@const catEntries = Object.entries(
        reportData.monthReport.category_totals,
      ).sort((a, b) => b[1] - a[1])}
      {@const catMax = catEntries[0][1]}
      {@const catTotal = catEntries.reduce((sum, [, mins]) => sum + mins, 0)}
      <SectionCard
        title={$t("Category breakdown")}
        helpText={$t("help_category_breakdown")}
        helpOpen={activeHelp === "cat"}
        onHelpToggle={() => toggleHelp("cat")}
      >
        <div class="cat-bars">
          {#each catEntries as [cat, mins] (cat)}
            <div class="cat-bar-row">
              <span class="cat-bar-label" title={$t(cat)}>{$t(cat)}</span>
              <div class="cat-bar-track">
                <div
                  class="cat-bar-fill"
                  style:width={(catMax > 0
                    ? Math.round((mins / catMax) * 100)
                    : 0) + "%"}
                ></div>
              </div>
              <span class="tab-num text-tertiary text-right"
                >{minToHM(mins)}</span
              >
              <span class="tab-num text-tertiary text-right cat-bar-pct">
                {catTotal > 0 ? Math.round((mins / catTotal) * 100) : 0}%
              </span>
            </div>
          {/each}
        </div>
      </SectionCard>
    {/if}

    {#if reportData.monthReport?.entries?.length}
      <SectionCard title={$t("Entries")} padded={false}>
        <DataTable>
          <thead>
            <tr>
              <th>{$t("Date")}</th>
              <th>{$t("Start")}</th>
              <th>{$t("End")}</th>
              <th>{$t("Duration")}</th>
              <th>{$t("Category")}</th>
              <th>{$t("Comment")}</th>
              <th>{$t("Status")}</th>
            </tr>
          </thead>
          <tbody>
            {#each reportData.monthReport.entries as e, i (`${e.entry_date}-${e.start_time}-${e.end_time}-${i}`)}
              <tr class:entry-rejected={e.status === "rejected"}>
                <td class="tab-num">{fmtDate(e.entry_date)}</td>
                <td class="tab-num">{e.start_time?.slice(0, 5)}</td>
                <td class="tab-num">{e.end_time?.slice(0, 5)}</td>
                <td class="tab-num">{minToHM(e.minutes || 0)}</td>
                <td>{e.category_name ? $t(e.category_name) : "-"}</td>
                <td>
                  {#if e.comment}
                    <span class="text-truncate-tooltip" title={e.comment}>
                      {e.comment}
                    </span>
                  {:else}
                    -
                  {/if}
                </td>
                <td>
                  <span class="zf-chip zf-chip-{e.status}"
                    >{statusLabel(e.status)}</span
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </DataTable>
      </SectionCard>
    {/if}

    {#if reportData.absences?.length}
      <SectionCard
        title={$t("Absences")}
        padded={false}
        helpText={$t("help_absence_report")}
        helpOpen={activeHelp === "absence"}
        onHelpToggle={() => toggleHelp("absence")}
      >
        <DataTable>
          <thead>
            <tr>
              <th>{$t("Type")}</th>
              <th class="text-right">{$t("From")}</th>
              <th class="text-right">{$t("To")}</th>
              <th class="text-right">{$t("Days")}</th>
              <th>{$t("Status")}</th>
            </tr>
          </thead>
          <tbody>
            {#each reportData.absences as a (a.id)}
              <tr>
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
      </SectionCard>
    {/if}

    {#if reportData.hasFlextime && reportData.flextimeChartData?.length}
      <SectionCard
        title={$t("Flextime balance")}
        helpText={$t("help_flextime_chart")}
      >
        <FlextimeChart data={reportData.flextimeChartData} />
      </SectionCard>
    {/if}
  {/if}
</SectionCard>

<style>
  .report-subheading {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 6px;
  }

  .leave-account-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 12px;
  }

  .report-subheading-help {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .help-icon {
    color: var(--text-tertiary);
    font-size: 13px;
    cursor: help;
  }

  .report-note {
    font-size: 13px;
    color: var(--text-tertiary);
    margin-top: -6px;
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .cat-bars {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .cat-bar-row {
    display: grid;
    grid-template-columns: 130px 1fr 52px 40px;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }

  .cat-bar-label {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cat-bar-track {
    background: var(--bg-muted);
    border-radius: 3px;
    height: 8px;
    overflow: hidden;
  }

  .cat-bar-fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.3s;
  }

  .cat-bar-pct {
    opacity: 0.8;
  }
</style>
