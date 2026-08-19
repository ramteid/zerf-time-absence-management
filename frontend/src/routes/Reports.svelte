<script>
  import { onDestroy, onMount } from "svelte";
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
  import LoadingState from "../lib/ui/LoadingState.svelte";
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
  import {
    isReportRangeTooLong,
    REPORT_RANGE_MAX_DAY_DIFFERENCE,
  } from "../lib/domain/dates.js";
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
  let completedUsersLoadKey = "";
  let failedUsersLoadKey = "";
  let usersLoadError = "";
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
        ) {
          users = [];
          completedUsersLoadKey = loadKey;
          failedUsersLoadKey = "";
          usersLoadError = "";
        }
        return;
      }
      const loadedUsers = await getUsersForReports(canTeam, user);
      if (
        loadKey === lastUsersLoadKey &&
        requestId === latestUsersLoadRequest
      ) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        users = loadedUsers;
        // eslint-disable-next-line svelte/infinite-reactive-loop -- completion is read only after this guarded request resolves.
        completedUsersLoadKey = loadKey;
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        failedUsersLoadKey = "";
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        usersLoadError = "";
      }
    } catch (e) {
      if (
        loadKey === lastUsersLoadKey &&
        requestId === latestUsersLoadRequest
      ) {
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        users = [];
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        completedUsersLoadKey = "";
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        failedUsersLoadKey = loadKey;
        // eslint-disable-next-line svelte/infinite-reactive-loop -- the guarded async response belongs to this load key and cannot retrigger it.
        usersLoadError = e?.message || "Error";
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
      completedUsersLoadKey = "";
      failedUsersLoadKey = "";
      usersLoadError = "";
      users = [];
      latestUsersLoadRequest += 1;
      // eslint-disable-next-line svelte/infinite-reactive-loop -- initUsers only commits a response guarded by this load key and request id.
      initUsers(loadKey, latestUsersLoadRequest, user);
    }
  }
  $: currentUserTracksTime = tracksOwnTime($currentUser);
  $: isSelfOnlyReportsView = !canViewTeamReports && currentUserTracksTime;

  // --- Shared filter state ---
  function isoMonthOf(date) {
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
  }

  function isValidIsoDate(value) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
    const date = new Date(`${value}T12:00:00Z`);
    return (
      !Number.isNaN(date.getTime()) && date.toISOString().slice(0, 10) === value
    );
  }

  function reportDeepLink(currentPath) {
    const queryString = currentPath.includes("?")
      ? currentPath.split("?")[1]
      : "";
    const params = new URLSearchParams(queryString);
    const userId = Number(params.get("user"));
    const from = params.get("from");
    const to = params.get("to");
    if (
      !Number.isSafeInteger(userId) ||
      userId < 1 ||
      !from ||
      !to ||
      !isValidIsoDate(from) ||
      !isValidIsoDate(to) ||
      from > to ||
      isReportRangeTooLong(from, to)
    )
      return null;
    return { userId, from, to };
  }

  function reportMaxDate(currentPath, todayDate) {
    const defaultMaxDate = isoDate(addDays(todayDate, 366));
    const linkedMaxDate = reportDeepLink(currentPath)?.to;
    return linkedMaxDate && linkedMaxDate > defaultMaxDate
      ? linkedMaxDate
      : defaultMaxDate;
  }

  function reportMinDate(currentPath, defaultMinDate, selectedUserId) {
    const deepLink = reportDeepLink(currentPath);
    // An absence can remain in the calendar after an administrator corrects
    // the employee's start date to a later day. Preserve this one valid
    // calendar target so its linked report can still reveal the comment.
    // Selecting another employee restores that employee's normal lower bound.
    if (
      !deepLink ||
      deepLink.userId !== Number(selectedUserId) ||
      !defaultMinDate ||
      deepLink.from >= defaultMinDate
    )
      return defaultMinDate;
    return deepLink.from;
  }

  function maxReportRangeEnd(value) {
    if (!value || isoDate(value) !== value) return "";
    return isoDate(addDays(value, REPORT_RANGE_MAX_DAY_DIFFERENCE));
  }

  let today = appTodayDate();
  let todayIso = isoDate(today);
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);
  $: currentMonthStr = isoMonthOf(today);
  // Custom ranges are limited to a year. A valid absence can nevertheless
  // start farther in the future, so its calendar deep link temporarily extends
  // the picker bound far enough to show the linked dates.
  $: maxDate = reportMaxDate($path, today);

  let activeTab = "employee"; // "employee" | "team"
  let periodMode = "month";
  // Derived from `today` directly rather than from the reactive
  // `currentMonthStr`, which is still undefined while these initialisers run —
  // seeding `month` from it left the first render with a blank period.
  let month = isoMonthOf(today);
  let from = "";
  let to = "";
  let selectedUserId = tracksOwnTime($currentUser) ? $currentUser.id : null;
  let dismissedUnavailableDeepLinkPath = "";

  // Deep-link support: Calendar rows and a pending approval's WeekReviewDialog
  // ("View in report") carry user/from/to query params. Apply them once per
  // navigation to preselect the person, force the linked custom date range,
  // and switch to the Employee tab.
  // Reports never calls go() for its own filter changes, so $path stays put
  // afterwards and this won't fight manual filter edits made later.
  function applyDeepLink(currentPath) {
    const deepLink = reportDeepLink(currentPath);
    if (!deepLink) return;
    dismissedUnavailableDeepLinkPath = "";
    selectedUserId = deepLink.userId;
    periodMode = "range";
    from = deepLink.from;
    to = deepLink.to;
    activeTab = "employee";
  }

  function reapplyDeepLinkAfterFragmentNavigation() {
    if ($path.startsWith("/reports?")) applyDeepLink($path);
  }

  onMount(() => {
    // The route store deliberately excludes fragments. A calendar navigation
    // with the same query but a new anchor must still restore its deep-link
    // target after someone manually selected a different employee.
    window.addEventListener(
      "hashchange",
      reapplyDeepLinkAfterFragmentNavigation,
    );
    window.addEventListener("popstate", reapplyDeepLinkAfterFragmentNavigation);
    return () => {
      window.removeEventListener(
        "hashchange",
        reapplyDeepLinkAfterFragmentNavigation,
      );
      window.removeEventListener(
        "popstate",
        reapplyDeepLinkAfterFragmentNavigation,
      );
    };
  });

  onDestroy(() => {
    // A delayed roster response must not update or toast after this route has
    // been removed. Every response is already guarded by this request id.
    latestUsersLoadRequest += 1;
  });

  $: if ($path.startsWith("/reports?")) applyDeepLink($path);

  $: if (isSelfOnlyReportsView) selectedUserId = $currentUser.id;
  $: activeReportDeepLink = reportDeepLink($path);
  $: userListLoadCompleted =
    !!lastUsersLoadKey && completedUsersLoadKey === lastUsersLoadKey;
  $: userListLoadFailed =
    !!lastUsersLoadKey && failedUsersLoadKey === lastUsersLoadKey;
  $: userListLoading =
    !!lastUsersLoadKey && !userListLoadCompleted && !userListLoadFailed;
  // A calendar row can outlive a team reassignment or account deactivation.
  // A self-only user must also never see their own report for a foreign URL.
  // Do not silently replace either inaccessible target with another employee:
  // that would show a different person's report under the linked URL.
  $: linkedUserUnavailable =
    !!activeReportDeepLink &&
    ((isSelfOnlyReportsView &&
      activeReportDeepLink.userId !== Number($currentUser?.id)) ||
      (!isSelfOnlyReportsView &&
        userListLoadCompleted &&
        dismissedUnavailableDeepLinkPath !== $path &&
        !hasUserId(users, activeReportDeepLink.userId)));
  $: if (
    !isSelfOnlyReportsView &&
    userListLoadCompleted &&
    !linkedUserUnavailable &&
    (selectedUserId == null || !hasUserId(users, selectedUserId)) &&
    users.length > 0
  ) {
    selectedUserId = users[0].id;
  }
  $: if (!canViewTeamReports) activeTab = "employee";

  $: selectedReportUser = findUserById(users, selectedUserId, $currentUser);
  $: userListFailureBlocksSelectedReport =
    userListLoadFailed && !selectedReportUser;
  $: userListLoadingBlocksSelectedReport =
    userListLoading && !selectedReportUser;
  $: userListFailureBlocksActiveReport =
    userListLoadFailed && (activeTab === "team" || !selectedReportUser);
  $: userListLoadingBlocksActiveReport =
    userListLoading && (activeTab === "team" || !selectedReportUser);

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
      : reportMinDate(
          $path,
          selectedReportUser?.start_date ?? $earliestStartDate,
          selectedUserId,
        );

  $: if (minMonth && month < minMonth) month = minMonth;
  $: if (minDate && from && from < minDate) from = minDate;
  // Switching from the Team tab to an employee whose own start date is later
  // than the currently-selected `to` would otherwise clamp `from` past `to`,
  // producing an invalid range that fails validation on every fetch.
  $: if (from && to && from > to) to = from;
  // The picker prevents normal UI selection past this point. Keep the report
  // state safe as well, because navigation and programmatic updates can set
  // the bound values without going through a date picker.
  $: maxRangeEnd = maxReportRangeEnd(from);
  $: if (maxRangeEnd && to > maxRangeEnd) to = maxRangeEnd;

  $: period = { mode: periodMode, month, from, to };

  function selectReportUser() {
    dismissedUnavailableDeepLinkPath = $path;
  }

  function retryUsers() {
    const user = $currentUser;
    const loadKey = usersLoadKey(user);
    if (!loadKey || loadKey !== lastUsersLoadKey) return;
    failedUsersLoadKey = "";
    usersLoadError = "";
    completedUsersLoadKey = "";
    users = [];
    latestUsersLoadRequest += 1;
    initUsers(loadKey, latestUsersLoadRequest, user);
  }

  // --- Export: acts on whatever the Employee tab currently shows ---
  let exportInProgress = false;
  let exportError = "";

  function exportFileNamePart(user, userId) {
    if (!user) return String(userId ?? "report");
    return `${user.first_name} ${user.last_name}`;
  }

  function exportSnapshot(teamWide = false) {
    const userId = teamWide ? undefined : selectedUserId;
    return {
      userId,
      namePart: teamWide
        ? $t("All")
        : exportFileNamePart(selectedReportUser, userId),
      period: { ...period },
      todayIso,
    };
  }

  async function exportCsv() {
    if (
      exportInProgress ||
      selectedUserId == null ||
      linkedUserUnavailable ||
      userListFailureBlocksSelectedReport ||
      userListLoadingBlocksSelectedReport
    )
      return;
    const snapshot = exportSnapshot();
    exportInProgress = true;
    exportError = "";
    try {
      const {
        from: qFrom,
        to: qTo,
        active,
      } = timeQueryRange(snapshot.period, snapshot.todayIso);
      if (!active) {
        exportError = $t("future_period_no_time_data");
        return;
      }
      const [report, flextime] = await Promise.all([
        getRangeReport({ userId: snapshot.userId, from: qFrom, to: qTo }),
        getFlextimeReport({
          userId: snapshot.userId,
          from: qFrom,
          to: qTo,
        }).catch(() => ({ days: [], balanceAsOf: null })),
      ]);
      const csvText = buildTimesheetCsv({
        report,
        flextimeData: flextime.days,
        balanceAsOf: flextime.balanceAsOf,
        translate: $t,
      });
      downloadBlob(
        timesheetCsvBlob(csvText),
        `stundennachweis-${safeFileNamePart(snapshot.namePart)}-${qFrom}_${qTo}.csv`,
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
    if (teamWide && (userListLoadFailed || userListLoading)) return;
    if (
      !teamWide &&
      (selectedUserId == null ||
        linkedUserUnavailable ||
        userListFailureBlocksSelectedReport ||
        userListLoadingBlocksSelectedReport)
    )
      return;
    const snapshot = exportSnapshot(teamWide);
    exportInProgress = true;
    exportError = "";
    try {
      // A fully-future custom range would otherwise cap `to` at today while
      // leaving `from` in the future, sending from > to to the backend (which
      // rejects it with a raw, untranslated validation error). Bail out with
      // the same friendly message exportCsv() already shows for this case.
      const {
        from: qFrom,
        to: qTo,
        active,
      } = timeQueryRange(snapshot.period, snapshot.todayIso);
      if (!active) {
        exportError = $t("future_period_no_time_data");
        return;
      }
      const response = await getTimesheetPdf({
        userId: snapshot.userId,
        from: qFrom,
        to: qTo,
      });
      const blob = await response.blob();
      downloadBlob(
        blob,
        `stundennachweis-${safeFileNamePart(snapshot.namePart)}-${qFrom}_${qTo}.pdf`,
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
      <div class="reports-filter-pair">
        {#if activeTab === "employee" && !isSelfOnlyReportsView}
          <div class="reports-user-filter">
            <label class="zf-label" for="reports-user-select"
              >{$t("Employee")}</label
            >
            <select
              id="reports-user-select"
              class="zf-select"
              bind:value={selectedUserId}
              on:change={selectReportUser}
              disabled={userListLoadFailed}
            >
              {#if userListLoadFailed && selectedReportUser}
                <option value={selectedUserId}
                  >{selectedReportUser.first_name}
                  {selectedReportUser.last_name}</option
                >
              {:else if linkedUserUnavailable}
                <option value={selectedUserId} disabled
                  >{$t("User not found or inactive.")}</option
                >
              {/if}
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
          maxRangeDays={REPORT_RANGE_MAX_DAY_DIFFERENCE}
        />
      </div>

      <div class="reports-export-actions">
        {#if activeTab === "employee"}
          <button
            class="zf-btn zf-btn-primary"
            on:click={exportCsv}
            disabled={exportInProgress ||
              selectedUserId == null ||
              linkedUserUnavailable ||
              userListFailureBlocksSelectedReport ||
              userListLoadingBlocksSelectedReport}
          >
            <Icon name="Download" size={14} />{$t("Export CSV")}
          </button>
          <button
            class="zf-btn zf-btn-primary"
            on:click={() => exportPdf(false)}
            disabled={exportInProgress ||
              selectedUserId == null ||
              linkedUserUnavailable ||
              userListFailureBlocksSelectedReport ||
              userListLoadingBlocksSelectedReport}
          >
            <Icon name="FileText" size={14} />{$t("Export PDF")}
          </button>
        {:else}
          <button
            class="zf-btn zf-btn-primary"
            on:click={() => exportPdf(true)}
            disabled={exportInProgress || userListLoadFailed || userListLoading}
          >
            <Icon name="FileText" size={14} />{$t("Export team PDF")}
          </button>
        {/if}
      </div>
    </div>
    {#if exportError}
      <div class="error-text">{exportError}</div>
    {/if}
    {#if activeTab === "employee" && userListLoadFailed && selectedReportUser}
      <div class="zf-row mt-8" role="alert">
        <span class="error-text">{$t(usersLoadError || "Error")}</span>
        <button
          type="button"
          class="zf-btn zf-btn-ghost zf-btn-sm"
          on:click={retryUsers}>{$t("Retry")}</button
        >
      </div>
    {/if}
  </div>

  {#if activeTab === "team" && canViewTeamReports && !userListFailureBlocksActiveReport && !userListLoadingBlocksActiveReport}
    <TeamReport {users} {periodMode} {month} {from} {to} />
  {:else if userListLoadingBlocksActiveReport}
    <div class="zf-card">
      <LoadingState />
    </div>
  {:else if userListFailureBlocksActiveReport}
    <div class="zf-card">
      <div class="zf-card-empty zf-col" role="alert">
        <span>{$t(usersLoadError || "Error")}</span>
        <button class="zf-btn zf-btn-primary" on:click={retryUsers}
          >{$t("Retry")}</button
        >
      </div>
    </div>
  {:else if linkedUserUnavailable}
    <div class="zf-card">
      <div class="zf-card-empty" role="status">
        {$t("User not found or inactive.")}
      </div>
    </div>
  {:else}
    <PersonReport
      userId={selectedUserId}
      {users}
      {periodMode}
      {month}
      {from}
      {to}
      navigationKey={$path}
    />
  {/if}
</div>

<style>
  .reports-toolbar {
    padding: 16px 20px;
    margin-bottom: 16px;
  }

  /* Groups the employee selector and the period picker. On desktop they sit at
     their natural sizes on one line (the toolbar-row wraps the export buttons
     below when space is tight). The mobile side-by-side behaviour lives in the
     media query at the bottom of this block and in PeriodPicker.svelte. */
  .reports-filter-pair {
    display: flex;
    align-items: flex-end;
    gap: 12px;
    flex: 1 1 auto;
    min-width: 0;
  }

  .reports-user-filter {
    flex: 0 1 auto;
    min-width: 0;
  }

  /* Mobile: keep the employee dropdown and the month nav on one row, with the
     period picker's "Custom range" toggle dropping to a full-width line below
     them (see PeriodPicker.svelte, which uses `display: contents` to promote
     its nav + toggle into THIS flex container). flex-wrap lets the toggle wrap;
     the employee select takes flex: 1 1 0 so it shrinks to share the row with
     the ~200px month nav instead of forcing a stack. */
  @media (max-width: 768px) {
    .reports-filter-pair {
      flex-wrap: wrap;
    }

    .reports-user-filter {
      flex: 1 1 0;
    }
  }

  .reports-export-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
</style>
