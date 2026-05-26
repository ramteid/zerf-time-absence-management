<script>
  import { tick } from "svelte";
  import { categories, currentUser, path, settings, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { isoDate, appTodayDate, addDays } from "../format.js";
  import { confirmDialog } from "../confirm.js";
  import { isAssistantUser, tracksOwnTime } from "../rolePolicy.js";
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
    allMonthsToCheck,
    buildPendingWeeks,
    buildSubmissionChecks,
    currentWeekIsOpen,
  } from "../lib/domain/dashboard.js";
  import AbsenceReviewDialog from "../dialogs/AbsenceReviewDialog.svelte";
  import ReopenReviewDialog from "../dialogs/ReopenReviewDialog.svelte";
  import WeekReviewDialog from "../dialogs/WeekReviewDialog.svelte";
  import ApprovalQueues from "./dashboard/ApprovalQueues.svelte";
  import AbsenceSlider from "./dashboard/AbsenceSlider.svelte";
  import BalanceSection from "./dashboard/BalanceSection.svelte";
  import FlextimeSection from "./dashboard/FlextimeSection.svelte";
  import TeamSummary from "./dashboard/TeamSummary.svelte";

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
  // Pure-admin users (tracks_time=false) have no own time/absence data, so the
  // personal Balance and Flextime panels are suppressed for them.
  $: hasOwnTrackingData = tracksOwnTime($currentUser);

  // ── Loaders ───────────────────────────────────────────────────────────────────

  async function loadChart() {
    if (chartFrom > chartTo) return;
    // Use the reactive vars (not raw $currentUser) so that an uninitialised
    // undefined value doesn't look like a pure-admin "false" and kill the load.
    if (isAssistantCurrentUser || hasOwnTrackingData === false) {
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
    // Same guard: use reactive vars so undefined doesn't suppress the load.
    if (isAssistantCurrentUser || hasOwnTrackingData === false) {
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
    // Same guard: use reactive var so undefined doesn't suppress the load.
    if (hasOwnTrackingData === false) {
      monthSubmissionChecks = [];
      monthSubmissionError = "";
      monthSubmissionLoading = false;
      return;
    }
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
  {#if hasOwnTrackingData}
    <BalanceSection
      {isAssistantCurrentUser}
      {overtimeLoading}
      {submittedOvertimeBalanceMin}
      {overtimeBalanceMin}
      {currentMonthDiffMin}
      {overtimeError}
      {monthSubmissionLoading}
      {allWeeksApproved}
      {allWeeksSubmitted}
      {currentWeekOpen}
      {monthSubmissionError}
      {activeHelp}
      onHelpToggle={toggleHelp}
    />
  {/if}

  {#if $currentUser?.permissions?.can_approve}
    <TeamSummary {pendingWeeks} {pendingAbsences} {users} />
  {/if}

  {#if $currentUser?.permissions?.can_approve}
    <ApprovalQueues
      {pendingWeeks}
      {pendingReopens}
      {pendingAbsences}
      {users}
      {focusedSection}
      bind:timesheetsSectionEl
      bind:absencesSectionEl
      onBatchApprove={batchApprove}
      onOpenWeekDetails={openWeekDetails}
      onApproveWeek={approveWeek}
      onRejectWeek={rejectWeek}
      onOpenReopenDetail={openReopenDetail}
      onApproveReopen={approveReopen}
      onRejectReopen={rejectReopen}
      onShowAbsenceDetail={showAbsenceDetail}
      onApproveAbsence={approveAbsence}
      onRejectAbsence={rejectAbsence}
    />

    <AbsenceSlider {users} />
  {/if}

  {#if !isAssistantCurrentUser && hasOwnTrackingData}
    <FlextimeSection
      bind:chartFrom
      bind:chartTo
      {todayIso}
      {chartData}
      {chartLoading}
      {activeHelp}
      onHelpToggle={toggleHelp}
      onSetRange={setRange}
      onLoadChart={loadChart}
    />
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
