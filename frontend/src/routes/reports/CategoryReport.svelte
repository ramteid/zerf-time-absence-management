<script>
  import { currentUser, earliestStartDate, settings, toast } from "../../stores.js";
  import { t, fmtDecimal } from "../../i18n.js";
  import { isoDate, appTodayDate, minToHM } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";
  import {
    getCategoryReport,
    getTeamCategoryReport,
  } from "../../lib/api/reportsApi.js";
  import {
    categoryColumnsFromTeamReport,
    categoryNamesFromTeamReport,
    filterCategories,
    filterTeamCategoryColumns,
    teamCategoryMinutes,
    teamCategoryRowTotal,
    totalCategoryMinutes,
  } from "../../lib/domain/reports.js";

  export let isSelfOnlyReportsView = false;

  let today = appTodayDate();
  // eslint-disable-next-line no-useless-assignment
  let todayIso = isoDate(today);
  // eslint-disable-next-line no-useless-assignment
  let currentYear = today.getFullYear();
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);
  $: currentYear = today.getFullYear();

  let catFrom = `${currentYear}-01-01`;
  let catTo = todayIso;
  let catReport = null;
  let teamCatReport = null;
  let catFilteredCategories = [];
  let catShowFilter = false;
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // Keep defaults aligned with app-timezone date changes if untouched.
  let previousCurrentYear = 0;
  let previousTodayIso = "";
  $: {
    if (!previousCurrentYear) {
      // eslint-disable-next-line no-useless-assignment
      previousCurrentYear = currentYear;
      // eslint-disable-next-line no-useless-assignment
      previousTodayIso = todayIso;
    } else {
      if (catFrom === `${previousCurrentYear}-01-01`) catFrom = `${currentYear}-01-01`;
      if (catTo === previousTodayIso) catTo = todayIso;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentYear = currentYear;
      // eslint-disable-next-line no-useless-assignment
      previousTodayIso = todayIso;
    }
  }

  // Clamp from-date to global earliest start.
  $: if ($earliestStartDate && catFrom < $earliestStartDate)
    catFrom = $earliestStartDate;

  async function showCat() {
    if (catFrom > catTo) return;
    catReport = null;
    teamCatReport = null;
    try {
      if (isSelfOnlyReportsView) {
        catReport = await getCategoryReport({
          userId: $currentUser.id,
          from: catFrom,
          to: catTo,
        });
        teamCatReport = null;
        catFilteredCategories = catReport.map((c) => c.category);
      } else {
        teamCatReport = await getTeamCategoryReport({
          from: catFrom,
          to: catTo,
        });
        catReport = null;
        catFilteredCategories = categoryNamesFromTeamReport(teamCatReport);
      }
      catShowFilter = false;
    } catch (e) {
      catReport = null;
      teamCatReport = null;
      catFilteredCategories = [];
      catShowFilter = false;
      toast($t(e?.message || "Error"), "error");
    }
  }

  function toggleCategoryFilter(categoryName) {
    catFilteredCategories = catFilteredCategories.includes(categoryName)
      ? catFilteredCategories.filter((name) => name !== categoryName)
      : [...catFilteredCategories, categoryName];
  }

  $: filteredCatReport = catReport
    ? filterCategories(catReport, catFilteredCategories)
    : catReport;
  $: filteredCatTotal = totalCategoryMinutes(filteredCatReport);

  $: allTeamCatColumns = teamCatReport
    ? categoryColumnsFromTeamReport(teamCatReport)
    : [];
  $: visibleTeamCatColumns = filterTeamCategoryColumns(
    allTeamCatColumns,
    catFilteredCategories,
  );

  function teamCatMinutes(row, category) {
    return teamCategoryMinutes(row, category);
  }
  function teamCatRowTotal(row) {
    return teamCategoryRowTotal(row, catFilteredCategories);
  }
</script>

