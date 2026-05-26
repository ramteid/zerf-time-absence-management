<script>
  import { currentUser, earliestStartDate, settings, toast } from "../../stores.js";
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
    minToHM,
    fmtDate,
  } from "../../format.js";
  import { normalizeMonthReport } from "../../apiMappers.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";
  import FlextimeChart from "../../FlextimeChart.svelte";
  import { hasFlextimeAccount, isAssistantUser } from "../../rolePolicy.js";
  import {
    getFlextimeReport,
    getLeaveBalance,
    getMonthReport,
    getOvertimeReport,
  } from "../../lib/api/reportsApi.js";
  import { monthEnd, monthStart } from "../../lib/domain/dates.js";
  import { summarizeAbsences } from "../../lib/domain/reports.js";
  import {
    findUserById,
    userWorkdaysPerWeekById,
  } from "../../lib/domain/users.js";

  export let users = [];
  export let isSelfOnlyReportsView = false;

  let today = appTodayDate();
  let todayIso = isoDate(today);
  let currentYear = today.getFullYear();
  let currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);
  $: currentYear = today.getFullYear();
  $: currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;

  let reportUserId = $currentUser.id;
  let reportMonth = currentMonthStr;
  let reportData = null;
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  $: selectedReportUser = findUserById(users, reportUserId, $currentUser);
  $: selectedUserIsAssistant = isAssistantUser(selectedReportUser);
  $: selectedUserHasFlextime = hasFlextimeAccount(selectedReportUser);
  $: reportMinMonth = selectedReportUser?.start_date
    ? selectedReportUser.start_date.slice(0, 7)
    : ($earliestStartDate?.slice(0, 7) ?? null);
  // Clamp selected month forward when switching to an employee with a later start date.
  $: if (reportMinMonth && reportMonth < reportMinMonth) {
    reportMonth = reportMinMonth;
  }
  // Force own user when in self-only mode.
  $: if (isSelfOnlyReportsView) {
    reportUserId = $currentUser.id;
  }

  // Keep untouched defaults aligned with app-timezone date changes.
  let previousCurrentMonthStr = "";
  $: {
    if (!previousCurrentMonthStr) {
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
    } else {
      if (reportMonth === previousCurrentMonthStr) reportMonth = currentMonthStr;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
    }
  }

  function userWorkdaysPerWeek(userId, fallback = 5) {
    return userWorkdaysPerWeekById(users, userId, fallback);
  }

  async function loadReport() {
    reportData = null;
    try {
      const reportYear = reportMonth.slice(0, 4);
      const reportYearNum = parseInt(reportYear);
      const isCurrentMonth = reportMonth === currentMonthStr;
      const chartMonthFrom = monthStart(reportMonth);
      const chartMonthTo = isCurrentMonth ? todayIso : monthEnd(reportMonth);
      const canFetchChart = reportYearNum < currentYear || chartMonthFrom <= todayIso;

      const [monthRaw, leaveRaw, overtimeRows, flextimeRaw] = await Promise.all([
        getMonthReport({ userId: reportUserId, month: reportMonth }),
        getLeaveBalance({ userId: reportUserId, year: reportYear }).catch(
          () => null,
        ),
        selectedUserHasFlextime
          ? getOvertimeReport({
              userId: reportUserId,
              year: reportYear,
            }).catch(() => null)
          : Promise.resolve(null),
        canFetchChart && selectedUserHasFlextime
          ? getFlextimeReport({
              userId: reportUserId,
              from: chartMonthFrom,
              to: chartMonthTo,
            }).catch(() => [])
          : Promise.resolve([]),
      ]);

      const monthReport = normalizeMonthReport(
        monthRaw,
        userWorkdaysPerWeek(reportUserId),
      );

      const flextimeBalanceRow = (overtimeRows || []).find(
        (row) => row.month === reportMonth,
      );

      reportData = {
        monthReport,
        leaveBalance: leaveRaw,
        flextimeBalance: flextimeBalanceRow?.cumulative_min ?? null,
        flextimeChartData: flextimeRaw || [],
      };
    } catch (e) {
      reportData = null;
      toast($t(e?.message || "Error"), "error");
    }
  }

  $: reportAbsenceSummary = reportData
    ? summarizeAbsences(reportData.monthReport.absences)
    : {};
</script>

