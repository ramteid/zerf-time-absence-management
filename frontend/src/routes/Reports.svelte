<script>
  // Reports page: an Employee/Team scope switch sharing one filter toolbar
  // (person, period) instead of five independently-filtered cards. Exports
  // act on whatever is currently shown ("export what you see").
  import {
    currentUser,
    earliestStartDate,
    settings,
    toast,
    path,
  } from "../stores.js";
  import { t } from "../i18n.js";
  import { tracksOwnTime } from "../rolePolicy.js";
  import { isoDate, appTodayDate, addDays } from "../format.js";
  import Icon from "../Icons.svelte";
  import PeriodPicker from "../lib/ui/PeriodPicker.svelte";
  import PersonReport from "./reports/PersonReport.svelte";
  import TeamReport from "./reports/TeamReport.svelte";
  import {
    getUsersForReports,
    getRangeReport,
    getFlextimeReport,
    getTimesheetPdf,
  } from "../lib/api/reportsApi.js";
  import { findUserById, hasUserId } from "../lib/domain/users.js";
  import { timeQueryRange } from "../lib/domain/reportPeriod.js";
  import { isReportRangeTooLong } from "../lib/domain/dates.js";
  import {
    buildTimesheetCsv,
    timesheetCsvBlob,
    downloadBlob,
    safeFileNamePart,
  } from "../lib/domain/timesheetCsv.js";

  // Leads and admins load all users for the dropdown. Other roles only see
  // their own data.
  let users = [];
  let lastUsersLoadKey = "";
  let latestUsersLoadRequest = 0;

  function usersLoadKey(user) {
    return user?.id
      ? `${user.id}:${!!user?.permissions?.can_view_team_reports}:${user.tracks_time !== false}`
      : "";
  }

  async function initUsers(loadKey, requestId, user) {
    try {
      const canTeam = !!user?.permissions?.can_view_team_reports;
      if (!user?.id) {
        if (
          loadKey === lastUsersLoadKey &&
          requestId === latestUsersLoadRequest
        )
          users = [];
        return;
      }
      const loadedUsers = await getUsersForReports(canTeam, user);
      if (
        loadKey === lastUsersLoadKey &&
        requestId === latestUsersLoadRequest
      ) {
        users = loadedUsers;
      }
    } catch (e) {
      if (
        loadKey === lastUsersLoadKey &&
        requestId === latestUsersLoadRequest
      ) {
        toast($t(e?.message || "Error"), "error");
      }
    }
  }

  $: canViewTeamReports = !!$currentUser?.permissions?.can_view_team_reports;
  $: {
    const user = $currentUser;
    const loadKey = usersLoadKey(user);
    if (loadKey !== lastUsersLoadKey) {
      lastUsersLoadKey = loadKey;
      latestUsersLoadRequest += 1;
      initUsers(loadKey, latestUsersLoadRequest, user);
    }
  }
  $: currentUserTracksTime = tracksOwnTime($currentUser);
  $: isSelfOnlyReportsView = !canViewTeamReports && currentUserTracksTime;

  // --- Shared filter state ---
  let today = appTodayDate();
  let todayIso = isoDate(today);
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);
  $: currentMonthStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  // Custom-range upper bound: keeps the calendar from reaching a date so far
  // out that the absence/holiday lookups below (one API call per calendar
  // year in the selected range) would balloon into a request flood.
  $: maxDate = isoDate(addDays(today, 366));

  let activeTab = "employee"; // "employee" | "team"
  let periodMode = "month";
  let month = currentMonthStr;
  let from = "";
  let to = "";
  let selectedUserId = tracksOwnTime($currentUser) ? $currentUser.id : null;

  // Deep-link support: when navigated here from a pending approval's
  // WeekReviewDialog ("View in report"), the URL carries user/from/to query
  // params. Apply them once per navigation to preselect the person, force a
  // custom date range covering that week, and switch to the Employee tab.
  // Reports never calls go() for its own filter changes, so $path stays put
  // afterwards and this won't fight manual filter edits made later.
  function applyDeepLink(currentPath) {
    const queryString = currentPath.includes("?")
      ? currentPath.split("?")[1]
      : "";
    const params = new URLSearchParams(queryString);
    const user = params.get("user");
    const fromParam = params.get("from");
    const toParam = params.get("to");
    const isoDatePattern = /^\d{4}-\d{2}-\d{2}$/;
    if (
      user &&
      fromParam &&
      toParam &&
      isoDatePattern.test(fromParam) &&
      isoDatePattern.test(toParam) &&
      fromParam <= toParam &&
      !isReportRangeTooLong(fromParam, toParam)
    ) {
      selectedUserId = Number(user);
      periodMode = "range";
      from = fromParam;
      to = toParam;
      activeTab = "employee";
    }
  }

  $: if ($path.startsWith("/reports?")) applyDeepLink($path);

  $: if (isSelfOnlyReportsView) selectedUserId = $currentUser.id;
  $: if (
    !isSelfOnlyReportsView &&
    (selectedUserId == null || !hasUserId(users, selectedUserId)) &&
    users.length > 0
  ) {
    selectedUserId = users[0].id;
  }
  $: if (!canViewTeamReports) activeTab = "employee";

  $: selectedReportUser = findUserById(users, selectedUserId, $currentUser);

  // Month/date lower bound depends on the active tab: an individual's own
  // start date on the Employee tab, the earliest start date across everyone
  // shown on the Team tab.
  $: minMonth =
    activeTab === "team"
      ? ($earliestStartDate?.slice(0, 7) ?? null)
      : (selectedReportUser?.start_date?.slice(0, 7) ??
        $earliestStartDate?.slice(0, 7) ??
        null);
  $: minDate =
    activeTab === "team"
      ? $earliestStartDate
      : (selectedReportUser?.start_date ?? $earliestStartDate);

  $: if (minMonth && month < minMonth) month = minMonth;
  $: if (minDate && from && from < minDate) from = minDate;
  // Switching from the Team tab to an employee whose own start date is later
  // than the currently-selected `to` would otherwise clamp `from` past `to`,
  // producing an invalid range that fails validation on every fetch.
  $: if (from && to && from > to) to = from;

  $: period = { mode: periodMode, month, from, to };

  // --- Export: acts on whatever the Employee tab currently shows ---
  let exportInProgress = false;
  let exportError = "";

  function exportFileNamePart() {
    if (!selectedReportUser) return String(selectedUserId ?? "report");
    return `${selectedReportUser.first_name} ${selectedReportUser.last_name}`;
  }

  async function exportCsv() {
    if (exportInProgress || selectedUserId == null) return;
    exportInProgress = true;
    exportError = "";
    try {
      const { from: qFrom, to: qTo, active } = timeQueryRange(period, todayIso);
      if (!active) {
        exportError = $t("future_period_no_time_data");
        return;
      }
      const [report, flextimeData] = await Promise.all([
        getRangeReport({ userId: selectedUserId, from: qFrom, to: qTo }),
        getFlextimeReport({
          userId: selectedUserId,
          from: qFrom,
          to: qTo,
        }).catch(() => []),
      ]);
      const csvText = buildTimesheetCsv({
        report,
        flextimeData,
        translate: $t,
      });
      downloadBlob(
        timesheetCsvBlob(csvText),
        `stundennachweis-${safeFileNamePart(exportFileNamePart())}-${qFrom}_${qTo}.csv`,
      );
      toast($t("CSV download started."), "ok");
    } catch (e) {
      exportError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }

  async function exportPdf(teamWide = false) {
    if (exportInProgress) return;
    if (!teamWide && selectedUserId == null) return;
    exportInProgress = true;
    exportError = "";
    try {
      // A fully-future custom range would otherwise cap `to` at today while
      // leaving `from` in the future, sending from > to to the backend (which
      // rejects it with a raw, untranslated validation error). Bail out with
      // the same friendly message exportCsv() already shows for this case.
      const { from: qFrom, to: qTo, active } = timeQueryRange(period, todayIso);
      if (!active) {
        exportError = $t("future_period_no_time_data");
        return;
      }
      const response = await getTimesheetPdf({
        userId: teamWide ? undefined : selectedUserId,
        from: qFrom,
        to: qTo,
      });
      const blob = await response.blob();
      const namePart = teamWide ? $t("All") : exportFileNamePart();
      downloadBlob(
        blob,
        `stundennachweis-${safeFileNamePart(namePart)}-${qFrom}_${qTo}.pdf`,
      );
      toast($t("PDF download started."), "ok");
    } catch (e) {
      exportError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Reports")}</h1>
    <div class="top-bar-subtitle">
      {#if canViewTeamReports}
        {$t("Team hours overview")}
      {:else}
        {$t("Your hours overview")}
      {/if}
    </div>
  </div>
</div>

{#if canViewTeamReports}
  <div class="admin-tabs desktop-tabs">
    <button
      type="button"
      class="tab-link"
      class:active={activeTab === "employee"}
      on:click={() => (activeTab = "employee")}
    >
      {$t("Employee report")}
    </button>
    <button
      type="button"
      class="tab-link"
      class:active={activeTab === "team"}
      on:click={() => (activeTab = "team")}
    >
      {$t("Team report")}
    </button>
  </div>
  <div class="mobile-tabs">
    <select value={activeTab} on:change={(e) => (activeTab = e.target.value)}>
      <option value="employee">{$t("Employee report")}</option>
      <option value="team">{$t("Team report")}</option>
    </select>
  </div>
{/if}

<div class="content-area">
  <div class="zf-card reports-toolbar">
    <div class="zf-toolbar-row">
      {#if activeTab === "employee" && !isSelfOnlyReportsView}
        <div>
          <label class="zf-label" for="reports-user-select"
            >{$t("Employee")}</label
          >
          <select
            id="reports-user-select"
            class="zf-select"
            bind:value={selectedUserId}
          >
            {#each users as u (u.id)}
              <option value={u.id}>{u.first_name} {u.last_name}</option>
            {/each}
          </select>
        </div>
      {/if}

      <PeriodPicker
        id="reports-period"
        bind:mode={periodMode}
        bind:month
        bind:from
        bind:to
        {minMonth}
        maxMonth={currentMonthStr}
        {minDate}
        {maxDate}
      />

      <div class="reports-export-actions">
        {#if activeTab === "employee"}
          <button
            class="zf-btn zf-btn-primary"
            on:click={exportCsv}
            disabled={exportInProgress || selectedUserId == null}
          >
            <Icon name="Download" size={14} />{$t("Export CSV")}
          </button>
          <button
            class="zf-btn zf-btn-primary"
            on:click={() => exportPdf(false)}
            disabled={exportInProgress || selectedUserId == null}
          >
            <Icon name="FileText" size={14} />{$t("Export PDF")}
          </button>
        {:else}
          <button
            class="zf-btn zf-btn-primary"
            on:click={() => exportPdf(true)}
            disabled={exportInProgress}
          >
            <Icon name="FileText" size={14} />{$t("Export team PDF")}
          </button>
        {/if}
      </div>
    </div>
    {#if exportError}
      <div class="error-text">{exportError}</div>
    {/if}
  </div>

  {#if activeTab === "team" && canViewTeamReports}
    <TeamReport {users} {periodMode} {month} {from} {to} />
  {:else}
    <PersonReport
      userId={selectedUserId}
      {users}
      {periodMode}
      {month}
      {from}
      {to}
    />
  {/if}
</div>

<style>
  .reports-toolbar {
    padding: 16px 20px;
    margin-bottom: 16px;
  }

  .reports-export-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
</style>
