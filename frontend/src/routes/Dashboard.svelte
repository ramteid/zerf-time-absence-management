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
  let allTimeEntries = [];
  let users = [];
  let absenceDetail = null;
  let absenceDetailDlg;
  let requestDetail = null;
  let requestDetailDlg;

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
  let currentMonthSubmitted = true;

  const reportYear = today.getFullYear();
  const currentMonthIndex = today.getMonth() + 1; // 1-based
  const currentMonthKey = `${reportYear}-${String(currentMonthIndex).padStart(2, "0")}`;
  const todayIso = isoDate(today);

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

  // A month report is considered fully submitted for week-tracking purposes
  // when the backend's weeks_all_submitted flag is true.
  function monthFullySubmitted(report) {
    return report?.weeks_all_submitted === true;
  }

  // Current week is treated as submitted only when every required workday up
  // to today has no draft entries and at least one submitted/approved entry.
  function currentWeekFullySubmitted(report) {
    if (!report?.days?.length) return true;
    const weekStart = isoDate(monday(today));
    return report.days
      .filter(
        (day) => day?.target_min > 0 && day?.date >= weekStart && day?.date <= todayIso,
      )
      .every((day) => {
        const entries = Array.isArray(day.entries) ? day.entries : [];
        const hasDraft = entries.some(
          (entry) => entry?.status === "draft" && entryCountsAsWork(entry),
        );
          // Any approved absence (target-removing or flextime_reduction) covers the day for
        // submission purposes: flextime_reduction blocks entry creation, so there is nothing
        // to submit on those days either.
        const hasAnyAbsence = !!day?.absence;
        const hasCreditedSubmittedOrApproved = entries.some((entry) => {
          if (entry?.status !== "submitted" && entry?.status !== "approved") {
            return false;
          }
          return entryCountsAsWork(entry);
        });
        return !hasDraft && (hasAnyAbsence || hasCreditedSubmittedOrApproved);
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

  // Builds all YYYY-MM month keys from the user's start month (inclusive) to
  // the current month (inclusive), spanning multiple years. The backend's
  // weeks_all_submitted flag correctly limits checking to fully elapsed weeks
  // (Sunday < today), including boundary weeks across month edges.
  function allMonthsToCheck() {
    const userStart = $currentUser?.start_date;
    if (!userStart) return [];
    const startYear = parseInt(userStart.slice(0, 4), 10);
    const startMonth = parseInt(userStart.slice(5, 7), 10);
    const endYear = today.getFullYear();
    const endMonth = today.getMonth() + 1;
    if (startYear > endYear || (startYear === endYear && startMonth > endMonth)) return [];
    const months = [];
    for (let y = startYear; y <= endYear; y++) {
      const fromMonth = y === startYear ? startMonth : 1;
      const toMonth = y === endYear ? endMonth : 12;
      for (let m = fromMonth; m <= toMonth; m++) {
        months.push(monthKey(y, m));
      }
    }
    return months;
  }

  async function loadPastMonthSubmissionStatus() {
    const monthsToCheck = allMonthsToCheck();
    if (!monthsToCheck.length) {
      monthSubmissionChecks = [];
      currentMonthSubmitted = true;
      return;
    }

    monthSubmissionLoading = true;
    monthSubmissionError = "";
    try {
      const requests = monthsToCheck.map((month) => api(`/reports/month?month=${month}`));
      const reports = await Promise.all(requests);
      const currentMonthReport = reports[reports.length - 1];
      monthSubmissionChecks = monthsToCheck.map((month, index) => ({
        month,
        submitted: monthFullySubmitted(reports[index]),
      }));
      currentMonthSubmitted = currentWeekFullySubmitted(currentMonthReport);
    } catch (error) {
      monthSubmissionChecks = [];
      currentMonthSubmitted = true;
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
    if (!canApprove) return;
    try {
      const [
        teamTimeEntries,
        submittedTimeEntries,
        requestedAbsences,
        openChangeRequests,
        pendingReopenRequests,
        teamMembers,
      ] = await Promise.all([
        api("/time-entries/all"),
        api("/time-entries/all?status=submitted"),
        api("/absences/all?status=pending_review"),
        api("/change-requests/all?status=open"),
        api("/reopen-requests/pending"),
        api("/users"),
      ]);
      allTimeEntries = teamTimeEntries;
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
  $: timeEntryById = new Map(allTimeEntries.map((entry) => [entry.id, entry]));
  $: pendingReopenWeekKeys = new Set(
    pendingReopens.map((reopen) => `${reopen.user_id}:${dateKey(reopen.week_start)}`),
  );
  $: visibleChangeRequests = changeRequests.filter(
    (changeRequest) =>
      !pendingReopenWeekKeys.has(
        `${changeRequest.user_id}:${dateKey(changeRequestWeekStart(changeRequest))}`,
      ),
  );

  $: currentOvertimeRow =
    overtimeRows.find((row) => row.month === currentMonthKey) ??
    (overtimeRows.length ? overtimeRows[overtimeRows.length - 1] : null);
  $: overtimeBalanceMin = currentOvertimeRow?.cumulative_min || 0;
  $: currentMonthDiffMin = currentOvertimeRow?.diff_min || 0;

  // ── Reactive derivations: submission compliance ───────────────────────────────

  // True when every month from the user's start to now has weeks_all_submitted.
  // Empty checks (no start date, or start date in the future) count as "all done".
  // The current month is checked separately to include the current week.
  $: allWeeksSubmitted =
    (monthSubmissionChecks.length === 0 ||
      monthSubmissionChecks.every((check) => check.submitted)) &&
    currentMonthSubmitted;

  // ── Pending-week builder (groups submitted entries by user + week) ─────────────

  function entryCountsAsWork(entry) {
    if (entry?.counts_as_work === false) return false;
    if (entry?.counts_as_work === true) return true;

    if (entry?.category_id != null) {
      const categoryById = $categories.find((item) => item.id === entry.category_id);
      if (categoryById) return categoryById.counts_as_work !== false;
    }

    if (entry?.category) {
      const categoryByName = $categories.find((item) => item.name === entry.category);
      if (categoryByName) return categoryByName.counts_as_work !== false;
    }

    return true;
  }

  function entryMinutes(entry) {
    if (!entry?.start_time || !entry?.end_time || !entryCountsAsWork(entry)) {
      return 0;
    }
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

  function weekEntryTypeSummary(week) {
    const types = Array.from(
      new Set((week?.entries || []).map((entry) => categoryName(entry.category_id))),
    );
    return types.join(", ");
  }

  function categoryName(categoryId) {
    const category = $categories.find((item) => item.id === categoryId);
    return category ? $t(category.name) : `#${categoryId}`;
  }

  function changeRequestWeekStart(changeRequest) {
    const entry = timeEntryById.get(changeRequest.time_entry_id);
    return entry ? weekStartOf(entry.entry_date) : "";
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
    if (focus === "reopen") return timesheetsSectionEl;
    if (focus === "changes") return timesheetsSectionEl;
    return null;
  }

  function changeRequestTypeLabel(changeRequest) {
    return $t("Change");
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

  function openRequestDetail(kind, item) {
    requestDetail = { kind, item };
    tick().then(() => {
      if (requestDetailDlg && !requestDetailDlg.open) {
        try {
          requestDetailDlg.showModal();
        } catch {
          requestDetailDlg.setAttribute("open", "open");
        }
      }
    });
  }

  function closeRequestDetail() {
    if (requestDetailDlg?.open) requestDetailDlg.close();
    requestDetail = null;
  }

  function changeDiffRows(changeRequest) {
    const currentEntry = timeEntryById.get(changeRequest.time_entry_id);
    if (!currentEntry) return [];

    const rows = [];
    if (changeRequest.new_date) {
      rows.push({
        field: $t("Date"),
        before: fmtDateShort(currentEntry.entry_date),
        after: fmtDateShort(changeRequest.new_date),
      });
    }
    if (changeRequest.new_start_time) {
      rows.push({
        field: $t("Start"),
        before: currentEntry.start_time.slice(0, 5),
        after: changeRequest.new_start_time.slice(0, 5),
      });
    }
    if (changeRequest.new_end_time) {
      rows.push({
        field: $t("End"),
        before: currentEntry.end_time.slice(0, 5),
        after: changeRequest.new_end_time.slice(0, 5),
      });
    }
    if (changeRequest.new_category_id) {
      rows.push({
        field: $t("Category"),
        before: categoryName(currentEntry.category_id),
        after: categoryName(changeRequest.new_category_id),
      });
    }
    if (
      changeRequest.new_comment !== null &&
      changeRequest.new_comment !== undefined
    ) {
      rows.push({
        field: $t("Comment"),
        before: currentEntry.comment || $t("Empty"),
        after:
          changeRequest.new_comment === ""
            ? $t("Cleared")
            : changeRequest.new_comment,
      });
    }
    return rows;
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
      $t("Approve all {n} submissions across all users?", { n: pendingWeeks.length }),
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

  async function approveAbsence(absence) {
    const isCancellation = absence.status === "cancellation_pending";
    const endpoint = isCancellation
      ? `/absences/${absence.id}/approve-cancellation`
      : `/absences/${absence.id}/approve`;
    try {
      await api(endpoint, { method: "POST" });
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
        await api(`/absences/${absence.id}/reject-cancellation`, { method: "POST" });
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
        await api(`/absences/${absence.id}/reject`, { method: "POST", body: { reason } });
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

  // ── Help tooltips ─────────────────────────────────────────────────────────────
  let activeHelp = null;
  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
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
    <div class="dashboard-group-label" style="display:flex;align-items:center;gap:6px">
      {$t("My Balance")}
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
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

      <!-- Cumulative overtime balance including today -->
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
        {/if}
        {#if overtimeError}
          <div class="error-text" style="font-size:11px;margin-top:4px">
            {$t("Overtime data unavailable.")}
          </div>
        {/if}
      </div>

      <!-- Whether all weeks since the user's start date (up to last week) are submitted -->
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Submissions")}</div>
        {#if monthSubmissionLoading}
          <div class="stat-card-value tab-num">...</div>
        {:else}
          <div
            class="stat-card-value tab-num"
            style="color:{allWeeksSubmitted
              ? 'var(--success-text)'
              : 'var(--warning-text)'}"
          >
            {allWeeksSubmitted ? $t("All submitted") : $t("Weeks missing")}
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
          <div class="stat-card-label">{$t("Change Requests")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{visibleChangeRequests.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{visibleChangeRequests.length}</div>
        </div>

        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Absence Requests")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{pendingAbsences.length > 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >{pendingAbsences.length}</div>
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
          {#if pendingWeeks.length + pendingReopens.length + visibleChangeRequests.length > 0}
            <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
              {pendingWeeks.length + pendingReopens.length + visibleChangeRequests.length}
              {$t("pending")}
            </span>
          {/if}
          {#if pendingWeeks.length}
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
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userName(week.user_id, users)}
                <span class="kz-chip kz-chip-submitted" style="font-size:10px">{$t("Approval")}</span>
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {$t("Week {week}", { week: isoWeek(parseDate(week.week_start)) })} ·
                {fmtDateShort(week.week_start)} - {fmtDateShort(week.week_end)} ·
                {weekHours(week)}
              </div>
              <div style="font-size:11px;color:var(--text-tertiary)">
                {week.entries.length} {$t("Days")}
              </div>
              <div style="font-size:11px;color:var(--text-tertiary)">
                {$t("Type")}: {weekEntryTypeSummary(week)}
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
        {#each pendingReopens as reopen}
          <div
            class="dashboard-click-row"
            on:click={() => openRequestDetail("reopen", reopen)}
            on:keydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openRequestDetail("reopen", reopen);
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
                <span class="kz-chip kz-chip-pending" style="font-size:10px">{$t("Reopen")}</span>
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
                on:click|stopPropagation={() => approveReopen(reopen.id)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="kz-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                title={$t("Reject")}
                on:click|stopPropagation={() => rejectReopen(reopen.id)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
        {#each visibleChangeRequests as cr}
          <div
            class="dashboard-click-row"
            on:click={() => openRequestDetail("change", cr)}
            on:keydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openRequestDetail("change", cr);
              }
            }}
            role="button"
            tabindex="0"
            title={$t("Show details")}
          >
            <div class="avatar" style="width:30px;height:30px;font-size:11px">
              {userInitials(cr.user_id, users)}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userName(cr.user_id, users)}
                <span class="kz-chip kz-chip-warning" style="font-size:10px">
                  {changeRequestTypeLabel(cr)}
                </span>
              </div>
              <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
                {fmtDateShort(cr.created_at)}
              </div>
              {#if cr.reason}
                <div style="font-size:11.5px;color:var(--text-secondary);margin-top:2px">{cr.reason}</div>
              {/if}
            </div>
            <div style="display:flex;gap:4px">
              <button
                class="kz-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                title={$t("Approve")}
                on:click|stopPropagation={() => approveCR(cr.id)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="kz-btn-icon-sm"
                style="color:var(--danger-text);background:var(--danger-soft)"
                title={$t("Reject")}
                on:click|stopPropagation={() => rejectCR(cr.id)}
              >
                <Icon name="X" size={14} />
              </button>
            </div>
          </div>
        {/each}
        {#if pendingWeeks.length === 0 && pendingReopens.length === 0 && visibleChangeRequests.length === 0}
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
              <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
                {userName(absence.user_id, users)}
                <span
                  class="kz-chip {absence.status === 'cancellation_pending' ? 'kz-chip-cancellation_pending' : 'kz-chip-warning'}"
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
                class="kz-btn-icon-sm"
                style="color:var(--success-text);background:var(--success-soft)"
                on:click={() => approveAbsence(absence)}
              >
                <Icon name="Check" size={14} />
              </button>
              <button
                class="kz-btn-icon-sm"
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
    <div class="kz-card" style="margin-top:16px">
      <div class="card-header">
        <Icon name="Users" size={15} sw={1.5} />
        <span class="card-header-title">{$t("Who is absent")}</span>
        <div class="absence-date-controls">
          <div class="absence-week-picker">
            <button
              class="kz-btn kz-btn-icon-sm kz-btn-ghost"
              on:click={absenceSliderPrevWeek}
              aria-label={$t("Previous week")}
            >
              <Icon name="ChevLeft" size={16} />
            </button>
            <button
              class="kz-btn kz-btn-ghost absence-week-range"
              on:click={absenceSliderToToday}
              title={$t("Today")}
            >
              {fmtDateShort(absenceSliderWeek)} -
              {fmtDateShort(isoDate(addDays(parseDate(absenceSliderWeek), 6)))}
            </button>
            <button
              class="kz-btn kz-btn-icon-sm kz-btn-ghost"
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
              {#each absenceSliderTeamData as absence}
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
  <div class="kz-card" style="padding:16px 20px;margin-top:16px">
    <div
      style="display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:14px"
    >
      <Icon name="TrendingUp" size={15} sw={1.5} />
      <span style="font-size:14px;font-weight:400;flex:1">{$t("Flextime balance")}</span>
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        title={$t("help_flextime_chart")}
        on:click={() => toggleHelp("flextime")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
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
          <div class="kz-label">{$t("Absence Type")}</div>
          <div>{absenceKindLabel(absenceDetail.kind)}</div>
        </div>
        <div>
          <div class="kz-label">{$t("Request Type")}</div>
          <div>
            <span
              class="kz-chip {absenceDetail.status === 'cancellation_pending' ? 'kz-chip-cancellation_pending' : 'kz-chip-warning'}"
            >
              {absenceRequestTypeLabel(absenceDetail)}
            </span>
          </div>
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
        {#if absenceDetail.review_type === "change"}
          {@const diffRows = absenceDiffRows(absenceDetail)}
          {#if diffRows.length}
            <div>
              <div class="kz-label">{$t("Changes")}</div>
              <div class="change-diff-list">
                {#each diffRows as row}
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
    </div>
    <footer>
      <button class="kz-btn" on:click={closeAbsenceDetail}>{$t("Close")}</button>
      <span style="flex:1"></span>
      <button
        class="kz-btn kz-btn-danger"
        on:click={() => {
          const absence = absenceDetail;
          closeAbsenceDetail();
          rejectAbsence(absence);
        }}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="kz-btn kz-btn-primary"
        on:click={() => {
          const absence = absenceDetail;
          closeAbsenceDetail();
          approveAbsence(absence);
        }}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </footer>
  </dialog>
{/if}

<!-- ── Time workflow detail dialog (reopen/change) ─────────────────────────── -->
{#if requestDetail}
  <dialog bind:this={requestDetailDlg} on:close={closeRequestDetail}>
    <header>
      <span style="flex:1">
        {#if requestDetail.kind === "reopen"}
          {$t("Reopen Request Details")}
        {:else}
          {$t("Change Request Details")}
        {/if}
      </span>
      <button class="kz-btn-icon-sm kz-btn-ghost" on:click={closeRequestDetail}>
        <Icon name="X" size={16} />
      </button>
    </header>
    <div class="dialog-body">
      {#if requestDetail.kind === "reopen"}
        <div style="display:flex;flex-direction:column;gap:10px">
          <div>
            <div class="kz-label">{$t("Employee")}</div>
            <div style="font-weight:500">{userName(requestDetail.item.user_id, users)}</div>
          </div>
          <div>
            <div class="kz-label">{$t("Type")}</div>
            <div><span class="kz-chip kz-chip-pending">{$t("Reopen")}</span></div>
          </div>
          <div>
            <div class="kz-label">{$t("Week")}</div>
            <div class="tab-num">
              {fmtDateShort(requestDetail.item.week_start)} -
              {fmtDateShort(isoDate(addDays(parseDate(requestDetail.item.week_start), 6)))}
            </div>
          </div>
          <div>
            <div class="kz-label">{$t("Requested at")}</div>
            <div class="tab-num" style="font-size:12px">{fmtDateTime(requestDetail.item.created_at)}</div>
          </div>
        </div>
      {:else}
        {@const cr = requestDetail.item}
        {@const diffRows = changeDiffRows(cr)}
        <div style="display:flex;flex-direction:column;gap:10px">
          <div>
            <div class="kz-label">{$t("Employee")}</div>
            <div style="font-weight:500">{userName(cr.user_id, users)}</div>
          </div>
          <div>
            <div class="kz-label">{$t("Type")}</div>
            <div><span class="kz-chip kz-chip-warning">{changeRequestTypeLabel(cr)}</span></div>
          </div>
          {#if cr.reason}
            <div>
              <div class="kz-label">{$t("Reason")}</div>
              <div style="white-space:pre-wrap;font-size:13px">{cr.reason}</div>
            </div>
          {/if}
          <div>
            <div class="kz-label">{$t("Requested at")}</div>
            <div class="tab-num" style="font-size:12px">{fmtDateTime(cr.created_at)}</div>
          </div>
          {#if diffRows.length}
            <div>
              <div class="kz-label">{$t("Changes")}</div>
              <div class="change-diff-list">
                {#each diffRows as row}
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
        </div>
      {/if}
    </div>
    <footer>
      <button class="kz-btn" on:click={closeRequestDetail}>{$t("Close")}</button>
      <span style="flex:1"></span>
      <button
        class="kz-btn kz-btn-danger"
        on:click={() => {
          const detail = requestDetail;
          closeRequestDetail();
          if (detail.kind === "reopen") rejectReopen(detail.item.id);
          else rejectCR(detail.item.id);
        }}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="kz-btn kz-btn-primary"
        on:click={() => {
          const detail = requestDetail;
          closeRequestDetail();
          if (detail.kind === "reopen") approveReopen(detail.item.id);
          else approveCR(detail.item.id);
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
          {selectedWeek.entries.length} {$t("Days")}
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
            <div style="font-size:11.5px;color:var(--text-tertiary)">
              {$t("Type")}: {categoryName(entry.category_id)}
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
