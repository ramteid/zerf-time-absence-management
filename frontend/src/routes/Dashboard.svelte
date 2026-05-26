<script>
  import { tick } from "svelte";
  import { categories, currentUser, path, settings, toast } from "../stores.js";
  import { t, formatHours, absenceKindLabel } from "../i18n.js";
  import {
    fmtDateShort,
    fmtWeekLabel,
    isoDate,
    appTodayDate,
    addDays,
  } from "../format.js";
  import Icon from "../Icons.svelte";
  import FlextimeChart from "../FlextimeChart.svelte";
  import DatePicker from "../DatePicker.svelte";
  import { confirmDialog } from "../confirm.js";
  import { isAssistantUser } from "../rolePolicy.js";
  import {
    approveAbsenceById,
    approveReopen as approveReopenRequest,
    approveWeek as approveWeekEntries,
    getApprovalDashboard,
    getFlextime,
    getMonthSubmissionReport,
    getOvertimeSummary,
    rejectAbsenceById,
    rejectReopen as rejectReopenRequest,
    rejectWeek as rejectWeekEntries,
  } from "../lib/api/dashboardApi.js";
  import {
    absenceRequestTypeLabelKey,
    allMonthsToCheck,
    buildPendingWeeks,
    buildSubmissionChecks,
    currentWeekIsOpen,
  } from "../lib/domain/dashboard.js";
  import {
    userInitialsFromRows,
    userNameFromRows,
  } from "../lib/domain/users.js";
  import AbsenceReviewDialog from "../dialogs/AbsenceReviewDialog.svelte";
  import ReopenReviewDialog from "../dialogs/ReopenReviewDialog.svelte";
  import WeekReviewDialog from "../dialogs/WeekReviewDialog.svelte";
  import AbsenceSlider from "./dashboard/AbsenceSlider.svelte";

  // ── Approval workflow state (team leads and admins only) ──────────────────────
  let pendingEntries = [];
  let pendingWeeks = [];
  let pendingAbsences = [];
  let pendingReopens = [];
  let users = [];
  let absenceDetail = null;
  let requestDetail = null;

  // Week details dialog (for inspecting a single pending timesheet).
  let selectedWeek = null;
  let weekActionBusy = false;

  // Section element refs used to scroll-to-section when navigating from a badge.
  let timesheetsSectionEl;
  let absencesSectionEl;
  let focusedSection = "";
  let lastFocusSignature = "";

  // ── Reference date: derived from configured app timezone ─────────────────────
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

  // Loads data only visible to team leads and admins (can_approve).
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

  // ── Reactive derivations: overtime balance ────────────────────────────────────

  $: pendingWeeks = buildPendingWeeks(pendingEntries, users, $categories);

  $: currentOvertimeRow =
    overtimeRows.find((row) => row.month === currentMonthKey) ??
    (overtimeRows.length ? overtimeRows[overtimeRows.length - 1] : null);
  $: overtimeBalanceMin = currentOvertimeRow?.cumulative_min || 0;
  $: submittedOvertimeBalanceMin = currentOvertimeRow?.submitted_cumulative_min ?? overtimeBalanceMin;
  $: currentMonthDiffMin = currentOvertimeRow?.diff_min || 0;

  // ── Reactive derivations: submission compliance ───────────────────────────────

  $: allWeeksSubmitted =
    monthSubmissionChecks.length === 0 ||
    monthSubmissionChecks.every((check) => check.submitted);

  $: allWeeksApproved =
    allWeeksSubmitted &&
    (monthSubmissionChecks.length === 0 ||
      monthSubmissionChecks.every((check) => check.approved));

  $: currentWeekOpen = currentWeekIsOpen(monthSubmissionChecks);

  // ── Reactive: keep selectedWeek in sync after a refresh ──────────────────────

  $: if (selectedWeek) {
    const next = pendingWeeks.find((week) => week.key === selectedWeek.key);
    if (!next) selectedWeek = null;
    else if (next !== selectedWeek) selectedWeek = next;
  }

  // ── Focus/scroll-to-section logic ────────────────────────────────────────────

  function sectionByFocus(focus) {
    if (focus === "timesheets") return timesheetsSectionEl;
    if (focus === "absences") return absencesSectionEl;
    if (focus === "reopen") return timesheetsSectionEl;
    return null;
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

  // ── URL-driven section focus ──────────────────────────────────────────────────

  $: dashboardQuery = (() => {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(queryString);
  })();

  $: focusTarget = dashboardQuery.get("focus") || "";
  $: focusNonce = dashboardQuery.get("n") || "";

  $: {
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
    const result = await confirmDialog(
      isCancellation ? $t("Reject cancellation?") : $t("Reject?"),
      isCancellation
        ? $t("Reject this cancellation request? The absence will remain approved.")
        : $t("Reject this request?"),
      { danger: true, confirm: $t("Reject"), reason: !isCancellation },
    );
    if (!result) return;
    try {
      await rejectAbsenceById(absence, isCancellation ? undefined : result);
      toast($t("Rejected."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
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
              {formatHours((submittedOvertimeBalanceMin || 0) / 60)}
            </div>
            <div class="stat-card-sub">
              {#if submittedOvertimeBalanceMin !== overtimeBalanceMin}
                {$t("Approved: {value}", { value: formatHours((overtimeBalanceMin || 0) / 60) })}
              {:else}
                {$t("This month: {value}", { value: formatHours((currentMonthDiffMin || 0) / 60) })}
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
              {userInitialsFromRows(week.user_id, users) || "?"}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userNameFromRows(week.user_id, users)}
                <span class="zf-chip zf-chip-submitted" style="font-size:10px">{$t("Approval")}</span>
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {fmtWeekLabel(week.week_start)} · {formatHours(week.total_min / 60)}
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
              {userInitialsFromRows(reopen.user_id, users) || "?"}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userNameFromRows(reopen.user_id, users)}
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
              {userInitialsFromRows(absence.user_id, users) || "?"}
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
                {userNameFromRows(absence.user_id, users)}
                <span
                  class="zf-chip {absence.status === 'cancellation_pending' ? 'zf-chip-cancellation_pending' : 'zf-chip-warning'}"
                  style="font-size:10px"
                >
                  {$t(absenceRequestTypeLabelKey(absence))}
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

    <AbsenceSlider {users} />

  {/if}

  <!-- ════════════════════════════════════════════════════════════════════════
       FLEXTIME CHART (all users)
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

{#if absenceDetail}
  <AbsenceReviewDialog
    absence={absenceDetail}
    {users}
    onClose={() => (absenceDetail = null)}
    onApprove={(absence) => {
      absenceDetail = null;
      approveAbsence(absence);
    }}
    onReject={(absence) => {
      absenceDetail = null;
      rejectAbsence(absence);
    }}
  />
{/if}

{#if requestDetail}
  <ReopenReviewDialog
    item={requestDetail.item}
    {users}
    onClose={() => (requestDetail = null)}
    onApprove={(id) => {
      requestDetail = null;
      approveReopen(id);
    }}
    onReject={(id) => {
      requestDetail = null;
      rejectReopen(id);
    }}
  />
{/if}

{#if selectedWeek}
  <WeekReviewDialog
    week={selectedWeek}
    {users}
    busy={weekActionBusy}
    onClose={() => (selectedWeek = null)}
    onApprove={approveWeek}
    onReject={rejectWeek}
  />
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

  .dashboard-focus {
    box-shadow: 0 0 0 2px var(--accent);
  }
</style>
