<script>
  import { tick } from "svelte";
  import { fly } from "svelte/transition";
  import { categories, currentUser, path, settings, toast } from "../stores.js";
  import { t, absenceKindLabel, formatHours } from "../i18n.js";
  import {
    fmtDate,
    fmtDateShort,
    fmtDateTime,
    fmtWeekLabel,
    isoDate,
    appTodayDate,
    addDays,
    parseDate,
    monday,
  } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import { confirmDialog } from "../confirm.js";
  import FlextimeChart from "../FlextimeChart.svelte";
  import DatePicker from "../DatePicker.svelte";
  import { isAssistantUser } from "../rolePolicy.js";
  import {
    approveAbsenceById,
    approveReopen as approveReopenRequest,
    approveWeek as approveWeekEntries,
    getApprovalDashboard,
    getFlextime,
    getMonthSubmissionReport,
    getOvertimeSummary,
    getTeamAbsences,
    rejectAbsenceById,
    rejectReopen as rejectReopenRequest,
    rejectWeek as rejectWeekEntries,
  } from "../lib/api/dashboardApi.js";
  import {
    allMonthsToCheck,
    buildPendingWeeks,
    buildSubmissionChecks,
    currentWeekIsOpen,
  } from "../lib/domain/dashboard.js";
  import {
    userInitialsFromRows,
    userNameFromRows,
  } from "../lib/domain/users.js";

  // ── Approval workflow state (team leads and admins only) ──────────────────────
  let pendingEntries = [];
  let pendingWeeks = [];
  let pendingAbsences = [];
  let pendingReopens = [];
  let users = [];
  let absenceDetail = null;
  let requestDetail = null;

  // Absence slider: browse approved absences week by week (leads/admins only).
  let absenceSliderWeek = isoDate(monday(appTodayDate()));
  let absenceSliderTeamData = [];
  let absenceSliderIsLeadView = false;
  let absenceSliderDirection = 1;

  // Week details dialog (for inspecting a single pending timesheet).
  let selectedWeek = null;
  let weekActionBusy = false;

  // Section element refs used to scroll-to-section when navigating from a badge.
  let timesheetsSectionEl;
  let absencesSectionEl;
  let focusedSection = "";
  let lastFocusSignature = "";

  // ── Reference date: derived from configured app timezone ─────────────────────
  // Initialize with a concrete value so imperative startup code (loadPastMonthSubmissionStatus,
  // clampFromToUserStart) can run before the reactive declaration is first evaluated.
  let today = appTodayDate();
  $: today = appTodayDate($settings?.timezone);

  function daysAgo(numberOfDays) {
    return isoDate(addDays(today, -numberOfDays));
  }

  // Clamp the chart's start date to the user's contract start so they don't see
  // a misleading deficit from before they were employed.
  function clampFromToUserStart(date) {
    const userStart = $currentUser?.start_date;
    return userStart && userStart > date ? userStart : date;
  }

  // ── Flextime chart ────────────────────────────────────────────────────────────
  let chartFrom = clampFromToUserStart(daysAgo(29));
  let chartTo = isoDate(today);
  let chartData = [];
  let chartLoading = false;

  // ── Overtime summary (monthly cumulative, for all users) ──────────────────────
  let overtimeRows = [];
  let overtimeLoading = false;
  let overtimeError = "";

  // ── Month-by-month submission compliance (for all users) ─────────────────────
  let monthSubmissionChecks = [];
  let monthSubmissionLoading = false;
  let monthSubmissionError = "";

  // eslint-disable-next-line no-useless-assignment
  let reportYear = today.getFullYear();
  $: reportYear = today.getFullYear();
  $: currentMonthIndex = today.getMonth() + 1; // 1-based
  $: currentMonthKey = `${reportYear}-${String(currentMonthIndex).padStart(2, "0")}`;
  $: todayIso = isoDate(today);
  $: isAssistantCurrentUser = isAssistantUser($currentUser);

  // ── Loaders ───────────────────────────────────────────────────────────────────

  async function loadChart() {
    if (chartFrom > chartTo) return;
    if (isAssistantCurrentUser) {
      chartData = [];
      chartLoading = false;
      return;
    }
    chartLoading = true;
    try {
      chartData = await getFlextime({ from: chartFrom, to: chartTo });
    } catch {
      chartData = [];
    } finally {
      chartLoading = false;
    }
  }

  // Convert a minute count into a formatted hours string (e.g. "1:30 h").
  function hoursFromMinutes(minutes) {
    return formatHours((minutes || 0) / 60);
  }

  async function loadOvertimeSummary() {
    if (isAssistantCurrentUser) {
      overtimeRows = [];
      overtimeError = "";
      overtimeLoading = false;
      return;
    }
    overtimeLoading = true;
    overtimeError = "";
    try {
      const year = today.getFullYear();
      overtimeRows = await getOvertimeSummary(year);
    } catch (error) {
      overtimeRows = [];
      overtimeError = error?.message || "Overtime data unavailable.";
    } finally {
      overtimeLoading = false;
    }
  }

  async function loadPastMonthSubmissionStatus() {
    const monthsToCheck = allMonthsToCheck($currentUser?.start_date, today);
    if (!monthsToCheck.length) {
      monthSubmissionChecks = [];
      return;
    }

    monthSubmissionLoading = true;
    monthSubmissionError = "";
    try {
      const requests = monthsToCheck.map((month) =>
        getMonthSubmissionReport(month),
      );
      const reports = await Promise.all(requests);
      monthSubmissionChecks = buildSubmissionChecks(monthsToCheck, reports);
    } catch (error) {
      monthSubmissionChecks = [];
      monthSubmissionError = error?.message || "Could not check submission status.";
    } finally {
      monthSubmissionLoading = false;
    }
  }

  function setRange(days) {
    chartFrom = clampFromToUserStart(daysAgo(days - 1));
    chartTo = isoDate(today);
    loadChart();
  }

  // Loads all data that is only visible to team leads and admins (can_approve).
  async function load() {
    const canApprove = !!$currentUser?.permissions?.can_approve;
    if (!canApprove) {
      pendingEntries = [];
      pendingAbsences = [];
      pendingReopens = [];
      users = [];
      return;
    }
    try {
      const {
        submittedTimeEntries,
        requestedAbsences,
        pendingReopenRequests,
        users: teamMembers,
      } = await getApprovalDashboard();
      pendingEntries = submittedTimeEntries;
      pendingAbsences = requestedAbsences;
      pendingReopens = pendingReopenRequests;
      users = teamMembers;
    } catch (error) {
      pendingEntries = [];
      pendingAbsences = [];
      pendingReopens = [];
      users = [];
      toast($t(error?.message || "Error"), "error");
    }
  }

  load();
  loadChart();
  loadOvertimeSummary();
  loadPastMonthSubmissionStatus();
  loadAbsenceSliderTeamData(absenceSliderWeek);

  // ── Reactive derivations: overtime balance ────────────────────────────────────

  $: pendingWeeks = buildPendingWeeks(pendingEntries, users, $categories);

  $: currentOvertimeRow =
    overtimeRows.find((row) => row.month === currentMonthKey) ??
    (overtimeRows.length ? overtimeRows[overtimeRows.length - 1] : null);
  $: overtimeBalanceMin = currentOvertimeRow?.cumulative_min || 0;
  $: submittedOvertimeBalanceMin = currentOvertimeRow?.submitted_cumulative_min ?? overtimeBalanceMin;
  $: currentMonthDiffMin = currentOvertimeRow?.diff_min || 0;

  // ── Reactive derivations: submission compliance ───────────────────────────────

  // True when every month from the user's start to now has weeks_all_submitted.
  // Empty checks (no start date, or start date in the future) count as "all done".
  // The backend excludes the current in-progress week from this flag.
  $: allWeeksSubmitted =
    monthSubmissionChecks.length === 0 ||
    monthSubmissionChecks.every((check) => check.submitted);

  // True when every submitted week is also fully approved (no pending approvals).
  $: allWeeksApproved =
    allWeeksSubmitted &&
    (monthSubmissionChecks.length === 0 ||
      monthSubmissionChecks.every((check) => check.approved));

  // True when the calendar week containing today is in draft, partial, or
  // rejected state — i.e., the Zeiterfassung view would show something other
  // than "Eingereicht"/"Genehmigt". Drives the sub-line on the submission tile.
  $: currentWeekOpen = currentWeekIsOpen(monthSubmissionChecks);

  // ── Reactive: keep selectedWeek in sync after a refresh ──────────────────────

  $: if (selectedWeek) {
    const next = pendingWeeks.find((week) => week.key === selectedWeek.key);
    if (!next) selectedWeek = null;
    else if (next !== selectedWeek) selectedWeek = next;
  }

  // ── Utility helpers ───────────────────────────────────────────────────────────

  function userName(userId, userRows) {
    return userNameFromRows(userId, userRows);
  }

  function userInitials(userId, userRows) {
    return userInitialsFromRows(userId, userRows) || "?";
  }

  function weekHours(week) {
    return formatHours(week.total_min / 60);
  }

  // ── Focus/scroll-to-section logic ────────────────────────────────────────────

  function sectionByFocus(focus) {
    if (focus === "timesheets") return timesheetsSectionEl;
    if (focus === "absences") return absencesSectionEl;
    if (focus === "reopen") return timesheetsSectionEl;
    return null;
  }


  function absenceRequestTypeLabel(absence) {
    if (absence.status === "cancellation_pending" || absence.review_type === "cancellation") {
      return $t("Cancellation");
    }
    if (absence.review_type === "change") {
      return $t("Change");
    }
    return $t("Approval");
  }

  function absenceDiffRows(absence) {
    if (absence.review_type !== "change") return [];
    const rows = [];
    if (absence.previous_kind && absence.previous_kind !== absence.kind) {
      rows.push({
        field: $t("Type"),
        before: absenceKindLabel(absence.previous_kind),
        after: absenceKindLabel(absence.kind),
      });
    }
    if (absence.previous_start_date && absence.previous_start_date !== absence.start_date) {
      rows.push({
        field: $t("From"),
        before: fmtDateShort(absence.previous_start_date),
        after: fmtDateShort(absence.start_date),
      });
    }
    if (absence.previous_end_date && absence.previous_end_date !== absence.end_date) {
      rows.push({
        field: $t("To"),
        before: fmtDateShort(absence.previous_end_date),
        after: fmtDateShort(absence.end_date),
      });
    }
    if ((absence.previous_comment || "") !== (absence.comment || "")) {
      rows.push({
        field: $t("Comment"),
        before: absence.previous_comment || $t("Empty"),
        after: absence.comment || $t("Empty"),
      });
    }
    return rows;
  }

  function openReopenDetail(item) {
    requestDetail = { item };
  }


  async function revealFocusSection(focus) {
    await tick();
    const section = sectionByFocus(focus);
    if (!section) return;
    section.scrollIntoView({ behavior: "smooth", block: "start" });
    focusedSection = focus;
    setTimeout(() => {
      if (focusedSection === focus) focusedSection = "";
    }, 1400);
  }

  // ── Absence slider (team view, leads/admins only) ─────────────────────────────

  async function loadAbsenceSliderTeamData(weekStartDate) {
    absenceSliderIsLeadView = $currentUser?.permissions?.can_approve || false;
    if (!absenceSliderIsLeadView) return;
    try {
      const weekEnd = isoDate(addDays(parseDate(weekStartDate), 6));
      const params = new URLSearchParams({
        from: weekStartDate,
        to: weekEnd,
        status: "approved",
      });
      absenceSliderTeamData = await getTeamAbsences(params);
    } catch {
      absenceSliderTeamData = [];
    }
  }

  function absenceSliderPrevWeek() {
    absenceSliderDirection = -1;
    absenceSliderWeek = isoDate(addDays(parseDate(absenceSliderWeek), -7));
    loadAbsenceSliderTeamData(absenceSliderWeek);
  }

  function absenceSliderNextWeek() {
    absenceSliderDirection = 1;
    absenceSliderWeek = isoDate(addDays(parseDate(absenceSliderWeek), 7));
    loadAbsenceSliderTeamData(absenceSliderWeek);
  }

  function absenceSliderToToday() {
    absenceSliderDirection = 0;
    absenceSliderWeek = isoDate(monday(today));
    loadAbsenceSliderTeamData(absenceSliderWeek);
  }

  // ── URL-driven section focus ──────────────────────────────────────────────────

  $: dashboardQuery = (() => {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(queryString);
  })();

  $: focusTarget = dashboardQuery.get("focus") || "";
  $: focusNonce = dashboardQuery.get("n") || "";

  $: {
    // A nonce ensures the scroll fires even when navigating to the same section twice.
    const signature = focusTarget ? `${focusTarget}:${focusNonce}` : "";
    if (signature && signature !== lastFocusSignature) {
      // eslint-disable-next-line no-useless-assignment
      lastFocusSignature = signature;
      revealFocusSection(focusTarget);
    }
  }

  // ── Week dialog (timesheet detail view) ───────────────────────────────────────

  function openWeekDetails(week) {
    selectedWeek = week;
  }


  async function approveWeek(week) {
    if (!week?.entries?.length || weekActionBusy) return;
    weekActionBusy = true;
    try {
      const result = await approveWeekEntries(
        week.entries.map((entry) => entry.id),
      );
      if ((result?.count ?? 0) > 0) {
        toast($t("Approved."), "ok");
      }
      selectedWeek = null;
      await load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    } finally {
      weekActionBusy = false;
    }
  }

  async function rejectWeek(week) {
    if (!week?.entries?.length || weekActionBusy) return;
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this request?"),
      { danger: true, confirm: $t("Reject"), reason: true },
    );
    if (!reason) return;

    weekActionBusy = true;
    try {
      await rejectWeekEntries(
        week.entries.map((entry) => entry.id),
        reason,
      );
      toast($t("Rejected."), "ok");
      selectedWeek = null;
      await load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    } finally {
      weekActionBusy = false;
    }
  }

  async function batchApprove() {
    const ids = pendingEntries.map((entry) => entry.id);
    if (!ids.length) return;
    const confirmed = await confirmDialog(
      $t("Approve all?"),
      $t("Approve all {n} weeks across all users?", { n: pendingWeeks.length }),
      { confirm: $t("Approve all") },
    );
    if (!confirmed) return;
    try {
      await approveWeekEntries(ids);
      toast($t("All approved."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  // ── Absence approval ──────────────────────────────────────────────────────────

  function showAbsenceDetail(absence) {
    absenceDetail = absence;
  }


  async function approveAbsence(absence) {
    try {
      await approveAbsenceById(absence);
      toast($t("Approved."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  async function rejectAbsence(absence) {
    const isCancellation = absence.status === "cancellation_pending";
    if (isCancellation) {
      const confirmed = await confirmDialog(
        $t("Reject cancellation?"),
        $t("Reject this cancellation request? The absence will remain approved."),
        { danger: true, confirm: $t("Reject") },
      );
      if (!confirmed) return;
      try {
        await rejectAbsenceById(absence);
        toast($t("Rejected."), "ok");
        load();
      } catch (error) {
        toast($t(error?.message || "Error"), "error");
      }
    } else {
      const reason = await confirmDialog(
        $t("Reject?"),
        $t("Reject this request?"),
        { danger: true, confirm: $t("Reject"), reason: true },
      );
      if (!reason) return;
      try {
        await rejectAbsenceById(absence, reason);
        toast($t("Rejected."), "ok");
        load();
      } catch (error) {
        toast($t(error?.message || "Error"), "error");
      }
    }
  }

  // ── Reopen-request approval ───────────────────────────────────────────────────

  async function approveReopen(id) {
    try {
      await approveReopenRequest(id);
      toast($t("Approved."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  async function rejectReopen(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this request?"),
      { danger: true, confirm: $t("Reject"), reason: true },
    );
    if (!reason) return;
    try {
      await rejectReopenRequest(id, reason);
      toast($t("Rejected."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  // ── Help tooltips ─────────────────────────────────────────────────────────────
  let activeHelp = null;
  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Dashboard")}</h1>
    <div class="top-bar-subtitle">
      {#if $currentUser?.permissions?.can_approve}
        {$t("Approve weeks & manage requests")}
      {:else}
        {$t("Your overview")}
      {/if}
    </div>
  </div>
</div>

<div class="content-area">

  <!-- ════════════════════════════════════════════════════════════════════════
       SECTION 1 – "Meine Bilanz": running balance & compliance (all users)
       ════════════════════════════════════════════════════════════════════════ -->
  <div class="dashboard-group">
    <div class="dashboard-group-label" style="display:flex;align-items:center;gap:6px">
      {$t("My Balance")}
      <button
        class="zf-btn-icon-sm zf-btn-ghost"
        title={$t("help_my_balance")}
        on:click={() => toggleHelp("balance")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
    </div>
    {#if activeHelp === "balance"}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("help_my_balance")}
      </div>
    {/if}
    <div class="stat-cards">

      {#if !isAssistantCurrentUser}
        <!-- Cumulative overtime balance including today -->
        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Overtime overview")}</div>
          {#if overtimeLoading}
            <div class="stat-card-value tab-num">...</div>
          {:else}
            <div
              class="stat-card-value tab-num"
              style="color:{submittedOvertimeBalanceMin < 0
                ? 'var(--danger-text)'
                : 'var(--success-text)'}"
            >
              {hoursFromMinutes(submittedOvertimeBalanceMin)}
            </div>
            <div class="stat-card-sub">
              {#if submittedOvertimeBalanceMin !== overtimeBalanceMin}
                {$t("Approved: {value}", { value: hoursFromMinutes(overtimeBalanceMin) })}
              {:else}
                {$t("This month: {value}", { value: hoursFromMinutes(currentMonthDiffMin) })}
              {/if}
            </div>
          {/if}
          {#if overtimeError}
            <div class="error-text" style="font-size:11px;margin-top:4px">
              {$t("Overtime data unavailable.")}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Whether all weeks since the user's start date (up to last week) are submitted -->
      <div class="zf-card stat-card">
        <div class="stat-card-label">{$t("Submissions")}</div>
        {#if monthSubmissionLoading}
          <div class="stat-card-value tab-num">...</div>
        {:else}
          <div
            class="stat-card-value tab-num"
            style="color:{allWeeksApproved ? 'var(--success-text)' : 'var(--warning-text)'}"
          >
            {#if allWeeksApproved}
              {$t("All submitted and approved")}
            {:else if allWeeksSubmitted}
              {$t("All submitted (approvals pending)")}
            {:else}
              {$t("Weeks missing")}
            {/if}
          </div>
          {#if currentWeekOpen}
            <div
              class="stat-card-sub"
              style="color:var(--text-tertiary);font-size:11px;margin-top:4px"
            >
              {$t("Current week: still open")}
            </div>
          {/if}
        {/if}
        {#if monthSubmissionError}
          <div class="error-text" style="font-size:11px;margin-top:4px">
            {$t("Could not check submission status.")}
          </div>
        {/if}
      </div>

    </div>
  </div>

  <!-- ════════════════════════════════════════════════════════════════════════
       SECTION 3 – "Mein Team": approval counters (team leads & admins only)
       ════════════════════════════════════════════════════════════════════════ -->
  {#if $currentUser?.permissions?.can_approve}
    <div class="dashboard-group">
      <div class="dashboard-group-label">{$t("My Team")}</div>
      <div class="stat-cards">

        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Pending Weeks")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{pendingWeeks.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{pendingWeeks.length}</div>
        </div>

        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Absence Requests")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{pendingAbsences.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{pendingAbsences.length}</div>
        </div>

        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Team Members")}</div>
          <div class="stat-card-value tab-num">{users.length}</div>
        </div>

      </div>
    </div>
  {/if}

  <!-- ════════════════════════════════════════════════════════════════════════
       APPROVAL GRIDS (team leads & admins only)
       ════════════════════════════════════════════════════════════════════════ -->
  {#if $currentUser?.permissions?.can_approve}
    <div
      class="dashboard-approval-grid"
      style="display:grid;grid-template-columns:1fr 1fr;gap:16px"
    >
      <!-- Timesheet approvals -->
      <div
        class="zf-card"
        class:dashboard-focus={focusedSection === "timesheets"}
        style="overflow-x:auto"
        bind:this={timesheetsSectionEl}
      >
        <div class="card-header">
          <Icon name="FileText" size={15} sw={1.5} />
          <span class="card-header-title">{$t("Week Approvals")}</span>
          {#if pendingWeeks.length + pendingReopens.length > 0}
            <span class="zf-chip zf-chip-pending" style="font-size:10.5px">
              {pendingWeeks.length + pendingReopens.length}
              {$t("pending")}
            </span>
          {/if}
          {#if pendingWeeks.length}
            <button class="zf-btn zf-btn-sm" on:click={batchApprove}>
              <Icon name="Check" size={13} />{$t("Approve All")}
            </button>
          {/if}
        </div>
        {#each pendingWeeks as week (week.key)}
          <div
            class="dashboard-click-row"
            on:click={() => openWeekDetails(week)}
            on:keydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openWeekDetails(week);
              }
            }}
            role="button"
            tabindex="0"
            title={$t("Show")}
          >
            <div class="avatar" style="width:30px;height:30px;font-size:11px">
              {userInitials(week.user_id, users)}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userName(week.user_id, users)}
                <span class="zf-chip zf-chip-submitted" style="font-size:10px">{$t("Approval")}</span>
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {fmtWeekLabel(week.week_start)} · {weekHours(week)}
              </div>
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="zf-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                title={$t("Approve")}
                on:click|stopPropagation={() => approveWeek(week)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="zf-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                title={$t("Reject")}
                on:click|stopPropagation={() => rejectWeek(week)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
        {#each pendingReopens as reopen (reopen.id)}
          <div
            class="dashboard-click-row"
            on:click={() => openReopenDetail(reopen)}
            on:keydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openReopenDetail(reopen);
              }
            }}
            role="button"
            tabindex="0"
            title={$t("Show details")}
          >
            <div class="avatar" style="width:30px;height:30px;font-size:11px">
              {userInitials(reopen.user_id, users)}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userName(reopen.user_id, users)}
                <span class="zf-chip zf-chip-pending" style="font-size:10px">{$t("Edit request")}</span>
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {$t("wants to edit {week_label}", { week_label: fmtWeekLabel(reopen.week_start) })}
              </div>
              {#if reopen.reason}
                <div style="font-size:11px;color:var(--text-tertiary);margin-top:2px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:300px" title={reopen.reason}>
                  {reopen.reason}
                </div>
              {/if}
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="zf-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                title={$t("Approve")}
                on:click|stopPropagation={() => approveReopen(reopen.id)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="zf-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                title={$t("Reject")}
                on:click|stopPropagation={() => rejectReopen(reopen.id)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
        {#if pendingWeeks.length === 0 && pendingReopens.length === 0}
          <div
            style="padding:32px;text-align:center;color:var(--text-tertiary);font-size:13px"
          >
            <Icon name="Check" size={24} sw={1.2} />
            <div style="margin-top:8px">{$t("All caught up!")}</div>
          </div>
        {/if}
      </div>

      <!-- Absence-request approvals -->
      <div
        class="zf-card"
        class:dashboard-focus={focusedSection === "absences"}
        style="overflow-x:auto"
        bind:this={absencesSectionEl}
      >
        <div class="card-header">
          <Icon name="Plane" size={15} sw={1.5} />
          <span class="card-header-title">{$t("Absence Requests")}</span>
          {#if pendingAbsences.length}
            <span class="zf-chip zf-chip-pending" style="font-size:10.5px">
              {pendingAbsences.length}
              {$t("pending")}
            </span>
          {/if}
        </div>
        {#each pendingAbsences as absence (absence.id)}
          <div
            style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
          >
            <div class="avatar" style="width:30px;height:30px;font-size:11px">
              {userInitials(absence.user_id, users)}
            </div>
            <div
              style="flex:1;min-width:0;cursor:pointer"
              on:click={() => showAbsenceDetail(absence)}
              on:keydown={(e) => {
                if (e.key === "Enter") showAbsenceDetail(absence);
              }}
              role="button"
              tabindex="0"
              title={$t("Show details")}
            >
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userName(absence.user_id, users)}
                <span
                  class="zf-chip {absence.status === 'cancellation_pending' ? 'zf-chip-cancellation_pending' : 'zf-chip-warning'}"
                  style="font-size:10px"
                >
                  {absenceRequestTypeLabel(absence)}
                </span>
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {absenceKindLabel(absence.kind)} · {fmtDateShort(absence.start_date)} -
                {fmtDateShort(absence.end_date)}
              </div>
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="zf-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                on:click={() => approveAbsence(absence)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="zf-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                on:click={() => rejectAbsence(absence)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
        {#if pendingAbsences.length === 0}
          <div
            style="padding:32px;text-align:center;color:var(--text-tertiary);font-size:13px"
          >
            <Icon name="Plane" size={24} sw={1.2} />
            <div style="margin-top:8px">{$t("No pending requests")}</div>
          </div>
        {/if}
      </div>
    </div>

    <!-- "Who is absent" team calendar widget -->
    <div class="zf-card" style="margin-top:16px;overflow:hidden">
      <div class="card-header">
        <Icon name="Users" size={15} sw={1.5} />
        <span class="card-header-title">{$t("Who is absent")}</span>
        <div class="absence-date-controls">
          <div class="absence-week-picker">
            <button
              class="zf-btn zf-btn-icon-sm zf-btn-ghost"
              on:click={absenceSliderPrevWeek}
              aria-label={$t("Previous week")}
            >
              <Icon name="ChevLeft" size={16} />
            </button>
            <button
              class="zf-btn zf-btn-ghost absence-week-range"
              on:click={absenceSliderToToday}
              title={$t("Today")}
            >
              {fmtDateShort(absenceSliderWeek)} -
              {fmtDateShort(isoDate(addDays(parseDate(absenceSliderWeek), 6)))}
            </button>
            <button
              class="zf-btn zf-btn-icon-sm zf-btn-ghost"
              on:click={absenceSliderNextWeek}
              aria-label={$t("Next week")}
            >
              <Icon name="ChevRight" size={16} />
            </button>
          </div>
        </div>
      </div>

      {#key absenceSliderWeek}
        <div class="dropdown-slider" in:fly={{ x: absenceSliderDirection * 80, duration: 200 }}>
          {#if absenceSliderTeamData.length === 0}
            <div style="padding:12px;color:var(--text-tertiary);font-size:13px">
              {$t("No absences this week.")}
            </div>
          {:else}
              {#each absenceSliderTeamData as absence (absence.user_id)}
                {@const absentUser = users.find((u) => u.id === absence.user_id)}
                <div class="dropdown-slider-item">
                  <div>
                    <div style="font-weight:500;font-size:13px">
                      {absentUser
                        ? `${absentUser.first_name} ${absentUser.last_name}`
                        : `#${absence.user_id}`}
                    </div>
                    <div style="font-size:12px;color:var(--text-tertiary)">
                      {absenceKindLabel(absence.kind)} · {fmtDateShort(absence.start_date)}{#if absence.start_date !== absence.end_date} - {fmtDateShort(absence.end_date)}{/if}
                    </div>
                  </div>
                </div>
              {/each}
          {/if}
        </div>
      {/key}
    </div>

  {/if}

  <!-- ════════════════════════════════════════════════════════════════════════
       FLEXTIME CHART (all users) – placed after approval sections so it
       doesn't push urgent approval work below the fold for leads/admins.
       ════════════════════════════════════════════════════════════════════════ -->
  {#if !isAssistantCurrentUser}
    <div class="zf-card" style="padding:16px 20px;margin-top:16px">
      <div
        style="display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:14px"
      >
        <Icon name="TrendingUp" size={15} sw={1.5} />
        <span style="font-size:14px;font-weight:400;flex:1">{$t("Flextime balance")}</span>
        <button
          class="zf-btn-icon-sm zf-btn-ghost"
          title={$t("help_flextime_chart")}
          on:click={() => toggleHelp("flextime")}
          style="color:var(--text-tertiary);font-size:14px;cursor:help"
        >
          <Icon name="Info" size={14} />
        </button>
        <div style="display:flex;gap:4px;flex-wrap:wrap">
          <button class="zf-btn zf-btn-sm" on:click={() => setRange(30)}
            >{$t("Last 30 days")}</button
          >
          <button class="zf-btn zf-btn-sm" on:click={() => setRange(90)}
            >{$t("Last 90 days")}</button
          >
          <button class="zf-btn zf-btn-sm" on:click={() => setRange(182)}
            >{$t("Last 6 months")}</button
          >
          <button class="zf-btn zf-btn-sm" on:click={() => setRange(365)}
            >{$t("Last year")}</button
          >
        </div>
        <div style="display:flex;align-items:center;gap:4px">
          <DatePicker
            bind:value={chartFrom}
            min={$currentUser?.start_date}
            max={chartTo}
            style="font-size:12px;padding:3px 6px;height:28px"
          />
          <span style="font-size:12px;color:var(--text-tertiary)">-</span>
          <DatePicker
            bind:value={chartTo}
            min={chartFrom}
            max={todayIso}
            style="font-size:12px;padding:3px 6px;height:28px"
          />
          <button class="zf-btn zf-btn-sm" on:click={loadChart} aria-label={$t("Show")}>
            <Icon name="Search" size={13} />
          </button>
        </div>
      </div>
      {#if activeHelp === "flextime"}
        <div
          style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
        >
          {$t("help_flextime_chart")}
        </div>
      {/if}
      {#if chartLoading}
        <div
          style="text-align:center;padding:40px 0;font-size:13px;color:var(--text-tertiary)"
        >
          {$t("Loading...")}
        </div>
      {:else}
        <FlextimeChart data={chartData} />
      {/if}
    </div>
  {/if}

</div>

<!-- ── Absence detail dialog ─────────────────────────────────────────────────── -->
{#if absenceDetail}
  <Dialog title={$t("Absence Request Details")} onClose={() => (absenceDetail = null)}>
    <div style="display:flex;flex-direction:column;gap:10px">
        <div>
          <div class="zf-label">{$t("Employee")}</div>
          <div style="font-weight:500">{userName(absenceDetail.user_id, users)}</div>
        </div>
        <div>
          <div class="zf-label">{$t("Absence Type")}</div>
          <div>{absenceKindLabel(absenceDetail.kind)}</div>
        </div>
        <div>
          <div class="zf-label">{$t("Request Type")}</div>
          <div>
            <span
              class="zf-chip {absenceDetail.status === 'cancellation_pending' ? 'zf-chip-cancellation_pending' : 'zf-chip-warning'}"
            >
              {absenceRequestTypeLabel(absenceDetail)}
            </span>
          </div>
        </div>
        <div class="field-row">
          <div>
            <div class="zf-label">{$t("From")}</div>
            <div class="tab-num">{fmtDate(absenceDetail.start_date)}</div>
          </div>
          <div>
            <div class="zf-label">{$t("To")}</div>
            <div class="tab-num">{fmtDate(absenceDetail.end_date)}</div>
          </div>
        </div>
        {#if absenceDetail.comment}
          <div>
            <div class="zf-label">{$t("Comment")}</div>
            <div style="white-space:pre-wrap;font-size:13px">{absenceDetail.comment}</div>
          </div>
        {/if}
        <div>
          <div class="zf-label">{$t("Requested at")}</div>
          <div class="tab-num" style="font-size:12px">
            {fmtDateTime(absenceDetail.created_at)}
          </div>
        </div>
        {#if absenceDetail.review_type === "change"}
          {@const diffRows = absenceDiffRows(absenceDetail)}
          {#if diffRows.length}
            <div>
              <div class="zf-label">{$t("Changes")}</div>
              <div class="change-diff-list">
                {#each diffRows as row (row.field)}
                  <div class="change-diff-row">
                    <div class="change-diff-field">{row.field}</div>
                    <div class="change-diff-before">{row.before}</div>
                    <div class="change-diff-arrow">→</div>
                    <div class="change-diff-after">{row.after}</div>
                  </div>
                {/each}
              </div>
            </div>
          {:else}
            <div style="font-size:12px;color:var(--text-tertiary)">
              {$t("Diff unavailable for this request.")}
            </div>
          {/if}
        {/if}
    </div>
    <svelte:fragment slot="footer">
      <button class="zf-btn" on:click={() => (absenceDetail = null)}>{$t("Close")}</button>
      <span style="flex:1"></span>
      <button
        class="zf-btn zf-btn-danger"
        on:click={() => {
          const absence = absenceDetail;
          absenceDetail = null;
          rejectAbsence(absence);
        }}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="zf-btn zf-btn-primary"
        on:click={() => {
          const absence = absenceDetail;
          absenceDetail = null;
          approveAbsence(absence);
        }}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </svelte:fragment>
  </Dialog>
{/if}

<!-- ── Reopen-request detail dialog ─────────────────────────────────────────── -->
{#if requestDetail}
  <Dialog title={$t("Edit Request Details")} onClose={() => (requestDetail = null)}>
    <div style="display:flex;flex-direction:column;gap:10px">
      <div>
        <div class="zf-label">{$t("Employee")}</div>
        <div style="font-weight:500">{userName(requestDetail.item.user_id, users)}</div>
      </div>
      <div>
        <div class="zf-label">{$t("Type")}</div>
        <div><span class="zf-chip zf-chip-pending">{$t("Edit request")}</span></div>
      </div>
      <div>
        <div class="zf-label">{$t("Week")}</div>
        <div class="tab-num">
          {fmtWeekLabel(requestDetail.item.week_start)}
        </div>
      </div>
      <div>
        <div class="zf-label">{$t("Requested at")}</div>
        <div class="tab-num" style="font-size:12px">{fmtDateTime(requestDetail.item.created_at)}</div>
      </div>
      {#if requestDetail.item.reason}
        <div>
          <div class="zf-label">{$t("Reason")}</div>
          <div style="font-size:13px;white-space:pre-wrap;word-break:break-word">{requestDetail.item.reason}</div>
        </div>
      {/if}
    </div>
    <svelte:fragment slot="footer">
      <button class="zf-btn" on:click={() => (requestDetail = null)}>{$t("Close")}</button>
      <span style="flex:1"></span>
      <button
        class="zf-btn zf-btn-danger"
        on:click={() => {
          const detail = requestDetail;
          requestDetail = null;
          rejectReopen(detail.item.id);
        }}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="zf-btn zf-btn-primary"
        on:click={() => {
          const detail = requestDetail;
          requestDetail = null;
          approveReopen(detail.item.id);
        }}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </svelte:fragment>
  </Dialog>
{/if}

<!-- ── Week detail dialog ────────────────────────────────────────────────────── -->
{#if selectedWeek}
  <Dialog
    title={$t("Week Approvals")}
    onClose={() => (selectedWeek = null)}
  >
    <svelte:fragment slot="title">
      <span style="flex:1">
        {$t("Week Approvals")} · {userName(selectedWeek.user_id, users)}
      </span>
    </svelte:fragment>
    <div class="tab-num" style="font-size:12px;color:var(--text-secondary)">
      {fmtWeekLabel(selectedWeek.week_start)}
    </div>

    <div style="display:flex;gap:8px;flex-wrap:wrap">
      <span class="zf-chip zf-chip-approved">{weekHours(selectedWeek)}</span>
    </div>
    <svelte:fragment slot="footer">
      <button class="zf-btn" on:click={() => (selectedWeek = null)} disabled={weekActionBusy}>
        {$t("Close")}
      </button>
      <span style="flex:1"></span>
      <button
        class="zf-btn zf-btn-danger"
        on:click={() => rejectWeek(selectedWeek)}
        disabled={weekActionBusy}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="zf-btn zf-btn-primary"
        on:click={() => approveWeek(selectedWeek)}
        disabled={weekActionBusy}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </svelte:fragment>
  </Dialog>
{/if}

<style>
  .dashboard-click-row {
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
  }

  .dashboard-click-row:hover {
    background: var(--bg-subtle);
  }

  /* Highlight ring for scroll-to-section navigation. */
  .dashboard-focus {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .absence-date-controls {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }

  .absence-week-picker {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .absence-week-range {
    color: var(--text-tertiary);
    font-size: 12px;
    min-width: 108px;
    justify-content: center;
    padding: 2px 6px;
    height: auto;
  }

  .absence-week-range:hover {
    color: var(--text-primary);
  }
  .change-diff-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .change-diff-row {
    display: grid;
    grid-template-columns: minmax(70px, auto) 1fr auto 1fr;
    gap: 8px;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-subtle);
    padding: 8px 10px;
    font-size: 12px;
  }

  .change-diff-field {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .change-diff-before {
    color: var(--text-tertiary);
    text-decoration: line-through;
  }

  .change-diff-arrow {
    color: var(--text-tertiary);
  }

  .change-diff-after {
    color: var(--text-primary);
    font-weight: 500;
  }
</style>
