<script>
  import { tick } from "svelte";
  import { fly } from "svelte/transition";
  import { api } from "../api.js";
  import { categories, currentUser, path, toast } from "../stores.js";
  import { t, absenceKindLabel, formatHours } from "../i18n.js";
  import {
    fmtDate,
    fmtDateShort,
    fmtDateTime,
    isoDate,
    addDays,
    parseDate,
    monday,
    isoWeek,
    durMin,
    dateKey,
  } from "../format.js";
  import Icon from "../Icons.svelte";
  import { confirmDialog } from "../confirm.js";
  import FlextimeChart from "../FlextimeChart.svelte";
  import DatePicker from "../DatePicker.svelte";

  // ── Approval workflow state (team leads and admins only) ──────────────────────
  let pendingEntries = [];
  let pendingWeeks = [];
  let pendingAbsences = [];
  let changeRequests = [];
  let pendingReopens = [];
  let users = [];
  let absenceDetail = null;
  let absenceDetailDlg;

  // Absence slider: browse approved absences week by week (leads/admins only).
  let absenceSliderWeek;
  let absenceSliderTeamData = [];
  let absenceSliderIsLeadView = false;
  let absenceSliderDirection = 1;

  // Week details dialog (for inspecting a single pending timesheet).
  let selectedWeek = null;
  let weekDialog;
  let weekActionBusy = false;

  // Section element refs used to scroll-to-section when navigating from a badge.
  let timesheetsSectionEl;
  let absencesSectionEl;
  let reopenSectionEl;
  let changesSectionEl;
  let focusedSection = "";
  let lastFocusSignature = "";

  // ── Reference date: today is fixed at component mount time ───────────────────
  const today = new Date();
  absenceSliderWeek = isoDate(monday(today));

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
  let chartTo = isoDate(addDays(today, -1));
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

  const reportYear = today.getFullYear();
  const currentMonthIndex = today.getMonth() + 1; // 1-based
  const currentMonthKey = `${reportYear}-${String(currentMonthIndex).padStart(2, "0")}`;

  // ── Loaders ───────────────────────────────────────────────────────────────────

  async function loadChart() {
    if (chartFrom > chartTo) return;
    chartLoading = true;
    try {
      chartData = await api(`/reports/flextime?from=${chartFrom}&to=${chartTo}`);
    } catch {
      chartData = [];
    } finally {
      chartLoading = false;
    }
  }

  function monthKey(year, month) {
    return `${year}-${String(month).padStart(2, "0")}`;
  }

  // Convert a minute count into a formatted hours string (e.g. "1:30 h").
  function hoursFromMinutes(minutes) {
    return formatHours(((minutes || 0) / 60).toFixed(1));
  }

  function monthFullySubmitted(report) {
    if (!Array.isArray(report?.days)) return false;
    return report.days.every((day) => {
      if (!day || Number(day.target_min || 0) <= 0) return true;
      if (day.absence) return true;
      const dayEntries = Array.isArray(day.entries) ? day.entries : [];
      if (!dayEntries.length) return false;
      // A day counts as submitted only when it has at least one submitted/approved
      // entry and no entries are still in draft status.
      const hasSubmittedOrApproved = dayEntries.some(
        (entry) => entry.status === "submitted" || entry.status === "approved",
      );
      const hasDraft = dayEntries.some((entry) => entry.status === "draft");
      return hasSubmittedOrApproved && !hasDraft;
    });
  }

  async function loadOvertimeSummary() {
    overtimeLoading = true;
    overtimeError = "";
    try {
      overtimeRows = await api(`/reports/overtime?year=${reportYear}`);
    } catch (error) {
      overtimeRows = [];
      overtimeError = error?.message || "Overtime data unavailable.";
    } finally {
      overtimeLoading = false;
    }
  }

  // Returns the first month (1-based) that should appear in submission checks,
  // accounting for when the user's contract began.
  function firstMonthForSubmission() {
    const userStart = $currentUser?.start_date;
    if (!userStart) return null;
    const startYear = parseInt(userStart.slice(0, 4), 10);
    const startMonth = parseInt(userStart.slice(5, 7), 10);
    if (reportYear < startYear) return null;
    return reportYear === startYear ? Math.max(startMonth, 1) : 1;
  }

  async function loadPastMonthSubmissionStatus() {
    const firstMonth = firstMonthForSubmission();
    if (firstMonth === null) {
      monthSubmissionChecks = [];
      monthSubmissionError = "";
      return;
    }
    // Build the list of months between contract start and the current month (exclusive).
    const monthsToCheck = [];
    for (let month = firstMonth; month < currentMonthIndex; month += 1) {
      monthsToCheck.push(monthKey(reportYear, month));
    }

    if (!monthsToCheck.length) {
      monthSubmissionChecks = [];
      monthSubmissionError = "";
      return;
    }

    monthSubmissionLoading = true;
    monthSubmissionError = "";
    try {
      const reports = await Promise.all(
        monthsToCheck.map((month) => api(`/reports/month?month=${month}`)),
      );
      monthSubmissionChecks = monthsToCheck.map((month, index) => ({
        month,
        submitted: monthFullySubmitted(reports[index]),
      }));
    } catch (error) {
      monthSubmissionChecks = [];
      monthSubmissionError = error?.message || "Could not check submission status.";
    } finally {
      monthSubmissionLoading = false;
    }
  }

  function setRange(days) {
    chartFrom = clampFromToUserStart(daysAgo(days - 1));
    chartTo = isoDate(addDays(today, -1));
    loadChart();
  }

  // Loads all data that is only visible to team leads and admins (can_approve).
  async function load() {
    const canApprove = !!$currentUser?.permissions?.can_approve;
    if (!canApprove) return;
    try {
      const [
        submittedTimeEntries,
        requestedAbsences,
        openChangeRequests,
        pendingReopenRequests,
        teamMembers,
      ] = await Promise.all([
        api("/time-entries/all?status=submitted"),
        api("/absences/all?status=requested"),
        api("/change-requests/all?status=open"),
        api("/reopen-requests/pending"),
        api("/users"),
      ]);
      pendingEntries = submittedTimeEntries;
      pendingAbsences = requestedAbsences;
      changeRequests = openChangeRequests;
      pendingReopens = pendingReopenRequests;
      users = teamMembers;
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  load();
  loadChart();
  loadOvertimeSummary();
  loadPastMonthSubmissionStatus();
  loadAbsenceSliderTeamData(absenceSliderWeek);

  // ── Reactive derivations: overtime balance ────────────────────────────────────

  $: pendingWeeks = buildPendingWeeks(pendingEntries, users);

  $: currentOvertimeRow =
    overtimeRows.find((row) => row.month === currentMonthKey) ??
    (overtimeRows.length ? overtimeRows[overtimeRows.length - 1] : null);
  $: overtimeBalanceMin = currentOvertimeRow?.cumulative_min || 0;
  $: currentMonthDiffMin = currentOvertimeRow?.diff_min || 0;

  // ── Reactive derivations: submission compliance ───────────────────────────────

  $: previousMonthsTotal = (() => {
    // Access $currentUser here so Svelte tracks the dependency.
    const userStart = $currentUser?.start_date;
    if (!userStart) return 0;
    const startYear = parseInt(userStart.slice(0, 4), 10);
    if (reportYear < startYear) return 0;
    const startMonth = parseInt(userStart.slice(5, 7), 10);
    const firstMonth = reportYear === startYear ? Math.max(startMonth, 1) : 1;
    return Math.max(0, currentMonthIndex - firstMonth);
  })();
  $: previousMonthsSubmitted = monthSubmissionChecks.filter((month) => month.submitted).length;
  $: allPreviousMonthsSubmitted =
    previousMonthsTotal === 0 ||
    (monthSubmissionChecks.length === previousMonthsTotal &&
      previousMonthsSubmitted === previousMonthsTotal);
  $: previousMonthsIncomplete = Math.max(0, previousMonthsTotal - previousMonthsSubmitted);

  // ── Pending-week builder (groups submitted entries by user + week) ─────────────

  function entryMinutes(entry) {
    if (!entry?.start_time || !entry?.end_time) return 0;
    const start = entry.start_time.slice(0, 5);
    const end = entry.end_time.slice(0, 5);
    return Math.max(0, durMin(start, end));
  }

  function weekStartOf(entryDate) {
    const day = dateKey(entryDate);
    if (!day) return "";
    return isoDate(monday(parseDate(day)));
  }

  function userNameFromRows(userId, userRows) {
    const user = userRows.find((u) => u.id === userId);
    return user ? `${user.first_name} ${user.last_name}` : `#${userId}`;
  }

  function buildPendingWeeks(submittedEntries, userRows) {
    // Group entries by (user_id, week_start) to create per-person per-week buckets.
    const weekGroupsByKey = new Map();

    for (const entry of submittedEntries) {
      const weekStart = weekStartOf(entry.entry_date);
      if (!weekStart) continue;
      const groupKey = `${entry.user_id}:${weekStart}`;

      if (!weekGroupsByKey.has(groupKey)) {
        weekGroupsByKey.set(groupKey, {
          key: groupKey,
          user_id: entry.user_id,
          week_start: weekStart,
          week_end: isoDate(addDays(parseDate(weekStart), 6)),
          entries: [],
          total_min: 0,
        });
      }

      const weekGroup = weekGroupsByKey.get(groupKey);
      weekGroup.entries.push(entry);
      weekGroup.total_min += entryMinutes(entry);
    }

    // Sort entries within each group chronologically, then sort groups newest-first
    // and alphabetically by employee name within the same week.
    const sortedWeekGroups = Array.from(weekGroupsByKey.values()).map((group) => ({
      ...group,
      entries: group.entries.sort((a, b) => {
        const dateDiff = dateKey(a.entry_date).localeCompare(dateKey(b.entry_date));
        if (dateDiff !== 0) return dateDiff;
        return a.start_time.localeCompare(b.start_time);
      }),
    }));

    sortedWeekGroups.sort((a, b) => {
      const weekDiff = b.week_start.localeCompare(a.week_start);
      if (weekDiff !== 0) return weekDiff;
      return userNameFromRows(a.user_id, userRows).localeCompare(
        userNameFromRows(b.user_id, userRows),
      );
    });

    return sortedWeekGroups;
  }

  // ── Reactive: keep selectedWeek in sync after a refresh ──────────────────────

  $: if (selectedWeek) {
    const next = pendingWeeks.find((week) => week.key === selectedWeek.key);
    if (!next) selectedWeek = null;
    else if (next !== selectedWeek) selectedWeek = next;
  }

  // ── Utility helpers ───────────────────────────────────────────────────────────

  function userName(userId, userRows) {
    const user = userRows.find((u) => u.id === userId);
    return user ? `${user.first_name} ${user.last_name}` : `#${userId}`;
  }

  function userInitials(userId, userRows) {
    const user = userRows.find((u) => u.id === userId);
    return user
      ? ((user.first_name?.[0] || "") + (user.last_name?.[0] || "")).toUpperCase()
      : "?";
  }

  function weekHours(week) {
    return formatHours((week.total_min / 60).toFixed(1));
  }

  function categoryName(categoryId) {
    const category = $categories.find((item) => item.id === categoryId);
    return category ? $t(category.name) : `#${categoryId}`;
  }

  function changeRequestChanges(changeRequest) {
    const lines = [];
    if (changeRequest.new_date) {
      lines.push(`${$t("Date")}: ${fmtDateShort(changeRequest.new_date)}`);
    }
    if (changeRequest.new_start_time) {
      lines.push(`${$t("Start")}: ${changeRequest.new_start_time.slice(0, 5)}`);
    }
    if (changeRequest.new_end_time) {
      lines.push(`${$t("End")}: ${changeRequest.new_end_time.slice(0, 5)}`);
    }
    if (changeRequest.new_category_id) {
      lines.push(`${$t("Category")}: ${categoryName(changeRequest.new_category_id)}`);
    }
    if (
      changeRequest.new_comment !== null &&
      changeRequest.new_comment !== undefined
    ) {
      lines.push(
        changeRequest.new_comment === ""
          ? `${$t("Comment")}: ${$t("Cleared")}`
          : `${$t("Comment")}: ${changeRequest.new_comment}`,
      );
    }
    return lines;
  }

  // ── Focus/scroll-to-section logic ────────────────────────────────────────────

  function sectionByFocus(focus) {
    if (focus === "timesheets") return timesheetsSectionEl;
    if (focus === "absences") return absencesSectionEl;
    if (focus === "reopen") return reopenSectionEl;
    if (focus === "changes") return changesSectionEl;
    return null;
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
    absenceSliderIsLeadView = $currentUser.permissions?.can_approve || false;
    if (!absenceSliderIsLeadView) return;
    try {
      const weekEnd = isoDate(addDays(parseDate(weekStartDate), 6));
      const params = new URLSearchParams({
        from: weekStartDate,
        to: weekEnd,
        status: "approved",
      });
      absenceSliderTeamData = await api(`/absences/all?${params}`);
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
      lastFocusSignature = signature;
      revealFocusSection(focusTarget);
    }
  }

  // ── Week dialog (timesheet detail view) ───────────────────────────────────────

  function openWeekDetails(week) {
    selectedWeek = week;
  }

  function closeWeekDialog() {
    selectedWeek = null;
  }

  $: if (selectedWeek) {
    tick().then(() => {
      if (!weekDialog || weekDialog.open) return;
      try {
        if (typeof weekDialog.showModal === "function") {
          weekDialog.showModal();
        } else {
          weekDialog.setAttribute("open", "open");
        }
      } catch {
        weekDialog.setAttribute("open", "open");
      }
    });
  }

  async function approveWeek(week) {
    if (!week?.entries?.length || weekActionBusy) return;
    weekActionBusy = true;
    try {
      await api("/time-entries/batch-approve", {
        method: "POST",
        body: { ids: week.entries.map((entry) => entry.id) },
      });
      toast($t("Approved."), "ok");
      closeWeekDialog();
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
      await api("/time-entries/batch-reject", {
        method: "POST",
        body: { ids: week.entries.map((entry) => entry.id), reason },
      });
      toast($t("Rejected."), "ok");
      closeWeekDialog();
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
      $t("Approve all {n} submitted entries across all users?", { n: ids.length }),
      { confirm: $t("Approve all") },
    );
    if (!confirmed) return;
    try {
      await api("/time-entries/batch-approve", { method: "POST", body: { ids } });
      toast($t("All approved."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  // ── Absence approval ──────────────────────────────────────────────────────────

  function showAbsenceDetail(absence) {
    absenceDetail = absence;
    tick().then(() => {
      if (absenceDetailDlg && !absenceDetailDlg.open) {
        try {
          absenceDetailDlg.showModal();
        } catch {
          absenceDetailDlg.setAttribute("open", "open");
        }
      }
    });
  }

  function closeAbsenceDetail() {
    if (absenceDetailDlg?.open) absenceDetailDlg.close();
    absenceDetail = null;
  }

  async function approveAbsence(id) {
    try {
      await api(`/absences/${id}/approve`, { method: "POST" });
      toast($t("Approved."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  async function rejectAbsence(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this request?"),
      { danger: true, confirm: $t("Reject"), reason: true },
    );
    if (!reason) return;
    try {
      await api(`/absences/${id}/reject`, { method: "POST", body: { reason } });
      toast($t("Rejected."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  // ── Reopen-request approval ───────────────────────────────────────────────────

  async function approveReopen(id) {
    try {
      await api(`/reopen-requests/${id}/approve`, { method: "POST", body: {} });
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
      await api(`/reopen-requests/${id}/reject`, { method: "POST", body: { reason } });
      toast($t("Rejected."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  // ── Change-request approval ───────────────────────────────────────────────────

  async function approveCR(id) {
    try {
      await api(`/change-requests/${id}/approve`, { method: "POST" });
      toast($t("Approved."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  async function rejectCR(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this change request?"),
      { danger: true, confirm: $t("Reject"), reason: true },
    );
    if (!reason) return;
    try {
      await api(`/change-requests/${id}/reject`, { method: "POST", body: { reason } });
      toast($t("Rejected."), "ok");
      load();
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Dashboard")}</h1>
  </div>
  <div class="top-bar-subtitle">
    {#if $currentUser?.permissions?.can_approve}
      {$t("Approve timesheets & manage requests")}
    {:else}
      {$t("Your overview")}
    {/if}
  </div>
</div>

<div class="content-area">

  <!-- ════════════════════════════════════════════════════════════════════════
       SECTION 1 – "Meine Bilanz": running balance & compliance (all users)
       ════════════════════════════════════════════════════════════════════════ -->
  <div class="dashboard-group">
    <div class="dashboard-group-label">{$t("My Balance")}</div>
    <div class="stat-cards">

      <!-- Cumulative overtime balance (as of yesterday) -->
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Overtime overview")}</div>
        {#if overtimeLoading}
          <div class="stat-card-value tab-num">...</div>
        {:else}
          <div
            class="stat-card-value tab-num"
            style="color:{overtimeBalanceMin < 0
              ? 'var(--danger-text)'
              : 'var(--success-text)'}"
          >
            {hoursFromMinutes(overtimeBalanceMin)}
          </div>
          <div class="stat-card-sub">
            {$t("This month: {value}", { value: hoursFromMinutes(currentMonthDiffMin) })}
          </div>
          <div class="stat-card-sub">{$t("As of yesterday")}</div>
        {/if}
        {#if overtimeError}
          <div class="error-text" style="font-size:11px;margin-top:4px">
            {$t("Overtime data unavailable.")}
          </div>
        {/if}
      </div>

      <!-- How many past months have been fully submitted -->
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Submission status")}</div>
        {#if monthSubmissionLoading}
          <div class="stat-card-value tab-num">...</div>
        {:else if previousMonthsTotal === 0}
          <div class="stat-card-value tab-num">{$t("No previous months yet")}</div>
        {:else}
          <div
            class="stat-card-value tab-num"
            style="color:{allPreviousMonthsSubmitted
              ? 'var(--success-text)'
              : 'var(--warning-text)'}"
          >
            {previousMonthsSubmitted}/{previousMonthsTotal}
          </div>
          <div class="stat-card-sub">
            {#if allPreviousMonthsSubmitted}
              {$t("All previous months submitted")}
            {:else}
              {$t("{count} month(s) incomplete", { count: previousMonthsIncomplete })}
            {/if}
          </div>
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

        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Pending Timesheets")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{pendingWeeks.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{pendingWeeks.length}</div>
        </div>

        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Absence Requests")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{pendingAbsences.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{pendingAbsences.length}</div>
        </div>

        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Change Requests")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{changeRequests.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{changeRequests.length}</div>
        </div>

        <div class="kz-card stat-card">
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
        class="kz-card"
        class:dashboard-focus={focusedSection === "timesheets"}
        style="overflow-x:auto"
        bind:this={timesheetsSectionEl}
      >
        <div class="card-header">
          <Icon name="FileText" size={15} sw={1.5} />
          <span class="card-header-title">{$t("Timesheet Approvals")}</span>
          {#if pendingWeeks.length}
            <span class="kz-chip kz-chip-submitted" style="font-size:10.5px">
              {pendingWeeks.length}
              {$t("pending")}
            </span>
            <button class="kz-btn kz-btn-sm" on:click={batchApprove}>
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
              <div style="font-size:13px;font-weight:500">
                {userName(week.user_id, users)}
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {$t("Week {week}", { week: isoWeek(parseDate(week.week_start)) })} ·
                {fmtDateShort(week.week_start)} - {fmtDateShort(week.week_end)} ·
                {weekHours(week)}
              </div>
              <div style="font-size:11px;color:var(--text-tertiary)">
                {week.entries.length} {$t("Entries")}
              </div>
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="kz-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                title={$t("Approve")}
                on:click|stopPropagation={() => approveWeek(week)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="kz-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                title={$t("Reject")}
                on:click|stopPropagation={() => rejectWeek(week)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
        {#if pendingWeeks.length === 0}
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
        class="kz-card"
        class:dashboard-focus={focusedSection === "absences"}
        style="overflow-x:auto"
        bind:this={absencesSectionEl}
      >
        <div class="card-header">
          <Icon name="Plane" size={15} sw={1.5} />
          <span class="card-header-title">{$t("Absence Requests")}</span>
          {#if pendingAbsences.length}
            <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
              {pendingAbsences.length}
              {$t("pending")}
            </span>
          {/if}
        </div>
        {#each pendingAbsences as absence}
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
              <div style="font-size:13px;font-weight:500">
                {userName(absence.user_id, users)}
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {absenceKindLabel(absence.kind)} · {fmtDateShort(absence.start_date)} -
                {fmtDateShort(absence.end_date)}
              </div>
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="kz-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                on:click={() => approveAbsence(absence.id)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="kz-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                on:click={() => rejectAbsence(absence.id)}
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

    <!-- Week reopen requests (only rendered when there are pending ones) -->
    {#if pendingReopens.length > 0}
      <div
        class="kz-card"
        class:dashboard-focus={focusedSection === "reopen"}
        style="overflow-x:auto;margin-top:16px"
        bind:this={reopenSectionEl}
      >
        <div class="card-header">
          <Icon name="Edit" size={15} sw={1.5} />
          <span class="card-header-title">{$t("Week reopen requests")}</span>
          <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
            {pendingReopens.length}
            {$t("open")}
          </span>
        </div>
        {#each pendingReopens as reopen}
          <div
            style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
          >
            <div class="avatar" style="width:30px;height:30px;font-size:11px">
              {userInitials(reopen.user_id, users)}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-size:13px;font-weight:500">
                {userName(reopen.user_id, users)}
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {$t("wants to edit week of {date}", { date: fmtDateShort(reopen.week_start) })}
              </div>
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="kz-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                title={$t("Approve")}
                on:click={() => approveReopen(reopen.id)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="kz-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                title={$t("Reject")}
                on:click={() => rejectReopen(reopen.id)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- "Who is absent" team calendar widget -->
    <div class="kz-card" style="padding:16px 20px;margin-top:16px">
      <div
        style="display:flex;align-items:flex-start;gap:10px;flex-wrap:wrap;margin-bottom:14px"
      >
        <Icon name="Users" size={15} sw={1.5} />
        <span style="font-size:14px;font-weight:400;flex:1">{$t("Who is absent")}</span>
        <div class="absence-date-controls">
          <div class="absence-week-picker">
            <button
              class="kz-btn kz-btn-icon-sm kz-btn-ghost"
              on:click={absenceSliderPrevWeek}
              aria-label={$t("Previous week")}
            >
              <Icon name="ChevLeft" size={16} />
            </button>
            <span class="absence-week-range">
              {fmtDateShort(absenceSliderWeek)} -
              {fmtDateShort(isoDate(addDays(parseDate(absenceSliderWeek), 6)))}
            </span>
            <button
              class="kz-btn kz-btn-icon-sm kz-btn-ghost"
              on:click={absenceSliderNextWeek}
              aria-label={$t("Next week")}
            >
              <Icon name="ChevRight" size={16} />
            </button>
          </div>
          <button class="kz-btn kz-btn-sm" on:click={absenceSliderToToday}>
            {$t("Today")}
          </button>
        </div>
      </div>

      {#key absenceSliderWeek}
        <div in:fly={{ x: absenceSliderDirection * 80, duration: 200 }} style="overflow:hidden">
          {#if absenceSliderTeamData.length === 0}
            <div style="padding:12px;color:var(--text-tertiary);font-size:13px">
              {$t("No absences this week.")}
            </div>
          {:else}
            <div style="display:flex;flex-direction:column;gap:8px">
              {#each absenceSliderTeamData as absence}
                {@const absentUser = users.find((u) => u.id === absence.user_id)}
                <div
                  style="padding:12px;border-left:3px solid var(--border);background:var(--bg-muted);border-radius:var(--radius-sm);display:flex;justify-content:space-between;align-items:center"
                >
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
            </div>
          {/if}
        </div>
      {/key}
    </div>

    <!-- Change requests table (only rendered when there are open ones) -->
    {#if changeRequests.length > 0}
      <div
        class="kz-card"
        class:dashboard-focus={focusedSection === "changes"}
        style="overflow-x:auto;margin-top:16px"
        bind:this={changesSectionEl}
      >
        <div class="card-header">
          <Icon name="Edit" size={15} sw={1.5} />
          <span class="card-header-title">{$t("Change Requests")}</span>
          <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
            {changeRequests.length}
            {$t("open")}
          </span>
        </div>
        <table class="kz-table">
          <thead>
            <tr>
              <th>{$t("Employee")}</th>
              <th>{$t("Created")}</th>
              <th>{$t("Request")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each changeRequests as cr}
              <tr>
                <td style="font-weight:500">{userName(cr.user_id, users)}</td>
                <td class="tab-num">{fmtDate(cr.created_at)}</td>
                <td
                  style="max-width:300px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
                >
                  <div style="display:flex;flex-direction:column;gap:4px;white-space:normal">
                    <div>{cr.reason || "-"}</div>
                    {#each changeRequestChanges(cr) as change}
                      <div style="font-size:11.5px;color:var(--text-tertiary)">{change}</div>
                    {/each}
                  </div>
                </td>
                <td style="text-align:right">
                  <div style="display:flex;gap:4px;justify-content:flex-end">
                    <button
                      class="kz-btn-icon-sm"
                      style="color:var(--success-text);background:var(--success-soft)"
                      on:click={() => approveCR(cr.id)}
                    >
                      <Icon name="Check" size={14} />
                    </button>
                    <button
                      class="kz-btn-icon-sm"
                      style="color:var(--danger-text);background:var(--danger-soft)"
                      on:click={() => rejectCR(cr.id)}
                    >
                      <Icon name="X" size={14} />
                    </button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}

  <!-- ════════════════════════════════════════════════════════════════════════
       FLEXTIME CHART (all users) – placed after approval sections so it
       doesn't push urgent approval work below the fold for leads/admins.
       ════════════════════════════════════════════════════════════════════════ -->
  <div class="kz-card" style="padding:16px 20px;margin-top:16px">
    <div
      style="display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:14px"
    >
      <Icon name="TrendingUp" size={15} sw={1.5} />
      <span style="font-size:14px;font-weight:400;flex:1">{$t("Flextime balance")}</span>
      <div style="display:flex;gap:4px;flex-wrap:wrap">
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(30)}
          >{$t("Last 30 days")}</button
        >
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(90)}
          >{$t("Last 90 days")}</button
        >
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(182)}
          >{$t("Last 6 months")}</button
        >
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(365)}
          >{$t("Last year")}</button
        >
      </div>
      <div style="display:flex;align-items:center;gap:4px">
        <DatePicker
          bind:value={chartFrom}
          max={chartTo}
          style="font-size:12px;padding:3px 6px;height:28px"
        />
        <span style="font-size:12px;color:var(--text-tertiary)">-</span>
        <DatePicker
          bind:value={chartTo}
          min={chartFrom}
          style="font-size:12px;padding:3px 6px;height:28px"
        />
        <button class="kz-btn kz-btn-sm" on:click={loadChart} aria-label={$t("Show")}>
          <Icon name="Search" size={13} />
        </button>
      </div>
    </div>
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

</div>

<!-- ── Absence detail dialog ─────────────────────────────────────────────────── -->
{#if absenceDetail}
  <dialog bind:this={absenceDetailDlg} on:close={closeAbsenceDetail}>
    <header>
      <span style="flex:1">{$t("Absence Request Details")}</span>
      <button class="kz-btn-icon-sm kz-btn-ghost" on:click={closeAbsenceDetail}>
        <Icon name="X" size={16} />
      </button>
    </header>
    <div class="dialog-body">
      <div style="display:flex;flex-direction:column;gap:10px">
        <div>
          <div class="kz-label">{$t("Employee")}</div>
          <div style="font-weight:500">{userName(absenceDetail.user_id, users)}</div>
        </div>
        <div>
          <div class="kz-label">{$t("Type")}</div>
          <div>{absenceKindLabel(absenceDetail.kind)}</div>
        </div>
        <div class="field-row">
          <div>
            <div class="kz-label">{$t("From")}</div>
            <div class="tab-num">{fmtDate(absenceDetail.start_date)}</div>
          </div>
          <div>
            <div class="kz-label">{$t("To")}</div>
            <div class="tab-num">{fmtDate(absenceDetail.end_date)}</div>
          </div>
        </div>
        {#if absenceDetail.comment}
          <div>
            <div class="kz-label">{$t("Comment")}</div>
            <div style="white-space:pre-wrap;font-size:13px">{absenceDetail.comment}</div>
          </div>
        {/if}
        <div>
          <div class="kz-label">{$t("Requested at")}</div>
          <div class="tab-num" style="font-size:12px">
            {fmtDateTime(absenceDetail.created_at)}
          </div>
        </div>
      </div>
    </div>
    <footer>
      <button class="kz-btn" on:click={closeAbsenceDetail}>{$t("Close")}</button>
      <span style="flex:1"></span>
      <button
        class="kz-btn kz-btn-danger"
        on:click={() => {
          const absenceId = absenceDetail.id;
          closeAbsenceDetail();
          rejectAbsence(absenceId);
        }}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="kz-btn kz-btn-primary"
        on:click={() => {
          const absenceId = absenceDetail.id;
          closeAbsenceDetail();
          approveAbsence(absenceId);
        }}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </footer>
  </dialog>
{/if}

<!-- ── Week detail dialog ────────────────────────────────────────────────────── -->
{#if selectedWeek}
  <dialog bind:this={weekDialog} on:close={closeWeekDialog}>
    <header>
      <span style="flex:1">
        {$t("Timesheet Approvals")} · {userName(selectedWeek.user_id, users)}
      </span>
      <button class="kz-btn-icon-sm kz-btn-ghost" on:click={closeWeekDialog}>
        <Icon name="X" size={16} />
      </button>
    </header>
    <div class="dialog-body">
      <div class="tab-num" style="font-size:12px;color:var(--text-secondary)">
        {$t("Week {week}", { week: isoWeek(parseDate(selectedWeek.week_start)) })} ·
        {fmtDateShort(selectedWeek.week_start)} - {fmtDateShort(selectedWeek.week_end)}
      </div>

      <div style="display:flex;gap:8px;flex-wrap:wrap">
        <span class="kz-chip kz-chip-submitted">
          {selectedWeek.entries.length} {$t("Entries")}
        </span>
        <span class="kz-chip kz-chip-approved">{weekHours(selectedWeek)}</span>
      </div>

      <div class="week-entry-list">
        {#each selectedWeek.entries as entry (entry.id)}
          <div class="week-entry-row">
            <div style="font-size:12.5px;font-weight:500">
              {fmtDateShort(entry.entry_date)}
            </div>
            <div class="tab-num" style="font-size:12px;color:var(--text-secondary)">
              {entry.start_time.slice(0, 5)} - {entry.end_time.slice(0, 5)} ·
              {formatHours((entryMinutes(entry) / 60).toFixed(1))}
            </div>
            {#if entry.comment}
              <div style="font-size:11.5px;color:var(--text-tertiary)">{entry.comment}</div>
            {/if}
          </div>
        {/each}
      </div>
    </div>
    <footer>
      <button class="kz-btn" on:click={closeWeekDialog} disabled={weekActionBusy}>
        {$t("Close")}
      </button>
      <span style="flex:1"></span>
      <button
        class="kz-btn kz-btn-danger"
        on:click={() => rejectWeek(selectedWeek)}
        disabled={weekActionBusy}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="kz-btn kz-btn-primary"
        on:click={() => approveWeek(selectedWeek)}
        disabled={weekActionBusy}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </footer>
  </dialog>
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
    gap: 4px;
  }

  .absence-week-range {
    color: var(--text-tertiary);
    font-size: 12px;
    min-width: 120px;
    text-align: center;
  }

  .week-entry-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: min(52vh, 420px);
    overflow: auto;
    padding-right: 2px;
  }

  .week-entry-row {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
    background: var(--bg-subtle);
  }
</style>
