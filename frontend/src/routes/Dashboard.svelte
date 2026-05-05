<script>
  import { tick } from "svelte";
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

  let pendingEntries = [];
  let pendingWeeks = [];
  let pendingAbsences = [];
  let changeRequests = [];
  let pendingReopens = [];
  let users = [];
  let absenceDetail = null;
  let absenceDetailDlg;

  let selectedWeek = null;
  let weekDialog;
  let weekActionBusy = false;

  let timesheetsSectionEl;
  let absencesSectionEl;
  let reopenSectionEl;
  let changesSectionEl;
  let focusedSection = "";
  let lastFocusSignature = "";

  // -- Flextime chart --------------------------------------------------------
  const today = new Date();

  function daysAgo(n) {
    return isoDate(addDays(today, -n));
  }

  let chartFrom = daysAgo(29);
  let chartTo = isoDate(today);
  let chartData = [];
  let chartLoading = false;
  let overtimeRows = [];
  let overtimeLoading = false;
  let overtimeError = "";
  let monthSubmissionChecks = [];
  let monthSubmissionLoading = false;
  let monthSubmissionError = "";

  const reportYear = today.getFullYear();
  const currentMonthIndex = today.getMonth() + 1;
  const currentMonthKey = `${reportYear}-${String(currentMonthIndex).padStart(
    2,
    "0",
  )}`;

  async function loadChart() {
    if (chartFrom > chartTo) return;
    chartLoading = true;
    try {
      chartData = await api(
        `/reports/flextime?from=${chartFrom}&to=${chartTo}`,
      );
    } catch {
      chartData = [];
    } finally {
      chartLoading = false;
    }
  }

  function monthKey(year, month) {
    return `${year}-${String(month).padStart(2, "0")}`;
  }

  function hoursFromMinutes(minutes) {
    return formatHours(((minutes || 0) / 60).toFixed(1));
  }

  function monthFullySubmitted(report) {
    if (!Array.isArray(report?.days)) return true;
    return report.days.every((day) => {
      if (!day || Number(day.target_min || 0) <= 0) return true;
      if (day.absence) return true;
      const entries = Array.isArray(day.entries) ? day.entries : [];
      if (!entries.length) return false;
      // A day counts as submitted only when it has at least one
      // submitted/approved entry and none are still in draft.
      const hasSubmittedOrApproved = entries.some(
        (entry) => entry.status === "submitted" || entry.status === "approved",
      );
      const hasDraft = entries.some((entry) => entry.status === "draft");
      return hasSubmittedOrApproved && !hasDraft;
    });
  }

  async function loadOvertimeSummary() {
    overtimeLoading = true;
    overtimeError = "";
    try {
      overtimeRows = await api(`/reports/overtime?year=${reportYear}`);
    } catch (e) {
      overtimeRows = [];
      overtimeError = e?.message || "Overtime data unavailable.";
    } finally {
      overtimeLoading = false;
    }
  }

  async function loadPastMonthSubmissionStatus() {
    const months = [];
    // Only check months from the user's start_date onward.
    const userStart = $currentUser?.start_date;
    const startYear = userStart ? parseInt(userStart.slice(0, 4), 10) : 0;
    const startMonth = userStart ? parseInt(userStart.slice(5, 7), 10) : 1;
    const firstMonth =
      reportYear === startYear ? Math.max(startMonth, 1) : reportYear > startYear ? 1 : currentMonthIndex;
    for (let month = firstMonth; month < currentMonthIndex; month += 1) {
      months.push(monthKey(reportYear, month));
    }

    if (!months.length) {
      monthSubmissionChecks = [];
      monthSubmissionError = "";
      return;
    }

    monthSubmissionLoading = true;
    monthSubmissionError = "";
    try {
      const reports = await Promise.all(
        months.map((month) => api(`/reports/month?month=${month}`)),
      );
      monthSubmissionChecks = months.map((month, index) => ({
        month,
        submitted: monthFullySubmitted(reports[index]),
      }));
    } catch (e) {
      monthSubmissionChecks = [];
      monthSubmissionError =
        e?.message || "Could not check submission status.";
    } finally {
      monthSubmissionLoading = false;
    }
  }

  function setRange(days) {
    chartFrom = daysAgo(days - 1);
    chartTo = isoDate(today);
    loadChart();
  }

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

  function userNameFromRows(uid, userRows) {
    const u = userRows.find((x) => x.id === uid);
    return u ? `${u.first_name} ${u.last_name}` : `#${uid}`;
  }

  function buildPendingWeeks(entries, userRows) {
    const grouped = new Map();

    for (const entry of entries) {
      const weekStart = weekStartOf(entry.entry_date);
      if (!weekStart) continue;
      const key = `${entry.user_id}:${weekStart}`;

      if (!grouped.has(key)) {
        grouped.set(key, {
          key,
          user_id: entry.user_id,
          week_start: weekStart,
          week_end: isoDate(addDays(parseDate(weekStart), 6)),
          entries: [],
          total_min: 0,
        });
      }

      const group = grouped.get(key);
      group.entries.push(entry);
      group.total_min += entryMinutes(entry);
    }

    const out = Array.from(grouped.values()).map((group) => ({
      ...group,
      entries: group.entries.sort((a, b) => {
        const dateCmp = dateKey(a.entry_date).localeCompare(dateKey(b.entry_date));
        if (dateCmp !== 0) return dateCmp;
        return a.start_time.localeCompare(b.start_time);
      }),
    }));

    out.sort((a, b) => {
      const weekCmp = b.week_start.localeCompare(a.week_start);
      if (weekCmp !== 0) return weekCmp;
      return userNameFromRows(a.user_id, userRows).localeCompare(
        userNameFromRows(b.user_id, userRows),
      );
    });

    return out;
  }

  async function load() {
    try {
      const [e, a, c, r, u] = await Promise.all([
        api("/time-entries/all?status=submitted"),
        api("/absences/all?status=requested"),
        api("/change-requests/all?status=open"),
        api("/reopen-requests/pending"),
        api("/users"),
      ]);
      pendingEntries = e;
      pendingAbsences = a;
      changeRequests = c;
      pendingReopens = r;
      users = u;
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  load();
  loadChart();
  loadOvertimeSummary();
  loadPastMonthSubmissionStatus();

  $: pendingWeeks = buildPendingWeeks(pendingEntries, users);
  $: currentOvertimeRow =
    overtimeRows.find((row) => row.month === currentMonthKey) || null;
  $: overtimeBalanceMin = currentOvertimeRow?.cumulative_min || 0;
  $: currentMonthDiffMin = currentOvertimeRow?.diff_min || 0;
  $: previousMonthsTotal = (() => {
    const userStart = $currentUser?.start_date;
    if (!userStart) return 0;
    const startYear = parseInt(userStart.slice(0, 4), 10);
    const startMonth = parseInt(userStart.slice(5, 7), 10);
    if (reportYear < startYear) return 0;
    const firstMonth = reportYear === startYear ? startMonth : 1;
    return Math.max(0, currentMonthIndex - firstMonth);
  })();
  $: previousMonthsSubmitted = monthSubmissionChecks.filter(
    (month) => month.submitted,
  ).length;
  $: allPreviousMonthsSubmitted =
    previousMonthsTotal === 0 ||
    (monthSubmissionChecks.length === previousMonthsTotal &&
      previousMonthsSubmitted === previousMonthsTotal);
  $: previousMonthsIncomplete = Math.max(
    0,
    previousMonthsTotal - previousMonthsSubmitted,
  );

  $: if (selectedWeek) {
    const next = pendingWeeks.find((week) => week.key === selectedWeek.key);
    if (!next) selectedWeek = null;
    else if (next !== selectedWeek) selectedWeek = next;
  }

  function userName(uid, userRows) {
    const u = userRows.find((x) => x.id === uid);
    return u ? `${u.first_name} ${u.last_name}` : `#${uid}`;
  }

  function userInitials(uid, userRows) {
    const u = userRows.find((x) => x.id === uid);
    return u
      ? ((u.first_name?.[0] || "") + (u.last_name?.[0] || "")).toUpperCase()
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
      lines.push(
        `${$t("Category")}: ${categoryName(changeRequest.new_category_id)}`,
      );
    }
    if (changeRequest.new_comment !== null && changeRequest.new_comment !== undefined) {
      lines.push(
        changeRequest.new_comment === ""
          ? `${$t("Comment")}: ${$t("Cleared")}`
          : `${$t("Comment")}: ${changeRequest.new_comment}`,
      );
    }
    return lines;
  }

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

  $: dashboardQuery = (() => {
    const q = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(q);
  })();

  $: focusTarget = dashboardQuery.get("focus") || "";
  $: focusNonce = dashboardQuery.get("n") || "";

  $: {
    const signature = focusTarget ? `${focusTarget}:${focusNonce}` : "";
    if (signature && signature !== lastFocusSignature) {
      lastFocusSignature = signature;
      revealFocusSection(focusTarget);
    }
  }

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
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
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
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    } finally {
      weekActionBusy = false;
    }
  }

  async function batchApprove() {
    const ids = pendingEntries.map((entry) => entry.id);
    if (!ids.length) return;
    const ok = await confirmDialog(
      $t("Approve all?"),
      $t("Approve all {n} submitted entries across all users?", { n: ids.length }),
      { confirm: $t("Approve all") },
    );
    if (!ok) return;
    try {
      await api("/time-entries/batch-approve", {
        method: "POST",
        body: { ids },
      });
      toast($t("All approved."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

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
    absenceDetail = null;
  }

  async function approveAbsence(id) {
    try {
      await api(`/absences/${id}/approve`, { method: "POST" });
      toast($t("Approved."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
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
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function approveReopen(id) {
    try {
      await api(`/reopen-requests/${id}/approve`, { method: "POST", body: {} });
      toast($t("Approved."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
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
      await api(`/reopen-requests/${id}/reject`, {
        method: "POST",
        body: { reason },
      });
      toast($t("Rejected."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function approveCR(id) {
    try {
      await api(`/change-requests/${id}/approve`, { method: "POST" });
      toast($t("Approved."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
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
      await api(`/change-requests/${id}/reject`, {
        method: "POST",
        body: { reason },
      });
      toast($t("Rejected."), "ok");
      load();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Dashboard")}</h1>
  </div>
  <div class="top-bar-subtitle">
    {$t("Approve timesheets & manage requests")}
  </div>
</div>

<div class="content-area">
  <div class="stat-cards">
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Pending Timesheets")}</div>
      <div class="stat-card-value accent tab-num">{pendingWeeks.length}</div>
    </div>
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Absence Requests")}</div>
      <div class="stat-card-value tab-num">{pendingAbsences.length}</div>
    </div>
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Change Requests")}</div>
      <div class="stat-card-value tab-num">{changeRequests.length}</div>
    </div>
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Team Members")}</div>
      <div class="stat-card-value tab-num">{users.length}</div>
    </div>
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
          {$t("This month: {value}", {
            value: hoursFromMinutes(currentMonthDiffMin),
          })}
        </div>
      {/if}
      {#if overtimeError}
        <div class="error-text" style="font-size:11px;margin-top:4px">
          {$t("Overtime data unavailable.")}
        </div>
      {/if}
    </div>
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
            {$t("{count} month(s) incomplete", {
              count: previousMonthsIncomplete,
            })}
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

  <div
    class="dashboard-approval-grid"
    style="display:grid;grid-template-columns:1fr 1fr;gap:16px"
  >
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
            <div
              class="tab-num"
              style="font-size:11.5px;color:var(--text-tertiary)"
            >
              {$t("Week {week}", { week: isoWeek(parseDate(week.week_start)) })} · {fmtDateShort(
                week.week_start,
              )} - {fmtDateShort(week.week_end)} · {weekHours(week)}
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
      {#each pendingAbsences as a}
        <div
          style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
        >
          <div class="avatar" style="width:30px;height:30px;font-size:11px">
            {userInitials(a.user_id, users)}
          </div>
          <div
            style="flex:1;min-width:0;cursor:pointer"
            on:click={() => showAbsenceDetail(a)}
            on:keydown={(e) => { if (e.key === "Enter") showAbsenceDetail(a); }}
            role="button"
            tabindex="0"
            title={$t("Show details")}
          >
            <div style="font-size:13px;font-weight:500">
              {userName(a.user_id, users)}
            </div>
            <div
              class="tab-num"
              style="font-size:11.5px;color:var(--text-tertiary)"
            >
              {absenceKindLabel(a.kind)} · {fmtDateShort(a.start_date)} - {fmtDateShort(
                a.end_date,
              )}
            </div>
          </div>
          <div style="display:flex;gap:4px">
            <button
              class="kz-btn-icon-sm"
              style="color:var(--success-text);background:var(--success-soft)"
              on:click={() => approveAbsence(a.id)}
            >
              <Icon name="Check" size={14} />
            </button>
            <button
              class="kz-btn-icon-sm"
              style="color:var(--danger-text);background:var(--danger-soft)"
              on:click={() => rejectAbsence(a.id)}
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
      {#each pendingReopens as r}
        <div
          style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
        >
          <div class="avatar" style="width:30px;height:30px;font-size:11px">
            {userInitials(r.user_id, users)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {userName(r.user_id, users)}
            </div>
            <div
              class="tab-num"
              style="font-size:11.5px;color:var(--text-tertiary)"
            >
              {$t("wants to edit week of {date}", {
                date: fmtDateShort(r.week_start),
              })}
            </div>
          </div>
          <div style="display:flex;gap:4px">
            <button
              class="kz-btn-icon-sm"
              style="color:var(--success-text);background:var(--success-soft)"
              title={$t("Approve")}
              on:click={() => approveReopen(r.id)}
            >
              <Icon name="Check" size={14} />
            </button>
            <button
              class="kz-btn-icon-sm"
              style="color:var(--danger-text);background:var(--danger-soft)"
              title={$t("Reject")}
              on:click={() => rejectReopen(r.id)}
            >
              <Icon name="X" size={14} />
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="kz-card" style="padding:16px 20px;margin:16px 0">
    <div
      style="display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:14px"
    >
      <Icon name="TrendingUp" size={15} sw={1.5} />
      <span style="font-size:14px;font-weight:600;flex:1"
        >{$t("Flextime balance")}</span
      >
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
        <button
          class="kz-btn kz-btn-sm"
          on:click={loadChart}
          aria-label={$t("Show")}
        >
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
                    <div style="font-size:11.5px;color:var(--text-tertiary)">
                      {change}
                    </div>
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
</div>

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
          <div class="tab-num" style="font-size:12px">{fmtDateTime(absenceDetail.created_at)}</div>
        </div>
      </div>
    </div>
    <footer>
      <button class="kz-btn" on:click={closeAbsenceDetail}>{$t("Close")}</button>
      <span style="flex:1"></span>
      <button
        class="kz-btn kz-btn-danger"
        on:click={() => { const id = absenceDetail.id; closeAbsenceDetail(); rejectAbsence(id); }}
      >
        <Icon name="X" size={14} />{$t("Reject")}
      </button>
      <button
        class="kz-btn kz-btn-primary"
        on:click={() => { const id = absenceDetail.id; closeAbsenceDetail(); approveAbsence(id); }}
      >
        <Icon name="Check" size={14} />{$t("Approve")}
      </button>
    </footer>
  </dialog>
{/if}

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
        {$t("Week {week}", {
          week: isoWeek(parseDate(selectedWeek.week_start)),
        })} · {fmtDateShort(selectedWeek.week_start)} - {fmtDateShort(
          selectedWeek.week_end,
        )}
      </div>

      <div style="display:flex;gap:8px;flex-wrap:wrap">
        <span class="kz-chip kz-chip-submitted">{selectedWeek.entries.length} {$t(
            "Entries",
          )}</span>
        <span class="kz-chip kz-chip-approved">{weekHours(selectedWeek)}</span>
      </div>

      <div class="week-entry-list">
        {#each selectedWeek.entries as entry (entry.id)}
          <div class="week-entry-row">
            <div style="font-size:12.5px;font-weight:500">
              {fmtDateShort(entry.entry_date)}
            </div>
            <div class="tab-num" style="font-size:12px;color:var(--text-secondary)">
              {entry.start_time.slice(0, 5)} - {entry.end_time.slice(0, 5)} · {formatHours(
                (entryMinutes(entry) / 60).toFixed(1),
              )}
            </div>
            {#if entry.comment}
              <div style="font-size:11.5px;color:var(--text-tertiary)">
                {entry.comment}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </div>
    <footer>
      <button class="kz-btn" on:click={closeWeekDialog} disabled={weekActionBusy}
        >{$t("Close")}</button
      >
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

  .dashboard-focus {
    box-shadow: 0 0 0 2px var(--accent);
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