<div class="zf-card" style="padding:20px;margin-bottom:16px">
  <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
    <span style="font-size:14px;font-weight:400">{$t("Employee report")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost"
      title={$t("help_employee_details")}
      on:click={() => toggleHelp("report")}
      style="color:var(--text-tertiary);font-size:14px;cursor:help"
    >
      <Icon name="Info" size={14} />
    </button>
  </div>

  {#if activeHelp === "report"}
    <div
      style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
    >
      {$t("help_employee_details")}
    </div>
  {/if}

  <div class="field-row" style="margin-bottom:12px">
    {#if !isSelfOnlyReportsView}
      <div>
        <label class="zf-label" for="report-user-id">{$t("Employee")}</label>
        <select
          id="report-user-id"
          class="zf-select"
          bind:value={reportUserId}
        >
          {#each users as u (u.id)}
            <option value={u.id}>{u.first_name} {u.last_name}</option>
          {/each}
        </select>
      </div>
    {/if}
    <div>
      <label class="zf-label" for="report-month">{$t("Month")}</label>
      <DatePicker id="report-month" mode="month" bind:value={reportMonth} min={reportMinMonth} max={currentMonthStr} />
    </div>
  </div>

  <button class="zf-btn zf-btn-primary" on:click={loadReport}>{$t("Show")}</button>

  {#if reportData}
    <div
      style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-top:20px;margin-bottom:6px"
    >
      {selectedReportUser?.id === $currentUser?.id ? $t("My Balance") : $t("Balance")}
    </div>
    <div class="stat-cards" style="margin-bottom:16px">
      <div class="zf-card stat-card">
        <div class="stat-card-label stat-card-label-help">
          <span>{$t("Logged")}</span>
          <button
            class="zf-btn-icon-sm zf-btn-ghost"
            title={$t("help_logged")}
            on:click={() => toggleHelp("logged")}
            style="color:var(--text-tertiary);font-size:12px;cursor:help"
          >
            <Icon name="Info" size={12} />
          </button>
        </div>
        <div
          class="stat-card-value tab-num"
          style="color:{selectedUserIsAssistant
            ? 'var(--text-primary)'
            : reportData.monthReport.submitted_min >=
                reportData.monthReport.full_month_target_min
              ? 'var(--accent)'
              : 'var(--warning-text)'}"
        >
          {formatHours((reportData.monthReport.submitted_min || 0) / 60)}
        </div>
        {#if !selectedUserIsAssistant}
          <div class="stat-card-sub">
            {$t("of {target} target", {
              target: formatHours(
                (reportData.monthReport.full_month_target_min || 0) / 60,
              ),
            })}
          </div>
        {/if}
      </div>

      {#if selectedUserHasFlextime}
        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Flextime balance")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{reportData.flextimeBalance === null
              ? 'var(--text-tertiary)'
              : reportData.flextimeBalance < 0
                ? 'var(--danger-text)'
                : 'var(--success-text)'}"
          >
            {#if reportData.flextimeBalance !== null}
              {reportData.flextimeBalance >= 0 ? "+" : ""}{minToHM(
                reportData.flextimeBalance,
              )}
            {:else}
              –
            {/if}
          </div>
        </div>
      {/if}

      {#if !selectedUserIsAssistant}
        <div class="zf-card stat-card">
          <div class="stat-card-label stat-card-label-help">
            <span>{$t("Submissions")}</span>
            <button
              class="zf-btn-icon-sm zf-btn-ghost"
              title={$t("help_submission_status")}
              on:click={() => toggleHelp("approvals")}
              style="color:var(--text-tertiary);font-size:12px;cursor:help"
            >
              <Icon name="Info" size={12} />
            </button>
          </div>
          <div
            class="stat-card-value tab-num"
            style="color:{reportData.monthReport.weeks_all_submitted
              ? 'var(--success-text)'
              : 'var(--warning-text)'}"
          >
            {reportData.monthReport.weeks_all_submitted
              ? $t("All submitted")
              : $t("Weeks missing")}
          </div>
          {#if reportData.monthReport.current_week_status === "draft"}
            <div
              class="stat-card-sub"
              style="color:var(--text-tertiary);font-size:11px;margin-top:4px"
            >
              {$t("Current week: draft")}
            </div>
          {:else if reportData.monthReport.current_week_status === "partial"}
            <div
              class="stat-card-sub"
              style="color:var(--text-tertiary);font-size:11px;margin-top:4px"
            >
              {$t("Current week: partially submitted")}
            </div>
          {:else if reportData.monthReport.current_week_status === "rejected"}
            <div
              class="stat-card-sub"
              style="color:var(--text-tertiary);font-size:11px;margin-top:4px"
            >
              {$t("Current week: needs revision")}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    {#if activeHelp === "logged"}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-top:-6px;margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("help_logged")}
      </div>
    {/if}
    {#if activeHelp === "approvals" && !selectedUserIsAssistant}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-top:-6px;margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("help_submission_status")}
      </div>
    {/if}

    {#if reportData.leaveBalance}
      <div
        style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-bottom:6px"
      >
        {$t("Vacation")}
      </div>
      <div class="stat-cards" style="margin-bottom:16px">
        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Entitlement")}</div>
          <div class="stat-card-value tab-num">
            {formatDayCount(reportData.leaveBalance.annual_entitlement)}
          </div>
        </div>
        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Taken")}</div>
          <div class="stat-card-value tab-num">
            {formatDayCount(reportData.leaveBalance.already_taken)}
          </div>
        </div>
        {#if reportData.leaveBalance.approved_upcoming > 0}
          <div class="zf-card stat-card">
            <div class="stat-card-label">{$t("Planned")}</div>
            <div class="stat-card-value tab-num">
              {formatDayCount(reportData.leaveBalance.approved_upcoming)}
            </div>
          </div>
        {/if}
        {#if reportData.leaveBalance.requested > 0}
          <div class="zf-card stat-card">
            <div class="stat-card-label">{$t("Requested")}</div>
            <div class="stat-card-value tab-num">
              {formatDayCount(reportData.leaveBalance.requested)}
            </div>
          </div>
        {/if}
        <div class="zf-card stat-card">
          <div class="stat-card-label">{$t("Remaining")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{reportData.leaveBalance.available < 0
              ? 'var(--danger-text)'
              : 'var(--success-text)'}"
          >
            {formatDayCount(reportData.leaveBalance.available)}
          </div>
        </div>
      </div>
    {/if}

    {#if Object.keys(reportAbsenceSummary).length > 0}
      <div
        style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-bottom:6px"
      >
        {$t("Absences")}
      </div>
      <div class="stat-cards" style="margin-bottom:16px">
        {#each Object.entries(reportAbsenceSummary) as [kind, days] (kind)}
          <div class="zf-card stat-card">
            <div class="stat-card-label">{absenceKindLabel(kind)}</div>
            <div class="stat-card-value tab-num">{formatDayCount(days)}</div>
            <div class="stat-card-sub">{$t("days")}</div>
          </div>
        {/each}
      </div>
    {/if}

    {#if reportData.monthReport.category_totals && Object.keys(reportData.monthReport.category_totals).length > 0}
      {@const catEntries = Object.entries(
        reportData.monthReport.category_totals,
      ).sort((a, b) => b[1] - a[1])}
      {@const catMax = catEntries[0][1]}
      <div class="zf-card" style="padding:16px;margin-bottom:12px">
        <div style="font-weight:400;margin-bottom:12px">
          {$t("Category breakdown")}
        </div>
        <div style="display:flex;flex-direction:column;gap:8px">
          {#each catEntries as [cat, mins] (cat)}
            <div
              style="display:grid;grid-template-columns:130px 1fr 52px;align-items:center;gap:8px;font-size:12px"
            >
              <span
                style="font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
                title={$t(cat)}
              >
                {$t(cat)}
              </span>
              <div
                style="background:var(--bg-muted);border-radius:3px;height:8px;overflow:hidden"
              >
                <div
                  style="height:100%;border-radius:3px;background:var(--accent);width:{catMax >
                  0
                    ? Math.round((mins / catMax) * 100)
                    : 0}%;transition:width .3s"
                ></div>
              </div>
              <span
                class="tab-num"
                style="color:var(--text-tertiary);text-align:right"
                >{minToHM(mins)}</span
              >
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if reportData.monthReport.entries?.length}
      <div class="zf-card" style="overflow-x:auto;margin-bottom:12px">
        <div style="font-weight:400;padding:16px 16px 12px">
          {$t("Entries")}
        </div>
        <table class="zf-table">
          <thead>
            <tr>
              <th>{$t("Date")}</th>
              <th>{$t("Start")}</th>
              <th>{$t("End")}</th>
              <th>{$t("Duration")}</th>
              <th>{$t("Category")}</th>
              <th>{$t("Status")}</th>
            </tr>
          </thead>
          <tbody>
            {#each reportData.monthReport.entries as e (e.id)}
              <tr class:entry-rejected={e.status === "rejected"}>
                <td class="tab-num">{fmtDate(e.entry_date)}</td>
                <td class="tab-num">{e.start_time?.slice(0, 5)}</td>
                <td class="tab-num">{e.end_time?.slice(0, 5)}</td>
                <td class="tab-num">{minToHM(e.minutes || 0)}</td>
                <td>{e.category_name ? $t(e.category_name) : "-"}</td>
                <td>
                  <span class="zf-chip zf-chip-{e.status}"
                    >{statusLabel(e.status)}</span
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    {#if reportData.monthReport.absences?.length}
      <div class="zf-card" style="overflow-x:auto">
        <div style="font-weight:400;padding:16px 16px 12px">
          {$t("Absences")}
        </div>
        <table class="zf-table">
          <thead>
            <tr>
              <th>{$t("Type")}</th>
              <th>{$t("From")}</th>
              <th>{$t("To")}</th>
              <th>{$t("Days")}</th>
            </tr>
          </thead>
          <tbody>
            {#each reportData.monthReport.absences as a (a.id)}
              <tr>
                <td>{absenceKindLabel(a.kind)}</td>
                <td class="tab-num">{fmtDate(a.start_date)}</td>
                <td class="tab-num">{fmtDate(a.end_date)}</td>
                <td class="tab-num">{formatDayCount(a.days)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    {#if selectedUserHasFlextime && reportData.flextimeChartData?.length}
      <div class="zf-card" style="padding:16px;margin-top:12px">
        <div style="font-weight:400;margin-bottom:12px">
          {$t("Flextime balance")}
        </div>
        <FlextimeChart data={reportData.flextimeChartData} />
      </div>
    {/if}
  {/if}
</div>
