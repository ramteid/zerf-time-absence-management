<script>
  import { onDestroy, onMount, tick } from "svelte";
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
  import {
    isoDate,
    appTodayDate,
    addDays,
    minToHM,
    fmtDate,
  } from "../../format.js";
  import {
    normalizeMonthReport,
    countWorkdays,
    holidayDateSet,
  } from "../../apiMappers.js";
  import Icon from "../../Icons.svelte";
  import FlextimeChart from "../../FlextimeChart.svelte";
  import FlextimeRangeControls from "../dashboard/FlextimeRangeControls.svelte";
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
    isReportRangeTooLong,
  } from "../../lib/domain/dates.js";
  import {
    leaveYearForPeriod,
    timeQueryRange,
  } from "../../lib/domain/reportPeriod.js";
  import {
    absenceKindTotals,
    dedupeAbsences,
  } from "../../lib/domain/reports.js";
  import { findUserById, userWorkdaysPerWeek } from "../../lib/domain/users.js";

  export let userId = null;
  export let users = [];
  export let periodMode = "month"; // "month" | "range"
  export let month = "";
  export let from = "";
  export let to = "";
  export let navigationKey = "";

  let today = appTodayDate();
  let todayIso = isoDate(today);
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);

  $: selectedUser = findUserById(users, userId, $currentUser);
  $: isOwnReport = selectedUser?.id === $currentUser?.id;

  let reportData = null;
  let loading = false;
  let activeHelp = null;
  let entriesSection;
  let absencesSection;
  let reportHash = typeof window === "undefined" ? "" : window.location.hash;
  let hashVersion = 0;
  let lastFocusedSectionKey = "";
  let focusRequestId = 0;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // --- Absences (always the full selected period — unlike hours/flextime,
  // planned absences are shown even when they fall in the future). ---
  async function loadAbsencesFor(
    targetUserId,
    absenceFrom,
    absenceTo,
    workdaysPerWeek,
  ) {
    // A custom range with no sane upper bound (picked via the calendar, or
    // supplied unvalidated through a "View in report" deep link) would
    // otherwise expand into one API call per calendar year below — capping
    // here keeps that expansion bounded instead of flooding the API.
    if (isReportRangeTooLong(absenceFrom, absenceTo)) {
      toast($t("report_range_too_long"), "error");
      return [];
    }
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

  // Shape returned by `getFlextimeReport`, used wherever the request is
  // skipped or failed so callers never have to null-check.
  function emptyFlextime() {
    return { days: [], balanceAsOf: null };
  }

  // Closing balance of a flextime ledger: the last day's cumulative value.
  function closingBalance(days) {
    return days.length ? days[days.length - 1].cumulative_min : null;
  }

  // --- Time-based data (month report or range report + flextime). ---
  async function loadReportData(id, user, mode, m, f, t2) {
    const isAssist = isAssistantUser(user);
    const flexAccount = hasFlextimeAccount(user);
    const workdaysPerWeek = userWorkdaysPerWeek(user);
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

      const [monthRaw, leaveRaw, flextime, absences] = await Promise.all([
        getMonthReport({ userId: id, month: m }),
        getLeaveBalances({ userId: id, year: leaveYear }).catch(() => []),
        canFetchChart && flexAccount
          ? getFlextimeReport({
              userId: id,
              from: chartFrom,
              to: chartTo,
            }).catch(() => emptyFlextime())
          : Promise.resolve(emptyFlextime()),
        loadAbsencesFor(id, absenceFrom, absenceTo, workdaysPerWeek),
      ]);

      const monthReport = normalizeMonthReport(monthRaw, workdaysPerWeek);
      return {
        periodMode: mode,
        monthReport,
        leaveBalances: Array.isArray(leaveRaw) ? leaveRaw : [],
        flextimeBalance: closingBalance(flextime.days),
        flextimeChartData: flextime.days,
        flextimeBalanceAsOf: flextime.balanceAsOf,
        chartRange: { from: chartFrom, to: chartTo },
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

    const [rangeRaw, leaveRaw, flextime, absences] = await Promise.all([
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
          }).catch(() => emptyFlextime())
        : Promise.resolve(emptyFlextime()),
      loadAbsencesFor(id, f, t2, workdaysPerWeek),
    ]);

    const monthReport = rangeRaw
      ? normalizeMonthReport(rangeRaw, workdaysPerWeek)
      : null;
    return {
      periodMode: mode,
      monthReport,
      leaveBalances: Array.isArray(leaveRaw) ? leaveRaw : [],
      flextimeBalance: closingBalance(flextime.days),
      flextimeChartData: flextime.days,
      flextimeBalanceAsOf: flextime.balanceAsOf,
      chartRange: { from: rangeFrom, to: cappedTo },
      absences,
      targetForSub: monthReport?.target_min ?? null,
      isFutureOnly: !active,
      isAssistant: isAssist,
      hasFlextime: flexAccount,
    };
  }

  // --- Auto-load with a race guard: only the most recent (user, period)
  // combination's response is ever committed to `reportData`. ---
  // An empty key means "not ready" and suppresses the fetch entirely. The
  // period arrives one reactive pass after the component mounts, so without
  // these guards the first pass would fire a load for a blank month/range and
  // query nonsense bounds.
  function reportUserKey(id, user) {
    if (id == null || Number(user?.id) !== Number(id)) return "";
    // The report derives workday counts, assistant status and flextime access
    // locally, while the backend derives targets and balances from the same
    // metadata. Keep the complete report-relevant snapshot in the key so an
    // updated roster cannot leave a report backed by old assumptions.
    return [
      id,
      user.role || "",
      user.workdays_per_week ?? "",
      user.weekly_hours ?? "",
      user.start_date ?? "",
      user.hire_date ?? "",
      user.overtime_start_balance_min ?? "",
      user.tracks_time !== false,
      user.active !== false,
    ].join(":");
  }

  function loadKey(id, user, mode, m, f, t2) {
    const userKey = reportUserKey(id, user);
    if (!userKey) return "";
    if (mode === "month") return m ? `${userKey}:month:${m}` : "";
    return f && t2 ? `${userKey}:range:${f}:${t2}` : "";
  }

  let lastLoadKey = "";
  let loadedReportKey = "";
  let latestRequestId = 0;

  async function runLoad(key, requestId, id, user, mode, m, f, t2) {
    loading = true;
    try {
      const data = await loadReportData(id, user, mode, m, f, t2);
      if (key === lastLoadKey && requestId === latestRequestId) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- reportData isn't read by the triggering $: block, so there's no cycle.
        reportData = data;
        // eslint-disable-next-line svelte/infinite-reactive-loop -- this key is read only by the focus effect, not the loader.
        loadedReportKey = key;
      }
    } catch (e) {
      if (key === lastLoadKey && requestId === latestRequestId) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- see above.
        reportData = null;
        // eslint-disable-next-line svelte/infinite-reactive-loop -- see the successful-load assignment above.
        loadedReportKey = "";
        toast($t(e?.message || "Error"), "error");
      }
    } finally {
      if (key === lastLoadKey && requestId === latestRequestId) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- loading isn't read by the triggering $: block.
        loading = false;
      }
    }
  }

  $: {
    const key = loadKey(userId, selectedUser, periodMode, month, from, to);
    if (key && key !== lastLoadKey) {
      lastLoadKey = key;
      latestRequestId += 1;
      // Never render data for a previous person or period while its successor
      // is loading. Besides avoiding stale numbers, this prevents a deep-link
      // fragment from focusing a section belonging to the prior report.
      reportData = null;
      loadedReportKey = "";
      // eslint-disable-next-line svelte/infinite-reactive-loop -- runLoad only writes reportData, which this block never reads.
      runLoad(
        key,
        latestRequestId,
        userId,
        selectedUser,
        periodMode,
        month,
        from,
        to,
      );
    } else if (!key && lastLoadKey) {
      lastLoadKey = "";
      reportData = null;
      loadedReportKey = "";
      loading = false;
    }
  }

  // --- Flextime chart: its own range, independent of the report period. ---
  // Seeded from the period each time a report finishes loading, then steerable
  // with the same quick ranges the dashboard chart offers.
  let chartFrom = "";
  let chartTo = "";
  let chartDays = [];
  let chartBalanceAsOf = null;
  let chartLoading = false;
  let chartRequestId = 0;
  let lastSyncedReport = null;

  // Re-seed whenever a new report object arrives (new user or new period), so
  // the chart starts out showing exactly the period the report describes and
  // reuses the ledger that load already fetched.
  function syncChartWithReport(data) {
    if (data === lastSyncedReport) return;
    lastSyncedReport = data;
    // A manually requested chart may still be in flight while the selected
    // report changes. Its response belongs to the old person or period and
    // must not overwrite the chart reseeded from the new report.
    chartRequestId += 1;
    chartLoading = false;
    chartFrom = data?.chartRange?.from ?? "";
    chartTo = data?.chartRange?.to ?? "";
    chartDays = data?.flextimeChartData ?? [];
    chartBalanceAsOf = data?.flextimeBalanceAsOf ?? null;
  }

  $: syncChartWithReport(reportData);

  function syncReportHash() {
    const nextHash = typeof window === "undefined" ? "" : window.location.hash;
    if (nextHash === reportHash) return;
    reportHash = nextHash;
    hashVersion += 1;
  }

  function sectionForHash(hash) {
    if (hash === "#report-entries") return entriesSection;
    if (hash === "#report-absences") return absencesSection;
    return null;
  }

  function focusLinkedSection(data, dataKey, hash, version, navigation) {
    // Invalidate every queued focus before inspecting the next state. This is
    // important even when the new hash is unknown or data is loading: a prior
    // tick callback must not focus an obsolete report section afterwards.
    const requestId = ++focusRequestId;
    const currentHash =
      typeof window === "undefined" ? hash : window.location.hash;
    if (
      !data ||
      !dataKey ||
      dataKey !== lastLoadKey ||
      (currentHash !== "#report-entries" && currentHash !== "#report-absences")
    )
      return;
    const focusKey = `${navigation}:${dataKey}:${currentHash}:${version}`;
    if (focusKey === lastFocusedSectionKey) return;
    lastFocusedSectionKey = focusKey;
    tick().then(() => {
      if (
        requestId !== focusRequestId ||
        focusKey !== lastFocusedSectionKey ||
        dataKey !== loadedReportKey ||
        dataKey !== lastLoadKey ||
        navigation !== navigationKey ||
        (typeof window !== "undefined" && window.location.hash !== currentHash)
      )
        return;
      const target = sectionForHash(currentHash);
      if (!target) return;
      target.scrollIntoView?.({ block: "start" });
      target.focus({ preventScroll: true });
    });
  }

  onMount(() => {
    syncReportHash();
    window.addEventListener("hashchange", syncReportHash);
    window.addEventListener("popstate", syncReportHash);
    return () => {
      window.removeEventListener("hashchange", syncReportHash);
      window.removeEventListener("popstate", syncReportHash);
    };
  });

  onDestroy(() => {
    // Detached reports must not receive delayed focus or async updates.
    latestRequestId += 1;
    focusRequestId += 1;
    chartRequestId += 1;
  });

  $: focusLinkedSection(
    reportData,
    loadedReportKey,
    reportHash,
    hashVersion,
    navigationKey,
  );

  async function loadChart() {
    if (
      userId == null ||
      !chartFrom ||
      !chartTo ||
      chartFrom > chartTo ||
      !loadedReportKey
    )
      return;
    const requestId = ++chartRequestId;
    const requestedUserId = userId;
    const requestedFrom = chartFrom;
    const requestedTo = chartTo;
    const requestedReportKey = loadedReportKey;
    chartLoading = true;
    try {
      const flextime = await getFlextimeReport({
        userId: requestedUserId,
        from: requestedFrom,
        to: requestedTo,
      });
      if (
        !isCurrentChartRequest(
          requestId,
          requestedUserId,
          requestedFrom,
          requestedTo,
          requestedReportKey,
        )
      )
        return;
      chartDays = flextime.days;
      chartBalanceAsOf = flextime.balanceAsOf;
    } catch (e) {
      if (
        !isCurrentChartRequest(
          requestId,
          requestedUserId,
          requestedFrom,
          requestedTo,
          requestedReportKey,
        )
      )
        return;
      toast($t(e?.message || "Error"), "error");
    } finally {
      // Keep the spinner tied to request identity rather than the mutable
      // chart inputs. Editing dates invalidates a response but does not start
      // a successor, so the same request must still clear its own spinner.
      // A newer request or report reset increments chartRequestId first.
      if (requestId === chartRequestId) chartLoading = false;
    }
  }

  function isCurrentChartRequest(
    requestId,
    requestedUserId,
    requestedFrom,
    requestedTo,
    requestedReportKey,
  ) {
    return (
      requestId === chartRequestId &&
      requestedUserId === userId &&
      requestedFrom === chartFrom &&
      requestedTo === chartTo &&
      requestedReportKey === loadedReportKey &&
      requestedReportKey === lastLoadKey
    );
  }

  // Quick range: the last `days` days ending today, clamped to the selected
  // employee's start date so the chart never opens before their employment.
  function setChartRange(days) {
    const start = isoDate(addDays(today, -(days - 1)));
    const employeeStart = selectedUser?.start_date;
    chartFrom = employeeStart && employeeStart > start ? employeeStart : start;
    chartTo = todayIso;
    loadChart();
  }

  // Date the stat card's balance refers to: the report period ends where it
  // ends, but the balance itself stops at the flextime cutoff, so the earlier
  // of the two is what the number actually describes.
  $: statBalanceAsOf =
    reportData?.flextimeBalanceAsOf && reportData?.chartRange?.to
      ? reportData.flextimeBalanceAsOf < reportData.chartRange.to
        ? reportData.flextimeBalanceAsOf
        : reportData.chartRange.to
      : (reportData?.flextimeBalanceAsOf ?? null);

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
      <!-- The contracted weekly hours put the numbers below into context, so
           they get their own line under the heading rather than trailing it. -->
      {#if selectedUser?.weekly_hours && selectedUser.weekly_hours > 0}
        <div class="weekly-hours-tag">
          {$t("Weekly hours")}: {formatHours(selectedUser.weekly_hours)}
        </div>
      {/if}
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
            sub={statBalanceAsOf
              ? $t("As of {date}", { date: fmtDate(statBalanceAsOf) })
              : ""}
            color={reportData.flextimeBalance === null
              ? "var(--text-tertiary)"
              : reportData.flextimeBalance < 0
                ? "var(--danger-text)"
                : "var(--success-text)"}
          >
            {#if reportData.flextimeBalance !== null}
              <!-- Route the signed HH:MM balance through formatHours so it
                   carries the hours unit ("Std.") like every other hour tile
                   (dashboard overtime, logged hours). formatHours passes a
                   pre-formatted string through untouched and just appends the
                   unit. -->
              {formatHours(
                (reportData.flextimeBalance >= 0 ? "+" : "") +
                  minToHM(reportData.flextimeBalance),
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
      <div class="stat-cards absence-stat-cards mb-16">
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
      <div
        id="report-entries"
        class="report-focus-target"
        role="region"
        aria-label={$t("Entries")}
        tabindex="-1"
        bind:this={entriesSection}
      >
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
      </div>
    {/if}

    {#if reportData.absences?.length}
      <div
        id="report-absences"
        class="report-focus-target"
        role="region"
        aria-label={$t("Absences")}
        tabindex="-1"
        bind:this={absencesSection}
      >
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
                <th>{$t("Comment")}</th>
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
                    {#if a.comment}
                      <span class="report-absence-comment">
                        {a.comment}
                      </span>
                    {:else}
                      -
                    {/if}
                  </td>
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
      </div>
    {/if}

    {#if reportData.hasFlextime}
      <SectionCard
        title={$t("Flextime balance")}
        helpText={$t("help_flextime_chart")}
        actionsOwnRow
      >
        <!-- The chart has its own range, seeded from the report period: the
             balance curve is usually worth looking at over a longer window
             than the month currently being reported on. -->
        <FlextimeRangeControls
          slot="actions"
          bind:from={chartFrom}
          bind:to={chartTo}
          {todayIso}
          minDate={selectedUser?.start_date}
          onSetRange={setChartRange}
          onLoad={loadChart}
        />
        {#if chartBalanceAsOf}
          <div class="chart-as-of">
            {$t("As of {date}", { date: fmtDate(chartBalanceAsOf) })}
          </div>
        {/if}
        {#if chartLoading && chartDays.length === 0}
          <div class="zf-card-empty">{$t("Loading...")}</div>
        {:else if chartDays.length}
          <FlextimeChart data={chartDays} asOf={chartBalanceAsOf} />
        {:else}
          <div class="zf-card-empty">{$t("No data.")}</div>
        {/if}
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

  /* Unlike the balance stat-cards above, the number of absence kinds varies
     (often just one), so cards must not grow to fill the row — a lone card
     would otherwise stretch to the full container width. */
  .absence-stat-cards :global(.stat-card) {
    flex: 0 1 auto;
  }

  .report-focus-target {
    scroll-margin-top: 16px;
  }

  /* The report is the canonical destination for absence remarks. Unlike the
     compact time-entry list, a remark must stay readable without relying on a
     hover-only native tooltip. */
  .report-absence-comment {
    display: block;
    color: var(--text-tertiary);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .leave-account-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 12px;
  }

  .report-subheading-help {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .weekly-hours-tag {
    font-size: 0.75rem;
    color: var(--text-secondary);
    font-weight: 400;
    margin-bottom: 10px;
  }

  .help-icon {
    color: var(--text-tertiary);
    font-size: 13px;
    cursor: help;
  }

  /* Stichtag line above the flextime chart: the curve is flat after this date
     because nothing beyond it is approved yet. */
  .chart-as-of {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin-bottom: 10px;
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