<div class="zf-card" style="padding:20px;margin-bottom:16px">
  <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
    <span style="font-size:14px;font-weight:400">{$t("Category breakdown")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost"
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
      <label class="zf-label" for="cat-from">{$t("From")}</label>
      <DatePicker id="cat-from" bind:value={catFrom} min={$earliestStartDate} max={catTo} />
    </div>
    <div>
      <label class="zf-label" for="cat-to">{$t("To")}</label>
      <DatePicker
        id="cat-to"
        bind:value={catTo}
        min={catFrom}
        max={todayIso}
      />
    </div>
  </div>

  <div style="display:flex;gap:8px;margin-bottom:12px;flex-wrap:wrap">
    <button class="zf-btn zf-btn-primary" on:click={showCat}>{$t("Show")}</button>
    {#if (catReport && catReport.length > 0) || allTeamCatColumns.length > 0}
      <button class="zf-btn" on:click={() => (catShowFilter = !catShowFilter)}>
        {$t("Filter")}
        {#if catFilteredCategories.length > 0}
          ({catFilteredCategories.length})
        {/if}
      </button>
    {/if}
  </div>

  {#if catShowFilter && allTeamCatColumns.length > 0}
    <div
      style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px"
    >
      <div style="display:flex;flex-wrap:wrap;gap:8px">
        {#each allTeamCatColumns as col (col.category)}
          <label style="display:flex;align-items:center;gap:6px;cursor:pointer">
            <input
              type="checkbox"
              checked={catFilteredCategories.includes(col.category)}
              on:change={() => toggleCategoryFilter(col.category)}
            />
            <span class="cat-dot" style="background:{col.color || '#999'}"></span>
            <span style="font-size:13px">{$t(col.category)}</span>
          </label>
        {/each}
      </div>
    </div>
  {/if}

  {#if catShowFilter && catReport && catReport.length > 0}
    <div
      style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px"
    >
      <div style="display:flex;flex-wrap:wrap;gap:8px">
        {#each catReport as cat (cat.category)}
          <label style="display:flex;align-items:center;gap:6px;cursor:pointer">
            <input
              type="checkbox"
              checked={catFilteredCategories.includes(cat.category)}
              on:change={() => toggleCategoryFilter(cat.category)}
            />
            <span class="cat-dot" style="background:{cat.color || '#999'}"></span>
            <span style="font-size:13px">{$t(cat.category)}</span>
          </label>
        {/each}
      </div>
    </div>
  {/if}

  {#if teamCatReport}
    {#if teamCatReport.length === 0 || visibleTeamCatColumns.length === 0}
      <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
        {$t("No data.")}
      </div>
    {:else}
      <div class="zf-table-wrap" style="margin-top:12px">
        <table class="zf-table zf-table--fit">
          <thead>
            <tr>
              <th>{$t("Employee")}</th>
              {#each visibleTeamCatColumns as col (col.category)}
                <th style="text-align:right">
                  <span
                    style="display:inline-flex;align-items:center;gap:4px;justify-content:flex-end"
                  >
                    <span class="cat-dot" style="background:{col.color || '#999'}"></span>
                    {$t(col.category)}
                  </span>
                </th>
              {/each}
              <th style="text-align:right">{$t("Total")}</th>
            </tr>
          </thead>
          <tbody>
            {#each teamCatReport as row (row.user_id)}
              {@const rowTotal = teamCatRowTotal(row)}
              <tr>
                <td style="font-weight:500">{row.name}</td>
                {#each visibleTeamCatColumns as col (col.category)}
                  {@const cellMin = teamCatMinutes(row, col.category)}
                  <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                    {#if cellMin > 0}
                      {minToHM(cellMin)}
                    {:else}
                      -
                    {/if}
                  </td>
                {/each}
                <td class="tab-num" style="text-align:right;font-weight:400">
                  {rowTotal > 0 ? minToHM(rowTotal) : "-"}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}

  {#if catReport}
    {#if catReport.length === 0}
      <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
        {$t("No data.")}
      </div>
    {:else if catFilteredCategories.length === 0 || (filteredCatReport && filteredCatReport.length === 0)}
      <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
        {$t("No data.")}
      </div>
    {:else if filteredCatReport}
      <div class="zf-table-wrap" style="margin-top:12px">
        <table class="zf-table zf-table--fit" style="table-layout:fixed">
          <thead>
            <tr>
              <th>{$t("Category")}</th>
              <th style="text-align:right;width:22%">{$t("Hours")}</th>
              <th style="text-align:right;width:16%">%</th>
            </tr>
          </thead>
          <tbody>
            {#each filteredCatReport as c (c.category)}
              <tr>
                <td style="font-weight:500">
                  <span style="display:inline-flex;align-items:center;gap:6px">
                    <span class="cat-dot" style="background:{c.color || '#999'}"></span>
                    {$t(c.category)}
                  </span>
                </td>
                <td class="tab-num" style="text-align:right">{minToHM(c.minutes)}</td>
                <td class="tab-num" style="text-align:right">
                  {filteredCatTotal > 0
                    ? fmtDecimal((c.minutes / filteredCatTotal) * 100, 1)
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

<style>
  .cat-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    display: inline-block;
    flex-shrink: 0;
  }
</style>
