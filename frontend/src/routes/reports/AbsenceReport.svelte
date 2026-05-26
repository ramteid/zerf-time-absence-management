<script>
  import { earliestStartDate, settings, toast } from "../../stores.js";
  import {
    t,
    absenceKindLabel,
    statusLabel,
    formatDayCount,
  } from "../../i18n.js";
  import { appTodayDate, fmtDate } from "../../format.js";
  import { countWorkdays, holidayDateSet } from "../../apiMappers.js";
  import DatePicker from "../../DatePicker.svelte";
  import SectionCard from "../../lib/ui/SectionCard.svelte";
  import StatCard from "../../lib/ui/StatCard.svelte";
  import DataTable from "../../lib/ui/DataTable.svelte";
  import {
    getAbsenceReport,
    getHolidaysByYear,
    getUserAbsencesByYear,
  } from "../../lib/api/reportsApi.js";
  import { isoMonthStart, yearsBetweenDates } from "../../lib/domain/dates.js";
  import {
    absenceKindTotals,
    dedupeAbsences,
    totalAbsenceDays,
  } from "../../lib/domain/reports.js";
  import { userWorkdaysPerWeekById } from "../../lib/domain/users.js";

  export let users = [];
  export let isSelfOnlyReportsView = false;

  let today = appTodayDate();
  // eslint-disable-next-line no-useless-assignment
  let currentYear = today.getFullYear();
  $: today = appTodayDate($settings?.timezone);
  $: currentYear = today.getFullYear();

  let absenceFrom = isoMonthStart(today);
  let absenceTo = `${currentYear}-12-31`;
  let absenceReport = null;
  let absenceHolidayDates = new Set();
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  $: absenceTotalDays = totalAbsenceDays(absenceReport);
  $: absenceByKind = absenceKindTotals(absenceReport);
  $: isLeadView = !isSelfOnlyReportsView;

  // Keep defaults aligned with app-timezone date changes if untouched.
  let previousCurrentMonthStr = "";
  let previousCurrentYear = 0;
  $: currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: {
    if (!previousCurrentMonthStr) {
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentYear = currentYear;
    } else {
      if (absenceFrom === `${previousCurrentMonthStr}-01`)
        absenceFrom = `${currentMonthStr}-01`;
      if (absenceTo === `${previousCurrentYear}-12-31`)
        absenceTo = `${currentYear}-12-31`;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentYear = currentYear;
    }
  }

  $: if ($earliestStartDate && absenceFrom < $earliestStartDate)
    absenceFrom = $earliestStartDate;

  function clampAbsenceRange(absence) {
    if (!absence?.start_date || !absence?.end_date) return null;
    const from =
      absence.start_date > absenceFrom ? absence.start_date : absenceFrom;
    const rangeEnd =
      absence.end_date < absenceTo ? absence.end_date : absenceTo;
    if (rangeEnd < from) return null;
    return { from, to: rangeEnd };
  }

  function absenceDays(absence) {
    const clamped = clampAbsenceRange(absence);
    if (!clamped) return 0;
    const workdaysPerWeek = userWorkdaysPerWeekById(users, absence?.user_id, 5);
    return countWorkdays(
      clamped.from,
      clamped.to,
      absenceHolidayDates,
      workdaysPerWeek,
    );
  }

  async function loadOwnAbsencesForRange() {
    const years = yearsBetweenDates(absenceFrom, absenceTo);
    const absenceLists = await Promise.all(
      years.map((yearValue) => getUserAbsencesByYear(yearValue)),
    );
    return absenceLists.flat().filter(
      (a) => a.end_date >= absenceFrom && a.start_date <= absenceTo,
    );
  }

  async function showAbsences() {
    if (absenceFrom > absenceTo) return;
    absenceReport = null;
    try {
      let raw;
      if (isSelfOnlyReportsView) {
        raw = dedupeAbsences(await loadOwnAbsencesForRange());
      } else {
        const [teamAbsences, ownAbsences] = await Promise.all([
          getAbsenceReport({ from: absenceFrom, to: absenceTo }),
          loadOwnAbsencesForRange(),
        ]);
        raw = dedupeAbsences([...teamAbsences, ...ownAbsences]);
      }
      raw = raw.filter(
        (a) => a.status !== "rejected" && a.status !== "cancelled",
      );
      const allYears = [
        ...new Set(
          raw.flatMap((a) => [
            parseInt(a.start_date.slice(0, 4), 10),
            parseInt(a.end_date.slice(0, 4), 10),
          ]),
        ),
      ];
      const holidayLists = await Promise.all(
        allYears.map((y) => getHolidaysByYear(y)),
      );
      absenceHolidayDates = holidayDateSet(holidayLists.flat());
      absenceReport = raw.map((a) => ({ ...a, days: absenceDays(a) }));
    } catch (e) {
      absenceReport = null;
      absenceHolidayDates = new Set();
      toast($t(e?.message || "Error"), "error");
    }
  }

</script>

<SectionCard
  title={$t("Absences")}
  helpText={$t("help_absence_report")}
  helpOpen={activeHelp === "absence"}
  onHelpToggle={() => toggleHelp("absence")}
>
  <div class="field-row" style="margin-bottom:12px">
    <div>
      <label class="zf-label" for="absence-from">{$t("From")}</label>
      <DatePicker
        id="absence-from"
        bind:value={absenceFrom}
        min={$earliestStartDate}
        max={absenceTo}
      />
    </div>
    <div>
      <label class="zf-label" for="absence-to">{$t("To")}</label>
      <DatePicker id="absence-to" bind:value={absenceTo} min={absenceFrom} />
    </div>
  </div>
  <button class="zf-btn zf-btn-primary" on:click={showAbsences}>{$t("Show")}</button>

  {#if absenceReport}
    {#if absenceReport.length === 0}
      <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
        {$t("No data.")}
      </div>
    {:else}
      {#if absenceTotalDays > 0}
        <div class="stat-cards" style="margin-top:16px">
          <StatCard
            label={$t("Total days")}
            value={formatDayCount(absenceTotalDays)}
          />
          {#each Object.entries(absenceByKind) as [kind, days] (kind)}
            <StatCard
              label={absenceKindLabel(kind)}
              value={formatDayCount(days)}
            />
          {/each}
        </div>
      {/if}

      <div style="margin-top:12px">
        <DataTable>
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
            {#each absenceReport as a (a.id)}
              {@const absUser = isLeadView
                ? users.find((u) => u.id === a.user_id)
                : null}
              <tr class:entry-rejected={a.status === "rejected"}>
                {#if isLeadView}
                  <td style="font-weight:500">
                    {absUser
                      ? `${absUser.first_name} ${absUser.last_name}`
                      : `#${a.user_id}`}
                  </td>
                {/if}
                <td>{absenceKindLabel(a.kind)}</td>
                <td class="tab-num" style="text-align:right">{fmtDate(a.start_date)}</td>
                <td class="tab-num" style="text-align:right">{fmtDate(a.end_date)}</td>
                <td class="tab-num" style="text-align:right">{formatDayCount(a.days)}</td>
                <td>
                  <span class="zf-chip zf-chip-{a.status}">{statusLabel(a.status)}</span>
                </td>
              </tr>
            {/each}
          </tbody>
        </DataTable>
      </div>
    {/if}
  {/if}
</SectionCard>
