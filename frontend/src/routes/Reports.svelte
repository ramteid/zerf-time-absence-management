<script>
  import { api } from "../api.js";
  import { currentUser } from "../stores.js";
  import { t, absenceKindLabel, language } from "../i18n.js";
  import { isoDate, minToHM, fmtDate } from "../format.js";
  import { normalizeMonthReport } from "../apiMappers.js";
  import Icon from "../Icons.svelte";

  const today = new Date();
  const monthStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}`;

  let users = [];
  let userId = $currentUser.id;
  let month = monthStr;
  let monthReport = null;

  let teamMonth = monthStr;
  let teamReport = null;

  let catFrom = isoDate(new Date(today.getFullYear(), 0, 1));
  let catTo = isoDate(today);
  let catReport = null;
  $: catTotal = (catReport || []).reduce((s, x) => s + x.minutes, 0);

  async function init() {
    users =
      $currentUser.role === "employee" ? [$currentUser] : await api("/users");
  }
  init();

  async function showMonth() {
    monthReport = normalizeMonthReport(
      await api(`/reports/month?user_id=${userId}&month=${month}`),
    );
  }
  async function showTeam() {
    teamReport = await api(`/reports/team?month=${teamMonth}`);
  }
  async function showCat() {
    if (catFrom > catTo) return;
    const params = new URLSearchParams({ from: catFrom, to: catTo });
    if ($currentUser.role === "employee")
      params.set("user_id", $currentUser.id);
    catReport = await api(`/reports/categories?${params}`);
  }
  function exportCsv() {
    window.open(`/api/v1/reports/month/csv?user_id=${userId}&month=${month}`);
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Reports")}</h1>
    <div class="top-bar-subtitle">{$t("Team hours overview")}</div>
  </div>
  <div class="top-bar-actions">
    <button class="kz-btn" on:click={exportCsv}>
      <Icon name="Download" size={14} />{$t("Export CSV")}
    </button>
  </div>
</div>

<div class="content-area">
  <!-- Monthly report -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="font-size:14px;font-weight:600;margin-bottom:14px">
      {$t("Monthly report")}
    </div>
    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="kz-label" for="reports-user-id">{$t("Employee")}</label>
        <select id="reports-user-id" class="kz-select" bind:value={userId}>
          {#each users as u}
            <option value={u.id}>{u.first_name} {u.last_name}</option>
          {/each}
        </select>
      </div>
      <div>
        <label class="kz-label" for="reports-month">{$t("Month")}</label>
        <input
          id="reports-month"
          class="kz-input"
          type="month"
          lang={$language}
          bind:value={month}
        />
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
                    ><span class="kz-chip kz-chip-{e.status}">{e.status}</span
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
      <div style="font-size:14px;font-weight:600;margin-bottom:14px">
        {$t("Team report")}
      </div>
      <div
        style="display:flex;gap:12px;align-items:flex-end;margin-bottom:12px"
      >
        <div style="flex:1">
          <label class="kz-label" for="reports-team-month">{$t("Month")}</label>
          <input
            id="reports-team-month"
            class="kz-input"
            type="month"
            lang={$language}
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
  <div class="kz-card" style="padding:20px">
    <div style="font-size:14px;font-weight:600;margin-bottom:14px">
      {$t("Category breakdown")}
    </div>
    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="kz-label" for="reports-category-from">{$t("From")}</label>
        <input
          id="reports-category-from"
          class="kz-input"
          type="date"
          lang={$language}
          bind:value={catFrom}
          max={catTo}
        />
      </div>
      <div>
        <label class="kz-label" for="reports-category-to">{$t("To")}</label>
        <input
          id="reports-category-to"
          class="kz-input"
          type="date"
          lang={$language}
          bind:value={catTo}
          min={catFrom}
        />
      </div>
    </div>
    <button class="kz-btn kz-btn-primary" on:click={showCat}>{$t("Run")}</button
    >

    {#if catReport}
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
            {#each catReport as c}
              <tr>
                <td style="font-weight:500">
                  <span style="display:inline-flex;align-items:center;gap:6px">
                    <span class="cat-dot" style="background:{c.color || '#999'}"
                    ></span>
                    {c.category}
                  </span>
                </td>
                <td class="tab-num" style="text-align:right"
                  >{minToHM(c.minutes)}</td
                >
                <td class="tab-num" style="text-align:right">
                  {catTotal > 0
                    ? ((c.minutes / catTotal) * 100).toFixed(1)
                    : 0}%
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
