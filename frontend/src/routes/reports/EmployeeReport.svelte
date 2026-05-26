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
  import SectionCard from "../../lib/ui/SectionCard.svelte";
  import StatCard from "../../lib/ui/StatCard.svelte";
  import DataTable from "../../lib/ui/DataTable.svelte";
  import {
    hasFlextimeAccount,
    isAssistantUser,
    tracksOwnTime,
  } from "../../rolePolicy.js";
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
    hasUserId,
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

  // Pure-admin users (tracks_time=false) don't appear in `users`, so default
  // the report selection to the first available employee instead of themselves.
  let reportUserId = tracksOwnTime($currentUser) ? $currentUser.id : null;
  let reportMonth = currentMonthStr;
  let reportData = null;
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  $: if (
    (reportUserId == null || !hasUserId(users, reportUserId)) &&
    users.length > 0
  ) {
    reportUserId = users[0].id;
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
    // Guard: no user selected yet (pure-admin before users have loaded).
    // Without this, the API is called without user_id which returns 403 for
    // pure-admin users on the backend (their own report is blocked).
    if (reportUserId == null) return;
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

<SectionCard
  title={$t("Employee report")}
  helpText={$t("help_employee_details")}
  helpOpen={activeHelp === "report"}
  onHelpToggle={() => toggleHelp("report")}
>
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

  <button class="zf-btn zf-btn-primary" on:click={loadReport} disabled={reportUserId == null}>{$t("Show")}</button>

  {#if reportData}
    <div
      style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-top:20px;margin-bottom:6px"
    >
      {selectedReportUser?.id === $currentUser?.id ? $t("My Balance") : $t("Balance")}
    </div>
    <div class="stat-cards" style="margin-bottom:16px">
      <StatCard
        color={selectedUserIsAssistant
          ? "var(--text-primary)"
          : reportData.monthReport.submitted_min >=
              reportData.monthReport.full_month_target_min
            ? "var(--accent)"
            : "var(--warning-text)"}
        sub={selectedUserIsAssistant
          ? ""
          : $t("of {target} target", {
              target: formatHours(
                (reportData.monthReport.full_month_target_min || 0) / 60,
              ),
            })}
      >
        <span slot="label" class="stat-card-label-help">
          <span>{$t("Logged")}</span>
          <button
            class="zf-btn-icon-sm zf-btn-ghost"
            title={$t("help_logged")}
            on:click={() => toggleHelp("logged")}
            style="color:var(--text-tertiary);font-size:12px;cursor:help"
          >
            <Icon name="Info" size={12} />
          </button>
        </span>
        {formatHours((reportData.monthReport.submitted_min || 0) / 60)}
      </StatCard>

      {#if selectedUserHasFlextime}
        <StatCard
          label={$t("Flextime balance")}
          color={reportData.flextimeBalance === null
            ? "var(--text-tertiary)"
            : reportData.flextimeBalance < 0
              ? "var(--danger-text)"
              : "var(--success-text)"}
        >
          {#if reportData.flextimeBalance !== null}
            {reportData.flextimeBalance >= 0 ? "+" : ""}{minToHM(
              reportData.flextimeBalance,
            )}
          {:else}
            –
          {/if}
        </StatCard>
      {/if}

      {#if !selectedUserIsAssistant}
        {@const currentWeekStatus = reportData.monthReport.current_week_status}
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
              class="zf-btn-icon-sm zf-btn-ghost"
              title={$t("help_submission_status")}
              on:click={() => toggleHelp("approvals")}
              style="color:var(--text-tertiary);font-size:12px;cursor:help"
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
        <StatCard
          label={$t("Entitlement")}
          value={formatDayCount(reportData.leaveBalance.annual_entitlement)}
        />
        <StatCard
          label={$t("Taken")}
          value={formatDayCount(reportData.leaveBalance.already_taken)}
        />
        {#if reportData.leaveBalance.approved_upcoming > 0}
          <StatCard
            label={$t("Planned")}
            value={formatDayCount(reportData.leaveBalance.approved_upcoming)}
          />
        {/if}
        {#if reportData.leaveBalance.requested > 0}
          <StatCard
            label={$t("Requested")}
            value={formatDayCount(reportData.leaveBalance.requested)}
          />
        {/if}
        <StatCard
          label={$t("Remaining")}
          value={formatDayCount(reportData.leaveBalance.available)}
          color={reportData.leaveBalance.available < 0
            ? "var(--danger-text)"
            : "var(--success-text)"}
        />
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
          <StatCard
            label={absenceKindLabel(kind)}
            value={formatDayCount(days)}
            sub={$t("days")}
          />
        {/each}
      </div>
    {/if}

    {#if reportData.monthReport.category_totals && Object.keys(reportData.monthReport.category_totals).length > 0}
      {@const catEntries = Object.entries(
        reportData.monthReport.category_totals,
      ).sort((a, b) => b[1] - a[1])}
      {@const catMax = catEntries[0][1]}
      <SectionCard title={$t("Category breakdown")}>
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
      </SectionCard>
    {/if}

    {#if reportData.monthReport.entries?.length}
      <SectionCard title={$t("Entries")} padded={false}>
        <DataTable>
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
        </DataTable>
      </SectionCard>
    {/if}

    {#if reportData.monthReport.absences?.length}
      <SectionCard title={$t("Absences")} padded={false}>
        <DataTable>
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
        </DataTable>
      </SectionCard>
    {/if}

    {#if selectedUserHasFlextime && reportData.flextimeChartData?.length}
      <SectionCard title={$t("Flextime balance")}>
        <FlextimeChart data={reportData.flextimeChartData} />
      </SectionCard>
    {/if}
  {/if}
</SectionCard>
