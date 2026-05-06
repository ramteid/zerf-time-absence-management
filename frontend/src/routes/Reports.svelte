<script>
  import { api } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { t, absenceKindLabel, statusLabel } from "../i18n.js";
  import { isoDate, minToHM, fmtDate } from "../format.js";
  import { normalizeMonthReport } from "../apiMappers.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";
  import FlextimeChart from "../FlextimeChart.svelte";

  const today = new Date();
  const monthStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}`;

  let overtime = [];
  $: cumulative = overtime.length > 0 ? overtime[overtime.length - 1].cumulative_min : 0;

  async function loadOvertime() {
    try {
      overtime = await api(
        `/reports/overtime?year=${new Date().getFullYear()}`,
      );
    } catch (e) {
      toast($t(e?.message || "Overtime data unavailable."), "error");
    }
  }
  loadOvertime();

  let users = [];
  let userId = $currentUser.id;
  let month = monthStr;
  let monthReport = null;

  let teamMonth = monthStr;
  let teamReport = null;

  let catFrom = isoDate(new Date(today.getFullYear(), 0, 1));
  let catTo = isoDate(today);
  let catReport = null;
  let catFilteredCategories = [];
  let catShowFilter = false;

  let absenceFrom = isoDate(new Date(today.getFullYear(), 0, 1));
  let absenceTo = isoDate(today);
  let absenceReport = null;
  $: absenceTotalDays = (absenceReport || []).reduce((s, x) => s + (x.days || 0), 0);
  $: absenceByKind = (absenceReport || []).reduce((map, x) => {
    const k = x.kind || "unknown";
    map[k] = (map[k] || 0) + (x.days || 0);
    return map;
  }, {});
  $: isLeadView = $currentUser.role !== "employee";

  // CSV export state
  let csvUserId = $currentUser.id;
  let csvFrom = isoDate(new Date(today.getFullYear(), today.getMonth(), 1));
  let csvTo = isoDate(today);
  let csvError = "";

  // Employee detail report state
  let detailUserId = $currentUser.id;
  let detailMonth = monthStr;
  let detailReport = null;
  let detailShowDialog = false;
  let detailOvertimeBalance = 0;
  let detailFlextimeData = [];

  // Help tooltip state
  let activeHelp = null;
  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  async function init() {
    users =
      $currentUser.role === "employee" ? [$currentUser] : await api("/users");
  }
  init();

  async function showMonth() {
    try {
      monthReport = normalizeMonthReport(
        await api(`/reports/month?user_id=${userId}&month=${month}`),
      );
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  async function showTeam() {
    try {
      teamReport = await api(`/reports/team?month=${teamMonth}`);
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  async function showCat() {
    if (catFrom > catTo) return;
    try {
      const params = new URLSearchParams({ from: catFrom, to: catTo });
      if ($currentUser.role === "employee")
        params.set("user_id", $currentUser.id);
      catReport = await api(`/reports/categories?${params}`);
      catFilteredCategories = [];
      catShowFilter = false;
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  function absenceDays(a) {
    if (!a.start_date || !a.end_date) return 0;
    const start = new Date(a.start_date);
    const end = new Date(a.end_date);
    return Math.max(1, Math.round((end - start) / 86400000) + 1);
  }
  async function showAbsences() {
    if (absenceFrom > absenceTo) return;
    try {
      let raw;
      if ($currentUser.role === "employee") {
        const fromYear = new Date(absenceFrom).getFullYear();
        raw = await api(`/absences?year=${fromYear}`);
        raw = raw.filter(a => a.end_date >= absenceFrom && a.start_date <= absenceTo);
      } else {
        const params = new URLSearchParams({ from: absenceFrom, to: absenceTo });
        raw = await api(`/absences/all?${params}`);
      }
      absenceReport = raw.map(a => ({ ...a, days: absenceDays(a) }));
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  function toggleCategoryFilter(categoryName) {
    const idx = catFilteredCategories.indexOf(categoryName);
    if (idx >= 0) {
      catFilteredCategories = catFilteredCategories.filter(c => c !== categoryName);
    } else {
      catFilteredCategories = [...catFilteredCategories, categoryName];
    }
  }
  $: filteredCatReport = catFilteredCategories.length === 0
    ? catReport
    : (catReport || []).filter(c => catFilteredCategories.includes(c.category));
  $: filteredCatTotal = (filteredCatReport || []).reduce((s, x) => s + x.minutes, 0);
  async function showDetail() {
    try {
      const year = detailMonth.slice(0, 4);
      const monthNum = parseInt(detailMonth.slice(5, 7), 10);
      const lastDay = new Date(parseInt(year, 10), monthNum, 0).getDate();
      const toDate = `${detailMonth}-${String(lastDay).padStart(2, "0")}`;
      const [monthRaw, overtimeRows, flextimeRaw] = await Promise.all([
        api(`/reports/month?user_id=${detailUserId}&month=${detailMonth}`),
        api(`/reports/overtime?user_id=${detailUserId}&year=${year}`).catch(() => []),
        api(`/reports/flextime?user_id=${detailUserId}&from=${detailMonth}-01&to=${toDate}`).catch(() => []),
      ]);
      detailReport = normalizeMonthReport(monthRaw);
      const lastRow = overtimeRows.length > 0 ? overtimeRows[overtimeRows.length - 1] : null;
      detailOvertimeBalance = lastRow?.cumulative_min || 0;
      detailFlextimeData = flextimeRaw;
      detailShowDialog = true;
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  function closeDetail() {
    detailShowDialog = false;
    detailReport = null;
    detailFlextimeData = [];
    detailOvertimeBalance = 0;
  }
  function csvSafe(s) {
    if (s && /^[=+\-@\t\r]/.test(s)) return "'" + s;
    return s;
  }
  function csvEncode(fields) {
    return fields
      .map((f) => {
        const s = f == null ? "" : String(f);
        return s.includes(",") || s.includes('"') || s.includes("\n")
          ? '"' + s.replace(/"/g, '""') + '"'
          : s;
      })
      .join(",");
  }
  async function exportCsv() {
    csvError = "";
    if (!csvFrom || !csvTo) {
      csvError = $t("Invalid date.");
      return;
    }
    if (csvFrom > csvTo) {
      csvError = $t("From cannot be after To.");
      return;
    }
    try {
      const params = new URLSearchParams({
        user_id: String(csvUserId),
        from: csvFrom,
        to: csvTo,
      });
      const report = await api(`/reports/range?${params}`);
      const header = csvEncode([
        $t("Date"), $t("Weekday"), $t("Start"), $t("End"),
        $t("Category"), $t("Minutes"), $t("Status"), $t("Comment"),
        $t("Absence"), $t("Holiday"),
      ]);
      const rows = [header];
      for (const day of report.days) {
        const weekday = $t(day.weekday);
        const absence = day.absence ? absenceKindLabel(day.absence) : "";
        const holiday = day.holiday || "";
        if (!day.entries || day.entries.length === 0) {
          rows.push(csvEncode([
            day.date, weekday, "", "", "", "0", "", "",
            csvSafe(absence), csvSafe(holiday),
          ]));
        } else {
          for (const e of day.entries) {
            rows.push(csvEncode([
              day.date, weekday, e.start_time, e.end_time,
              csvSafe($t(e.category)), e.minutes, statusLabel(e.status),
              csvSafe(e.comment || ""), csvSafe(absence), csvSafe(holiday),
            ]));
          }
        }
      }
      rows.push(csvEncode([
        "", $t("Total"), "", "", "", report.actual_min, "", "", "", "",
      ]));
      const blob = new Blob(["\uFEFF" + rows.join("\n")], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `report-${csvUserId}-${csvFrom}_to_${csvTo}.csv`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      csvError = $t(e?.message || "Export failed.");
    }
  }
</script>

<svelte:window on:keydown={e => { if (e.key === "Escape" && detailShowDialog) closeDetail(); }} />

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Reports")}</h1>
  </div>
  <div class="top-bar-subtitle">
    {#if $currentUser?.permissions?.can_view_team_reports}
      {$t("Team hours overview")}
    {:else}
      {$t("Your hours overview")}
    {/if}
  </div>
</div>

<div class="content-area">
  <!-- Employee detail report -->
  {#if $currentUser.permissions?.can_approve}
    <div class="kz-card" style="padding:20px;margin-bottom:16px">
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
        <span style="font-size:14px;font-weight:600">{$t("Employee Details")}</span>
        <button
          class="kz-btn-icon-sm kz-btn-ghost"
          title={$t("help_employee_details")}
          on:click={() => toggleHelp("detail")}
          style="color:var(--text-tertiary);font-size:14px;cursor:help"
        >
          <Icon name="Info" size={14} />
        </button>
      </div>
      {#if activeHelp === "detail"}
        <div
          style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
        >
          {$t("View detailed information about a team member including balance and statistics.")}
        </div>
      {/if}
      <div class="field-row" style="margin-bottom:12px">
        <div>
          <label class="kz-label" for="reports-detail-user-id">{$t("Employee")}</label>
          <select id="reports-detail-user-id" class="kz-select" bind:value={detailUserId}>
            {#each users as u}
              <option value={u.id}>{u.first_name} {u.last_name}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="kz-label" for="reports-detail-month">{$t("Month")}</label>
          <DatePicker id="reports-detail-month" mode="month" bind:value={detailMonth} />
        </div>
      </div>
      <button class="kz-btn kz-btn-primary" on:click={showDetail}
        >{$t("Show")}</button
      >
    </div>
  {/if}

  <!-- Overtime balance -->
  <div class="kz-card overtime-card" style="margin-bottom:16px">
    <div class="card-header">
      <span class="card-header-title">
        {$t("Overtime balance {year}", { year: new Date().getFullYear() })}
      </span>
      <span
        class="kz-chip"
        class:kz-chip-approved={cumulative >= 0}
        class:kz-chip-rejected={cumulative < 0}
      >
        {minToHM(cumulative)}
      </span>
    </div>

    <!-- Desktop: table -->
    <div class="overtime-table-desktop">
      <table class="kz-table">
        <thead>
          <tr>
            {#each ["Month", "Target", "Actual", "Diff", "Cumulative"] as c}
              <th>{$t(c)}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each overtime as m, i}
            {@const cum = m.cumulative_min}
            <tr>
              <td class="tab-num">{m.month}</td>
              <td class="tab-num">{minToHM(m.target_min)}</td>
              <td class="tab-num">{minToHM(m.actual_min)}</td>
              <td
                class="tab-num"
                style="color:{m.diff_min < 0
                  ? 'var(--danger-text)'
                  : 'var(--success-text)'}"
              >
                {minToHM(m.diff_min)}
              </td>
              <td
                class="tab-num"
                style="color:{cum < 0
                  ? 'var(--danger-text)'
                  : 'var(--success-text)'}"
              >
                {minToHM(cum)}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <!-- Mobile: stacked tiles -->
    <div class="overtime-tiles-mobile">
      {#each overtime as m, i}
        {@const cum = m.cumulative_min}
        <div class="overtime-tile">
          <div style="font-weight:600;font-size:13px;margin-bottom:4px">
            {m.month}
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Target")}</span><span class="tab-num"
              >{minToHM(m.target_min)}</span
            >
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Actual")}</span><span class="tab-num"
              >{minToHM(m.actual_min)}</span
            >
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Diff")}</span>
            <span
              class="tab-num"
              style="color:{m.diff_min < 0
                ? 'var(--danger-text)'
                : 'var(--success-text)'}"
            >
              {minToHM(m.diff_min)}
            </span>
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Cumulative")}</span>
            <span
              class="tab-num"
              style="color:{cum < 0
                ? 'var(--danger-text)'
                : 'var(--success-text)'}"
            >
              {minToHM(cum)}
            </span>
          </div>
        </div>
      {/each}
    </div>
  </div>

  <!-- Monthly report -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:600">{$t("Monthly report")}</span>
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        title={$t("help_monthly_report")}
        on:click={() => toggleHelp("monthly")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
    </div>
    {#if activeHelp === "monthly"}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("help_monthly_report")}
      </div>
    {/if}
    <div class="field-row" style="margin-bottom:12px">
      {#if $currentUser.role !== "employee"}
        <div>
          <label class="kz-label" for="reports-user-id">{$t("Employee")}</label>
          <select id="reports-user-id" class="kz-select" bind:value={userId}>
            {#each users as u}
              <option value={u.id}>{u.first_name} {u.last_name}</option>
            {/each}
          </select>
        </div>
      {/if}
      <div>
        <label class="kz-label" for="reports-month">{$t("Month")}</label>
        <DatePicker id="reports-month" mode="month" bind:value={month} />
      </div>
    </div>
    <button class="kz-btn kz-btn-primary" on:click={showMonth}
      >{$t("Show")}</button
    >

    {#if monthReport}
      <div class="stat-cards" style="margin-top:16px">
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Target")}</div>
          <div class="stat-card-value tab-num">
            {minToHM(monthReport.target_min)}
          </div>
        </div>
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Actual")}</div>
          <div class="stat-card-value tab-num">
            {minToHM(monthReport.actual_min)}
          </div>
        </div>
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Diff")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{monthReport.diff_min < 0
              ? 'var(--danger-text)'
              : 'var(--success-text)'}"
          >
            {minToHM(monthReport.diff_min)}
          </div>
        </div>
      </div>

      {#if monthReport.entries?.length}
        <div class="kz-card" style="overflow-x:auto;margin-top:12px">
          <table class="kz-table">
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
              {#each monthReport.entries as e}
                <tr>
                  <td class="tab-num">{fmtDate(e.entry_date)}</td>
                  <td class="tab-num">{e.start_time?.slice(0, 5)}</td>
                  <td class="tab-num">{e.end_time?.slice(0, 5)}</td>
                  <td class="tab-num">{minToHM(e.minutes || 0)}</td>
                  <td>{e.category_name ? $t(e.category_name) : "–"}</td>
                  <td
                    ><span class="kz-chip kz-chip-{e.status}"
                      >{statusLabel(e.status)}</span
                    ></td
                  >
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

      {#if monthReport.absences?.length}
        <div class="kz-card" style="overflow-x:auto;margin-top:12px">
          <div class="card-header">
            <span class="card-header-title">{$t("Absences")}</span>
          </div>
          <table class="kz-table">
            <thead>
              <tr>
                <th>{$t("Type")}</th>
                <th>{$t("From")}</th>
                <th>{$t("To")}</th>
                <th>{$t("Days")}</th>
              </tr>
            </thead>
            <tbody>
              {#each monthReport.absences as a}
                <tr>
                  <td>{absenceKindLabel(a.kind)}</td>
                  <td class="tab-num">{fmtDate(a.start_date)}</td>
                  <td class="tab-num">{fmtDate(a.end_date)}</td>
                  <td class="tab-num">{a.days}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Team report -->
  {#if $currentUser.permissions?.can_view_team_reports}
    <div class="kz-card" style="padding:20px;margin-bottom:16px">
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
        <span style="font-size:14px;font-weight:600">{$t("Team report")}</span>
        <button
          class="kz-btn-icon-sm kz-btn-ghost"
          title={$t("help_team_report")}
          on:click={() => toggleHelp("team")}
          style="color:var(--text-tertiary);font-size:14px;cursor:help"
        >
          <Icon name="Info" size={14} />
        </button>
      </div>
      {#if activeHelp === "team"}
        <div
          style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
        >
          {$t("help_team_report")}
        </div>
      {/if}
      <div
        style="display:flex;gap:12px;align-items:flex-end;margin-bottom:12px;flex-wrap:wrap"
      >
        <div style="flex:1">
          <label class="kz-label" for="reports-team-month">{$t("Month")}</label>
          <DatePicker
            id="reports-team-month"
            mode="month"
            bind:value={teamMonth}
          />
        </div>
        <button class="kz-btn kz-btn-primary" on:click={showTeam}
          >{$t("Show")}</button
        >
      </div>

      {#if teamReport}
        <div class="kz-card" style="overflow-x:auto">
          <table class="kz-table">
            <thead>
              <tr>
                <th>{$t("Employee")}</th>
                <th style="text-align:right">{$t("Target")}</th>
                <th style="text-align:right">{$t("Actual")}</th>
                <th style="text-align:right">{$t("Diff")}</th>
              </tr>
            </thead>
            <tbody>
              {#each teamReport as r}
                {@const diff = r.actual_min - r.target_min}
                <tr>
                  <td style="font-weight:500">{r.name}</td>
                  <td
                    class="tab-num"
                    style="text-align:right;color:var(--text-tertiary)"
                    >{minToHM(r.target_min)}</td
                  >
                  <td class="tab-num" style="text-align:right;font-weight:600"
                    >{minToHM(r.actual_min)}</td
                  >
                  <td
                    class="tab-num"
                    style="text-align:right;font-weight:500;color:{diff > 0
                      ? 'var(--warning-text)'
                      : diff < 0
                        ? 'var(--danger-text)'
                        : 'var(--success-text)'}"
                  >
                    {diff > 0 ? "+" : ""}{minToHM(diff)}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Category report -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:600"
        >{$t("Category breakdown")}</span
      >
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        title={$t("help_category_breakdown")}
        on:click={() => toggleHelp("cat")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
    </div>
    {#if activeHelp === "cat"}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("help_category_breakdown")}
      </div>
    {/if}
    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="kz-label" for="reports-category-from">{$t("From")}</label>
        <DatePicker
          id="reports-category-from"
          bind:value={catFrom}
          max={catTo}
        />
      </div>
      <div>
        <label class="kz-label" for="reports-category-to">{$t("To")}</label>
        <DatePicker id="reports-category-to" bind:value={catTo} min={catFrom} />
      </div>
    </div>
    <div style="display:flex;gap:8px;margin-bottom:12px">
      <button class="kz-btn kz-btn-primary" on:click={showCat}>{$t("Run")}</button>
      {#if catReport && catReport.length > 0}
        <button
          class="kz-btn"
          on:click={() => catShowFilter = !catShowFilter}
        >
          {$t("Filter")} ({catFilteredCategories.length})
        </button>
      {/if}
    </div>

    {#if catReport && catShowFilter && catReport.length > 0}
      <div style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px">
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          {#each catReport as cat}
            <label style="display:flex;align-items:center;gap:6px;cursor:pointer">
              <input
                type="checkbox"
                checked={catFilteredCategories.includes(cat.category)}
                on:change={() => toggleCategoryFilter(cat.category)}
              />
              <span
                class="cat-dot"
                style="background:{cat.color || '#999'}"
              ></span>
              <span style="font-size:13px">{$t(cat.category)}</span>
            </label>
          {/each}
        </div>
      </div>
    {/if}

    {#if catReport}
      {#if catReport.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
          {$t("No data.")}
        </div>
      {:else if filteredCatReport.length === 0 && catFilteredCategories.length > 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
          {$t("No data.")}
        </div>
      {:else}
        <div class="kz-card" style="overflow-x:auto;margin-top:12px">
          <table class="kz-table">
            <thead>
              <tr>
                <th>{$t("Category")}</th>
                <th style="text-align:right">{$t("Hours")}</th>
                <th style="text-align:right">%</th>
              </tr>
            </thead>
            <tbody>
              {#each filteredCatReport as c}
                <tr>
                  <td style="font-weight:500">
                    <span
                      style="display:inline-flex;align-items:center;gap:6px"
                    >
                      <span
                        class="cat-dot"
                        style="background:{c.color || '#999'}"
                      ></span>
                      {$t(c.category)}
                    </span>
                  </td>
                  <td class="tab-num" style="text-align:right"
                    >{minToHM(c.minutes)}</td
                  >
                  <td class="tab-num" style="text-align:right">
                    {filteredCatTotal > 0
                      ? ((c.minutes / filteredCatTotal) * 100).toFixed(1)
                      : 0}%
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Absence report -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:600">{$t("Absences")}</span>
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        title={$t("help_absence_report")}
        on:click={() => toggleHelp("absence")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
    </div>
    {#if activeHelp === "absence"}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("View absence entries over a selected period with type distribution.")}
      </div>
    {/if}
    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="kz-label" for="reports-absence-from">{$t("From")}</label>
        <DatePicker
          id="reports-absence-from"
          bind:value={absenceFrom}
          max={absenceTo}
        />
      </div>
      <div>
        <label class="kz-label" for="reports-absence-to">{$t("To")}</label>
        <DatePicker id="reports-absence-to" bind:value={absenceTo} min={absenceFrom} />
      </div>
    </div>
    <button class="kz-btn kz-btn-primary" on:click={showAbsences}>{$t("Run")}</button
    >

    {#if absenceReport}
      {#if absenceReport.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
          {$t("No data.")}
        </div>
      {:else}
        <div class="stat-cards" style="margin-top:16px">
          <div class="kz-card stat-card">
            <div class="stat-card-label">{$t("Total days")}</div>
            <div class="stat-card-value tab-num">
              {absenceTotalDays}
            </div>
          </div>
          {#each Object.entries(absenceByKind) as [kind, days]}
            <div class="kz-card stat-card">
              <div class="stat-card-label">{absenceKindLabel(kind)}</div>
              <div class="stat-card-value tab-num">{days}</div>
            </div>
          {/each}
        </div>

        <div class="kz-card" style="overflow-x:auto;margin-top:12px">
          <table class="kz-table">
            <thead>
              <tr>
                {#if isLeadView}<th>{$t("Employee")}</th>{/if}
                <th>{$t("Type")}</th>
                <th style="text-align:right">{$t("From")}</th>
                <th style="text-align:right">{$t("To")}</th>
                <th style="text-align:right">{$t("Days")}</th>
                <th>{$t("Status")}</th>
              </tr>
            </thead>
            <tbody>
              {#each absenceReport as a}
                {@const absUser = isLeadView ? users.find(u => u.id === a.user_id) : null}
                <tr>
                  {#if isLeadView}
                    <td style="font-weight:500">{absUser ? `${absUser.first_name} ${absUser.last_name}` : `#${a.user_id}`}</td>
                  {/if}
                  <td>{absenceKindLabel(a.kind)}</td>
                  <td class="tab-num" style="text-align:right">{fmtDate(a.start_date)}</td>
                  <td class="tab-num" style="text-align:right">{fmtDate(a.end_date)}</td>
                  <td class="tab-num" style="text-align:right">{a.days}</td>
                  <td><span class="kz-chip kz-chip-{a.status}">{statusLabel(a.status)}</span></td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>

  <!-- CSV Export tile (moved to bottom) -->
  <div class="kz-card" style="padding:20px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:600">{$t("Export CSV")}</span>
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        title={$t("help_csv_export")}
        on:click={() => toggleHelp("csv")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
    </div>
    {#if activeHelp === "csv"}
      <div
        style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
      >
        {$t("help_csv_export")}
      </div>
    {/if}
    <div class="field-row" style="margin-bottom:12px">
      {#if $currentUser.role !== "employee"}
        <div>
          <label class="kz-label" for="csv-user-id">{$t("Employee")}</label>
          <select id="csv-user-id" class="kz-select" bind:value={csvUserId}>
            {#each users as u}
              <option value={u.id}>{u.first_name} {u.last_name}</option>
            {/each}
          </select>
        </div>
      {/if}
      <div>
        <label class="kz-label" for="csv-from">{$t("From")}</label>
        <DatePicker id="csv-from" bind:value={csvFrom} max={csvTo} />
      </div>
      <div>
        <label class="kz-label" for="csv-to">{$t("To")}</label>
        <DatePicker id="csv-to" bind:value={csvTo} min={csvFrom} />
      </div>
    </div>
    <div class="error-text">{csvError}</div>
    <button class="kz-btn kz-btn-primary" on:click={exportCsv}>
      <Icon name="Download" size={14} />{$t("Export CSV")}
    </button>
  </div>
</div>

{#if detailShowDialog && detailReport}
  {@const detailUser = users.find(u => u.id === detailUserId)}
  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
  <div class="dialog-backdrop" on:click={closeDetail}></div>
  <dialog open style="position:fixed;top:50%;left:50%;transform:translate(-50%,-50%);width:90%;max-width:900px;max-height:90vh;border:none;border-radius:var(--radius);box-shadow:var(--shadow-lg);padding:0;z-index:1001">
    <header style="padding:16px 20px;border-bottom:1px solid var(--border);display:flex;align-items:center;justify-content:space-between">
      <span style="font-size:16px;font-weight:600">
        {$t("Employee Details")} · {detailUser ? `${detailUser.first_name} ${detailUser.last_name}` : `#${detailUserId}`}
      </span>
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        on:click={closeDetail}
        aria-label={$t("Close")}
      >
        <Icon name="X" size={16} />
      </button>
    </header>

    <div style="overflow-y:auto;padding:20px;display:flex;flex-direction:column;gap:16px">
      <!-- Balances -->
      <div class="stat-cards">
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Overtime balance")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{detailOvertimeBalance < 0
              ? 'var(--danger-text)'
              : 'var(--success-text)'}"
          >
            {minToHM(detailOvertimeBalance)}
          </div>
        </div>
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Target")}</div>
          <div class="stat-card-value tab-num">
            {minToHM(detailReport.target_min)}
          </div>
        </div>
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Actual")}</div>
          <div class="stat-card-value tab-num">
            {minToHM(detailReport.actual_min)}
          </div>
        </div>
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Diff")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{detailReport.diff_min < 0
              ? 'var(--danger-text)'
              : 'var(--success-text)'}"
          >
            {minToHM(detailReport.diff_min)}
          </div>
        </div>
      </div>

      <!-- Flextime Chart -->
      {#if detailFlextimeData.length > 0}
        <div class="kz-card" style="padding:16px">
          <div style="font-weight:600;margin-bottom:8px">{$t("Flextime")}</div>
          <FlextimeChart data={detailFlextimeData} />
        </div>
      {/if}

      <!-- Category breakdown -->
      {#if detailReport.category_totals && Object.keys(detailReport.category_totals).length > 0}
        <div class="kz-card" style="padding:16px">
          <div style="font-weight:600;margin-bottom:8px">{$t("Category breakdown")}</div>
          <div style="display:flex;flex-wrap:wrap;gap:12px">
            {#each Object.entries(detailReport.category_totals) as [cat, mins]}
              <div style="display:flex;align-items:center;gap:6px;font-size:13px">
                <span style="font-weight:500">{$t(cat)}</span>
                <span class="tab-num" style="color:var(--text-tertiary)">{minToHM(mins)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Entries -->
      {#if detailReport.entries?.length}
        <div class="kz-card" style="overflow-x:auto">
          <div style="font-weight:600;margin-bottom:12px;padding:0 16px;padding-top:16px">{$t("Entries")}</div>
          <table class="kz-table">
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
              {#each detailReport.entries as e}
                <tr>
                  <td class="tab-num">{fmtDate(e.entry_date)}</td>
                  <td class="tab-num">{e.start_time?.slice(0, 5)}</td>
                  <td class="tab-num">{e.end_time?.slice(0, 5)}</td>
                  <td class="tab-num">{minToHM(e.minutes || 0)}</td>
                  <td>{e.category_name ? $t(e.category_name) : "–"}</td>
                  <td
                    ><span class="kz-chip kz-chip-{e.status}"
                      >{statusLabel(e.status)}</span
                    ></td
                  >
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

      <!-- Absences -->
      {#if detailReport.absences?.length}
        <div class="kz-card" style="overflow-x:auto">
          <div style="font-weight:600;margin-bottom:12px;padding:0 16px;padding-top:16px">{$t("Absences")}</div>
          <table class="kz-table">
            <thead>
              <tr>
                <th>{$t("Type")}</th>
                <th>{$t("From")}</th>
                <th>{$t("To")}</th>
                <th>{$t("Days")}</th>
              </tr>
            </thead>
            <tbody>
              {#each detailReport.absences as a}
                <tr>
                  <td>{absenceKindLabel(a.kind)}</td>
                  <td class="tab-num">{fmtDate(a.start_date)}</td>
                  <td class="tab-num">{fmtDate(a.end_date)}</td>
                  <td class="tab-num">{a.days}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>

    <footer style="padding:16px 20px;border-top:1px solid var(--border);display:flex;justify-content:flex-end">
      <button class="kz-btn" on:click={closeDetail}>{$t("Close")}</button>
    </footer>
  </dialog>
{/if}

<style>
  .overtime-table-desktop {
    overflow-x: auto;
  }
  .overtime-tiles-mobile {
    display: none;
  }
  .overtime-tile {
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  .overtime-tile:last-child {
    border-bottom: none;
  }
  .overtime-tile-row {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    padding: 2px 0;
  }

  .dialog-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    z-index: 1000;
  }

  .cat-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    display: inline-block;
  }

  @media (max-width: 640px) {
    .overtime-table-desktop {
      display: none;
    }
    .overtime-tiles-mobile {
      display: block;
    }
  }
</style>
