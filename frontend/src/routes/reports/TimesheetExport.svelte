<script>
  import { currentUser, settings, toast } from "../../stores.js";
  import { t, absenceKindLabel, statusLabel } from "../../i18n.js";
  import { isoDate, appTodayDate, minToHM } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import DatePicker from "../../DatePicker.svelte";
  import SectionCard from "../../lib/ui/SectionCard.svelte";
  import { hasFlextimeAccount, tracksOwnTime } from "../../rolePolicy.js";
  import {
    getFlextimeReport,
    getRangeReport,
    getTimesheetPdf,
  } from "../../lib/api/reportsApi.js";
  import {
    isReportRangeTooLong,
    isoMonthStart,
  } from "../../lib/domain/dates.js";
  import { findUserById, hasUserId } from "../../lib/domain/users.js";

  export let users = [];
  export let isSelfOnlyReportsView = false;
  export let canViewTeamReports = false;

  // Sentinel selection value for "export all assigned employees into one PDF".
  // Only offered to leads/admins (see `canViewTeamReports`); CSV export does not
  // support it, so the CSV button is disabled while it is selected.
  const ALL_USERS_VALUE = "all";

  let today = appTodayDate();
  // eslint-disable-next-line no-useless-assignment
  let todayIso = isoDate(today);
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);

  // Pure-admin users (tracks_time=false) don't appear in `users`, so default
  // the export selection to the first available employee instead of themselves.
  let csvUserId = tracksOwnTime($currentUser) ? $currentUser.id : null;
  let csvFrom = isoMonthStart(today);
  let csvTo = todayIso;
  let csvError = "";
  let exportInProgress = false;
  let activeHelp = null;

  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // Force own user when in self-only mode.
  $: if (isSelfOnlyReportsView) {
    csvUserId = $currentUser.id;
  }

  // Fall back to the first available employee whenever the current selection
  // is missing (e.g. pure-admin login who has no own row in `users`).
  $: if (
    !isSelfOnlyReportsView &&
    csvUserId !== ALL_USERS_VALUE &&
    (csvUserId == null || !hasUserId(users, csvUserId)) &&
    users.length > 0
  ) {
    csvUserId = users[0].id;
  }

  // Lower bound: the selected export user's own start date.
  $: csvUserMinDate =
    findUserById(users, csvUserId, $currentUser)?.start_date || null;

  // Keep defaults aligned with app-timezone date changes if untouched.
  let previousCurrentMonthStr = "";
  let previousTodayIso = "";
  $: currentYear = today.getFullYear();
  $: currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: {
    if (!previousCurrentMonthStr) {
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
      // eslint-disable-next-line no-useless-assignment
      previousTodayIso = todayIso;
    } else {
      if (csvFrom === `${previousCurrentMonthStr}-01`)
        csvFrom = `${currentMonthStr}-01`;
      if (csvTo === previousTodayIso) csvTo = todayIso;
      // eslint-disable-next-line no-useless-assignment
      previousCurrentMonthStr = currentMonthStr;
      // eslint-disable-next-line no-useless-assignment
      previousTodayIso = todayIso;
    }
  }

  $: if (csvUserMinDate && csvFrom < csvUserMinDate) csvFrom = csvUserMinDate;

  function userHasFlextime(userId) {
    if (userId === $currentUser?.id) return hasFlextimeAccount($currentUser);
    const found = findUserById(users, userId);
    return found ? hasFlextimeAccount(found) : false;
  }

  // Cells starting with =, +, -, @, etc. are prefixed with a leading single-quote so
  // spreadsheets treat them as text (CSV formula-injection guard).
  function csvSafe(cellValue) {
    if (cellValue && /^[=+\-@\t\r]/.test(cellValue)) return "'" + cellValue;
    return cellValue;
  }

  function csvEncode(fields) {
    return fields
      .map((fieldValue) => {
        const s = fieldValue == null ? "" : String(fieldValue);
        return s.includes(",") ||
          s.includes('"') ||
          s.includes("\n") ||
          s.includes("\r")
          ? '"' + s.replace(/"/g, '""') + '"'
          : s;
      })
      .join(",");
  }

  function downloadBlob(blob, fileName) {
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = fileName;
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  function safeFileNamePart(value, fallback = "report") {
    const cleaned = String(value || "")
      .trim()
      .replace(/[^A-Za-z0-9._-]+/g, "-")
      .replace(/^-+|-+$/g, "");
    return cleaned || fallback;
  }

  async function fetchExportDataForUser(userId) {
    const exportUserHasFlextime = userHasFlextime(userId);
    return Promise.all([
      getRangeReport({ userId, from: csvFrom, to: csvTo }),
      exportUserHasFlextime
        ? getFlextimeReport({
            userId,
            from: csvFrom,
            to: csvTo,
          }).catch(() => [])
        : Promise.resolve([]),
    ]);
  }

  function flextimeBounds(flextimeData) {
    if (!flextimeData || flextimeData.length === 0) {
      return { opening: null, closing: null };
    }
    return {
      opening: flextimeData[0].cumulative_min - flextimeData[0].diff_min,
      closing: flextimeData[flextimeData.length - 1].cumulative_min,
    };
  }

  function validateRange() {
    csvError = "";
    if (
      csvUserId == null ||
      (csvUserId === ALL_USERS_VALUE && users.length === 0)
    ) {
      csvError = $t("Select an employee.");
      return false;
    }
    if (!csvFrom || !csvTo) {
      csvError = $t("Invalid date.");
      return false;
    }
    if (csvFrom > csvTo) {
      csvError = $t("From cannot be after To.");
      return false;
    }
    if (isReportRangeTooLong(csvFrom, csvTo)) {
      csvError = $t("Date range must not exceed 366 days.");
      return false;
    }
    return true;
  }

  async function exportCsv() {
    if (exportInProgress) return;
    if (!validateRange()) return;
    exportInProgress = true;
    try {
      const [report, flextimeData] = await fetchExportDataForUser(csvUserId);
      const { opening, closing } = flextimeBounds(flextimeData);
      const header = csvEncode([
        $t("Date"),
        $t("Weekday"),
        $t("Start"),
        $t("End"),
        $t("Category"),
        $t("Duration"),
        $t("Status"),
        $t("Comment"),
        $t("Absence"),
        $t("Holiday"),
      ]);
      const rows = [header];
      for (const day of report.days) {
        const weekday = $t(day.weekday);
        const absence = day.absence ? absenceKindLabel(day.absence) : "";
        const holiday = day.holiday || "";
        if (!day.entries || day.entries.length === 0) {
          rows.push(
            csvEncode([
              day.date,
              weekday,
              "",
              "",
              "",
              "0:00",
              "",
              "",
              csvSafe(absence),
              csvSafe(holiday),
            ]),
          );
        } else {
          for (const entry of day.entries) {
            rows.push(
              csvEncode([
                day.date,
                weekday,
                entry.start_time,
                entry.end_time,
                csvSafe($t(entry.category)),
                minToHM(entry.minutes || 0),
                statusLabel(entry.status),
                csvSafe(entry.comment || ""),
                csvSafe(absence),
                csvSafe(holiday),
              ]),
            );
          }
        }
      }
      const totalMin = report.days.reduce(
        (sum, d) =>
          sum +
          (d.entries || []).reduce(
            (entrySum, e) =>
              entrySum +
              (e.status === "approved" && e.counts_as_work !== false
                ? e.minutes || 0
                : 0),
            0,
          ),
        0,
      );
      rows.push(
        csvEncode([
          "",
          $t("Total"),
          "",
          "",
          "",
          minToHM(totalMin),
          "",
          "",
          "",
          "",
        ]),
      );
      if (opening !== null) {
        rows.push(
          csvEncode([
            "",
            $t("Flextime opening balance"),
            "",
            "",
            "",
            (opening >= 0 ? "+" : "") + minToHM(opening),
            "",
            "",
            "",
            "",
          ]),
        );
      }
      if (closing !== null) {
        rows.push(
          csvEncode([
            "",
            $t("Flextime closing balance"),
            "",
            "",
            "",
            (closing >= 0 ? "+" : "") + minToHM(closing),
            "",
            "",
            "",
            "",
          ]),
        );
      }
      const blob = new Blob(["\uFEFF" + rows.join("\r\n")], {
        type: "text/csv;charset=utf-8",
      });
      downloadBlob(
        blob,
        `stundennachweis-${safeFileNamePart(csvUserId)}-${csvFrom}_${csvTo}.csv`,
      );
      toast($t("CSV download started."), "ok");
    } catch (e) {
      csvError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }

  // PDF generation happens entirely in the backend: it builds either a
  // single-employee timesheet or — when "All" is selected — one combined PDF
  // covering every employee the requester leads. We just download the blob.
  async function exportPdf() {
    if (exportInProgress) return;
    if (!validateRange()) return;
    exportInProgress = true;
    try {
      const isAllUsers = csvUserId === ALL_USERS_VALUE;
      const selectedUser = isAllUsers ? null : findUserById(users, csvUserId);
      const fileNamePart = isAllUsers
        ? $t("All")
        : selectedUser
          ? `${selectedUser.first_name} ${selectedUser.last_name}`
          : String(csvUserId);
      const response = await getTimesheetPdf({
        userId: isAllUsers ? undefined : csvUserId,
        from: csvFrom,
        to: csvTo,
      });
      const blob = await response.blob();
      downloadBlob(
        blob,
        `stundennachweis-${safeFileNamePart(fileNamePart)}-${csvFrom}_${csvTo}.pdf`,
      );
      toast($t("PDF download started."), "ok");
    } catch (e) {
      csvError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }
</script>

<SectionCard
  title={$t("Export timesheet")}
  helpText={$t("help_csv_export")}
  helpOpen={activeHelp === "csv"}
  onHelpToggle={() => toggleHelp("csv")}
>
  {#if !isSelfOnlyReportsView}
    <div style="margin-bottom:12px">
      <label class="zf-label" for="csv-user-id">{$t("Employee")}</label>
      <select id="csv-user-id" class="zf-select" bind:value={csvUserId}>
        {#if canViewTeamReports}
          <option value={ALL_USERS_VALUE}>{$t("All")}</option>
        {/if}
        {#each users as u (u.id)}
          <option value={u.id}>{u.first_name} {u.last_name}</option>
        {/each}
      </select>
    </div>
  {/if}
  <div class="field-row" style="margin-bottom:12px">
    <div>
      <label class="zf-label" for="csv-from">{$t("From")}</label>
      <DatePicker
        id="csv-from"
        bind:value={csvFrom}
        min={csvUserMinDate}
        max={csvTo}
      />
    </div>
    <div>
      <label class="zf-label" for="csv-to">{$t("To")}</label>
      <DatePicker id="csv-to" bind:value={csvTo} min={csvFrom} max={todayIso} />
    </div>
  </div>

  <div class="error-text">{csvError}</div>
  <div style="display:flex;gap:8px;flex-wrap:wrap">
    <button
      class="zf-btn zf-btn-primary"
      on:click={exportCsv}
      disabled={exportInProgress || csvUserId == null || csvUserId === ALL_USERS_VALUE}
      title={csvUserId === ALL_USERS_VALUE
        ? $t("CSV export is only available for a single employee.")
        : null}
    >
      <Icon name="Download" size={14} />{$t("Export CSV")}
    </button>
    <button
      class="zf-btn zf-btn-primary"
      on:click={exportPdf}
      disabled={exportInProgress || csvUserId == null}
    >
      <Icon name="FileText" size={14} />{$t("Export PDF")}
    </button>
  </div>
</SectionCard>
