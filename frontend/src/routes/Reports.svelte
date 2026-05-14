<script>
  // Reports page for monthly and team-related statistics.
  // The card order is: employee report, team report,
  // category breakdown, absences, and timesheet export.
  // Current-day hours are included in the monthly report. Boundary weeks count by day for
  // month totals, but they count for both months when checking week submission.

  import { api } from "../api.js";
  import { currentUser, settings, toast } from "../stores.js";
  import {
    t,
    absenceKindLabel,
    statusLabel,
    formatHours,
    fmtDecimal,
    formatDayCount,
  } from "../i18n.js";
  import {
    isoDate,
    appTodayDate,
    minToHM,
    fmtDate,
    fmtMonthLabel,
  } from "../format.js";
  import {
    normalizeMonthReport,
    countWorkdays,
    holidayDateSet,
  } from "../apiMappers.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";
  import FlextimeChart from "../FlextimeChart.svelte";
  import { jsPDF } from "jspdf";
  import { hasFlextimeAccount, isAssistantUser } from "../rolePolicy.js";

  // Date reference is tied to configured app timezone.
  let today = new Date();
  let todayIso = isoDate(today);
  let currentYear = today.getFullYear();
  let currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: today = appTodayDate($settings?.timezone);
  $: todayIso = isoDate(today);
  $: currentYear = today.getFullYear();
  $: currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;
  $: canViewTeamReports = !!$currentUser?.permissions?.can_view_team_reports;
  $: isSelfOnlyReportsView = !canViewTeamReports;

  // Leads and admins load all users for the employee dropdown.
  // Non-lead roles only see their own data.
  let users = [];
  async function initUsers() {
    try {
      users =
        isSelfOnlyReportsView ? [$currentUser] : await api("/users");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  initUsers();

  let activeHelp = null;
  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // Section 1: employee report.
  // Merges the previously separate "Employee details" and "Monthly report" cards
  // into one combined card.
  // For leads/admins: employee dropdown is visible.
  // For employees: no dropdown; own data is selected automatically.
  // After "Show" the following are loaded:
  // - Monthly report with target, actual, diff, entries, and absences.
  // - Leave balance for the selected year.
  let reportUserId = $currentUser.id;
  let reportMonth = currentMonthStr;
  // reportData holds all needed information after loading.
  let reportData = null;
  $: selectedReportUser =
    users.find((user) => user.id === Number(reportUserId)) ||
    ($currentUser?.id === Number(reportUserId) ? $currentUser : null);
  $: selectedUserIsAssistant = isAssistantUser(selectedReportUser);
  $: selectedUserHasFlextime = hasFlextimeAccount(selectedReportUser);

  function userById(userId) {
    return (
      users.find((user) => user.id === Number(userId)) ||
      ($currentUser?.id === Number(userId) ? $currentUser : null)
    );
  }

  function userHasFlextime(userId) {
    return hasFlextimeAccount(userById(userId));
  }

  function userWorkdaysPerWeek(userId, fallback = 5) {
    const matchedUser = users.find((user) => user.id === userId);
    const value = Number(matchedUser?.workdays_per_week);
    return Number.isFinite(value) && value >= 1 && value <= 7
      ? value
      : fallback;
  }

  function monthStart(monthKey) {
    return `${monthKey}-01`;
  }

  function isoMonthStart(dateValue) {
    return `${dateValue.getFullYear()}-${String(dateValue.getMonth() + 1).padStart(2, "0")}-01`;
  }

  function monthEnd(monthKey) {
    const [yearPart, monthPart] = monthKey.split("-");
    const year = Number(yearPart);
    const month = Number(monthPart);
    const lastDay = new Date(year, month, 0).getDate();
    return `${monthKey}-${String(lastDay).padStart(2, "0")}`;
  }

  async function loadReport() {
    try {
      const reportYear = reportMonth.slice(0, 4);
      const reportYearNum = parseInt(reportYear);
      const isCurrentMonth = reportMonth === currentMonthStr;
      const chartMonthFrom = monthStart(reportMonth);
      const chartMonthTo = isCurrentMonth ? todayIso : monthEnd(reportMonth);
      // Only fetch when the selected month includes at least one day up to today.
      const canFetchChart = reportYearNum < currentYear || chartMonthFrom <= todayIso;

      const [monthRaw, leaveRaw, overtimeRows, flextimeRaw] =
        await Promise.all([
          api(`/reports/month?user_id=${reportUserId}&month=${reportMonth}`),
          api(`/leave-balance/${reportUserId}?year=${reportYear}`).catch(
            () => null,
          ),
          selectedUserHasFlextime
            ? api(
                `/reports/overtime?user_id=${reportUserId}&year=${reportYear}`,
              ).catch(() => null)
            : Promise.resolve(null),
          canFetchChart && selectedUserHasFlextime
            ? api(
                `/reports/flextime?user_id=${reportUserId}&from=${chartMonthFrom}&to=${chartMonthTo}`,
              ).catch(() => [])
            : Promise.resolve([]),
        ]);

      const monthReport = normalizeMonthReport(
        monthRaw,
        userWorkdaysPerWeek(reportUserId, Number($currentUser?.workdays_per_week || 5)),
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

  // Absence summary for stat cards: { vacation: 2, sick: 1, ... }
  $: reportAbsenceSummary = (() => {
    if (!reportData) return {};
    const map = {};
    for (const absenceEntry of reportData.monthReport.absences || []) {
      map[absenceEntry.kind] =
        (map[absenceEntry.kind] || 0) + (absenceEntry.days || 0);
    }
    return map;
  })();

  // Section 3: team report per employee.
  // Visible for leads and admins only.
  // Columns: flextime balance, monthly diff, sick days, vacation taken/planned,
  //          all weeks submitted.
  // For the current month: all values relative to working days from the 1st to today.
  let teamMonth = currentMonthStr;
  let teamReport = null;

  async function showTeam() {
    try {
      teamReport = await api(`/reports/team?month=${teamMonth}`);
    } catch (e) {
      teamReport = null;
      toast($t(e?.message || "Error"), "error");
    }
  }

  // Section 4: category breakdown.
  // Employees: own bookings as a ranked list.
  // Leads/admins: employee by category matrix.
  // Note: the backend only excludes "rejected" entries, so submitted
  // (not yet approved) bookings also appear.
  let catFrom = `${currentYear}-01-01`;
  // Category reports now include today's entries by default.
  let catTo = todayIso;
  let catReport = null;
  let teamCatReport = null;
  let catFilteredCategories = [];
  let catShowFilter = false;

  function categoryNamesFromTeamReport(rows) {
    return [
      ...new Set(
        (rows || []).flatMap((row) =>
          (row.categories || []).map((categoryEntry) => categoryEntry.category),
        ),
      ),
    ];
  }

  async function showCat() {
    if (catFrom > catTo) return;
    try {
      const params = new URLSearchParams({ from: catFrom, to: catTo });
      if (isSelfOnlyReportsView) {
        // Employees see their own category breakdown.
        params.set("user_id", $currentUser.id);
        catReport = await api(`/reports/categories?${params}`);
        teamCatReport = null;
        catFilteredCategories = catReport.map(
          (categoryEntry) => categoryEntry.category,
        );
      } else {
        // Leads and admins see the team matrix, including their own row.
        teamCatReport = await api(`/reports/team-categories?${params}`);
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
      ? catFilteredCategories.filter(
          (filteredCategoryName) => filteredCategoryName !== categoryName,
        )
      : [...catFilteredCategories, categoryName];
  }

  // Apply the active category filter to the individual list.
  $: filteredCatReport = catReport
    ? catReport.filter((categoryRow) =>
        catFilteredCategories.includes(categoryRow.category),
      )
    : catReport;
  $: filteredCatTotal = (filteredCatReport || []).reduce(
    (totalMinutes, categoryRow) => totalMinutes + categoryRow.minutes,
    0,
  );

  // Columns for the team matrix (sorted descending by total minutes).
  $: allTeamCatColumns = (() => {
    if (!teamCatReport) return [];
    const totals = new Map();
    for (const row of teamCatReport) {
      for (const categoryEntry of row.categories) {
        const entryTotals = totals.get(categoryEntry.category) || {
          color: categoryEntry.color,
          total: 0,
        };
        entryTotals.total += categoryEntry.minutes;
        totals.set(categoryEntry.category, entryTotals);
      }
    }
    return [...totals.entries()]
      .sort((a, b) => b[1].total - a[1].total)
      .map(([category, { color }]) => ({ category, color }));
  })();
  $: visibleTeamCatColumns = allTeamCatColumns.filter((column) =>
    catFilteredCategories.includes(column.category),
  );

  function teamCatMinutes(row, category) {
    const categoryEntry = row.categories.find(
      (categoryRow) => categoryRow.category === category,
    );
    return categoryEntry ? categoryEntry.minutes : 0;
  }
  function teamCatRowTotal(row) {
    return row.categories.reduce(
      (totalMinutes, categoryEntry) =>
        catFilteredCategories.includes(categoryEntry.category)
          ? totalMinutes + categoryEntry.minutes
          : totalMinutes,
      0,
    );
  }

  // Section 5: absences.
  // Shows absence entries in the selected date range with type distribution.
  // Employees load only their own absences; leads/admins see all.
  let absenceFrom = isoMonthStart(today);
  let absenceTo = `${currentYear}-12-31`;
  let absenceReport = null;
  $: absenceTotalDays = (absenceReport || []).reduce(
    (totalDays, absenceEntry) => totalDays + (absenceEntry.days || 0),
    0,
  );
  $: absenceByKind = (absenceReport || []).reduce(
    (daysByKind, absenceEntry) => {
      const absenceKind = absenceEntry.kind || "unknown";
      daysByKind[absenceKind] =
        (daysByKind[absenceKind] || 0) + (absenceEntry.days || 0);
      return daysByKind;
    },
    {},
  );
  $: isLeadView = canViewTeamReports;
  let absenceHolidayDates = new Set();

  // Clamps the absence date range to the selected from/to window.
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
    const workdaysPerWeek = userWorkdaysPerWeek(
      absence?.user_id,
      Number($currentUser?.workdays_per_week || 5),
    );
    // Count absence days using user's workdays_per_week (respects flexible work schedules).
    // Example: 4-day worker's absence only counts Mon-Thu, not Fri-Sun.
    return countWorkdays(
      clamped.from,
      clamped.to,
      absenceHolidayDates,
      workdaysPerWeek,
    );
  }

  async function showAbsences() {
    if (absenceFrom > absenceTo) return;
    try {
      let raw;
      if (isSelfOnlyReportsView) {
        // The personal absence API is year-based. Cross-year ranges therefore
        // need multiple requests and id-based deduplication.
        const fromYear = parseInt(absenceFrom.slice(0, 4), 10);
        const toYear = parseInt(absenceTo.slice(0, 4), 10);
        const years = Array.from(
          { length: toYear - fromYear + 1 },
          (_, yearOffset) => fromYear + yearOffset,
        );
        const absenceLists = await Promise.all(
          years.map((yearValue) => api(`/absences?year=${yearValue}`)),
        );
        const seen = new Set();
        raw = absenceLists.flat().filter((absenceEntry) => {
          if (seen.has(absenceEntry.id)) return false;
          seen.add(absenceEntry.id);
          return (
            absenceEntry.end_date >= absenceFrom &&
            absenceEntry.start_date <= absenceTo
          );
        });
      } else {
        const params = new URLSearchParams({
          from: absenceFrom,
          to: absenceTo,
        });
        raw = await api(`/absences/all?${params}`);
      }
      // Exclude absences that are no longer valid.
      raw = raw.filter(
        (a) => a.status !== "rejected" && a.status !== "cancelled",
      );
      // Load holidays for all involved years (needed for correct working-day counts).
      const allYears = [
        ...new Set(
          raw.flatMap((absenceEntry) => [
            parseInt(absenceEntry.start_date.slice(0, 4), 10),
            parseInt(absenceEntry.end_date.slice(0, 4), 10),
          ]),
        ),
      ];
      const holidayLists = await Promise.all(
        allYears.map((yearValue) => api(`/holidays?year=${yearValue}`)),
      );
      absenceHolidayDates = holidayDateSet(holidayLists.flat());
      absenceReport = raw.map((absenceEntry) => ({
        ...absenceEntry,
        days: absenceDays(absenceEntry),
      }));
    } catch (e) {
      absenceReport = null;
      absenceHolidayDates = new Set();
      toast($t(e?.message || "Error"), "error");
    }
  }

  // Section 6: timesheet export as CSV or PDF.
  // Leads/admins can select any employee.
  // Employees always export their own data.
  // Desktop layout: a new row starts after the employee dropdown.
  let csvUserId = $currentUser.id;
  let csvFrom = isoMonthStart(today);
  let csvTo = todayIso;
  let csvError = "";
  let exportInProgress = false;

  // Keep untouched defaults aligned with app-timezone date changes.
  let previousCurrentMonthStr = "";
  let previousCurrentYear = 0;
  let previousTodayIso = "";
  $: {
    if (!previousCurrentMonthStr) {
      previousCurrentMonthStr = currentMonthStr;
      previousCurrentYear = currentYear;
      previousTodayIso = todayIso;
    } else {
      if (reportMonth === previousCurrentMonthStr) reportMonth = currentMonthStr;
      if (teamMonth === previousCurrentMonthStr) teamMonth = currentMonthStr;
      if (absenceFrom === `${previousCurrentMonthStr}-01`) absenceFrom = `${currentMonthStr}-01`;
      if (csvFrom === `${previousCurrentMonthStr}-01`) csvFrom = `${currentMonthStr}-01`;
      if (catFrom === `${previousCurrentYear}-01-01`) catFrom = `${currentYear}-01-01`;
      if (absenceTo === `${previousCurrentYear}-12-31`) absenceTo = `${currentYear}-12-31`;
      if (catTo === previousTodayIso) catTo = todayIso;
      if (csvTo === previousTodayIso) csvTo = todayIso;

      previousCurrentMonthStr = currentMonthStr;
      previousCurrentYear = currentYear;
      previousTodayIso = todayIso;
    }
  }

  $: if (isSelfOnlyReportsView) {
    reportUserId = $currentUser.id;
    csvUserId = $currentUser.id;
  }

  // CSV formula-injection guard: cells starting with =, +, -, @, etc.
  // are prefixed with a leading single-quote so spreadsheets treat them as text.
  function csvSafe(cellValue) {
    if (cellValue && /^[=+\-@\t\r]/.test(cellValue)) return "'" + cellValue;
    return cellValue;
  }

  // Encode one RFC 4180 CSV row.
  function csvEncode(fields) {
    return fields
      .map((fieldValue) => {
        const stringValue = fieldValue == null ? "" : String(fieldValue);
        return stringValue.includes(",") ||
          stringValue.includes('"') ||
          stringValue.includes("\n")
          ? '"' + stringValue.replace(/"/g, '""') + '"'
          : stringValue;
      })
      .join(",");
  }

  // Creates a temporary <a> link and triggers the browser download.
  function downloadBlob(blob, fileName) {
    const url = URL.createObjectURL(blob);
    const downloadLink = document.createElement("a");
    downloadLink.href = url;
    downloadLink.download = fileName;
    document.body.appendChild(downloadLink);
    downloadLink.click();
    downloadLink.remove();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  async function exportCsv() {
    if (exportInProgress) return;
    csvError = "";
    if (!csvFrom || !csvTo) {
      csvError = $t("Invalid date.");
      return;
    }
    if (csvFrom > csvTo) {
      csvError = $t("From cannot be after To.");
      return;
    }
    exportInProgress = true;
    try {
      const params = new URLSearchParams({
        user_id: String(csvUserId),
        from: csvFrom,
        to: csvTo,
      });
      const exportUserHasFlextime = userHasFlextime(csvUserId);
      const [report, flextimeData] = await Promise.all([
        api(`/reports/range?${params}`),
        exportUserHasFlextime
          ? api(`/reports/flextime?${params}`).catch(() => [])
          : Promise.resolve([]),
      ]);
      // Derive opening balance (cumulative at day before from) and closing
      // balance (cumulative at the last day in the range).
      const openingBalance =
        flextimeData.length > 0
          ? flextimeData[0].cumulative_min - flextimeData[0].diff_min
          : null;
      const closingBalance =
        flextimeData.length > 0
          ? flextimeData[flextimeData.length - 1].cumulative_min
          : null;
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
        (summaryMinutes, reportDay) =>
          summaryMinutes +
          (reportDay.entries || []).reduce(
            (entryMinutes, entry) =>
              entryMinutes +
              (entry.status === "approved" && entry.counts_as_work !== false
                ? entry.minutes || 0
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
      // Flextime balance rows (opening and closing).
      if (openingBalance !== null) {
        rows.push(
          csvEncode([
            "",
            $t("Flextime opening balance"),
            "",
            "",
            "",
            (openingBalance >= 0 ? "+" : "") + minToHM(openingBalance),
            "",
            "",
            "",
            "",
          ]),
        );
      }
      if (closingBalance !== null) {
        rows.push(
          csvEncode([
            "",
            $t("Flextime closing balance"),
            "",
            "",
            "",
            (closingBalance >= 0 ? "+" : "") + minToHM(closingBalance),
            "",
            "",
            "",
            "",
          ]),
        );
      }
      const blob = new Blob(["\uFEFF" + rows.join("\n")], {
        type: "text/csv;charset=utf-8",
      });
      downloadBlob(
        blob,
        `stundennachweis-${csvUserId}-${csvFrom}_${csvTo}.csv`,
      );
      toast($t("CSV download started."), "ok");
    } catch (e) {
      csvError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }

  async function exportPdf() {
    if (exportInProgress) return;
    csvError = "";
    if (!csvFrom || !csvTo) {
      csvError = $t("Invalid date.");
      return;
    }
    if (csvFrom > csvTo) {
      csvError = $t("From cannot be after To.");
      return;
    }
    exportInProgress = true;
    try {
      const params = new URLSearchParams({
        user_id: String(csvUserId),
        from: csvFrom,
        to: csvTo,
      });
      const exportUserHasFlextime = userHasFlextime(csvUserId);
      const [report, flextimeData] = await Promise.all([
        api(`/reports/range?${params}`),
        exportUserHasFlextime
          ? api(`/reports/flextime?${params}`).catch(() => [])
          : Promise.resolve([]),
      ]);
      const openingBalance =
        flextimeData.length > 0
          ? flextimeData[0].cumulative_min - flextimeData[0].diff_min
          : null;
      const closingBalance =
        flextimeData.length > 0
          ? flextimeData[flextimeData.length - 1].cumulative_min
          : null;
      const selectedUser = users.find((userRow) => userRow.id === csvUserId);
      const fullName = selectedUser
        ? `${selectedUser.first_name} ${selectedUser.last_name}`
        : String(csvUserId);

      const doc = new jsPDF({ unit: "mm", format: "a4" });
      const pageHeight = 297,
        marginLeft = 15,
        marginTop = 15,
        contentWidth = 180;
      const rowHeight = 5.5,
        headerHeight = 7;
      let currentY = marginTop;

      // Column widths total 180 mm.
      // "Holiday" column needs 33 mm for long names like "Christi Himmelfahrt".
      const cols = [
        [$t("Date"), 22, "left"],
        [$t("Weekday"), 20, "left"],
        [$t("Start"), 12, "center"],
        [$t("End"), 12, "center"],
        [$t("Category"), 40, "left"],
        [$t("Duration"), 16, "right"],
        [$t("Absence"), 25, "left"],
        [$t("Holiday"), 33, "left"],
      ]; // 22+20+12+12+40+16+25+33 = 180

      function colX(columnIndex) {
        let currentX = marginLeft;
        for (
          let previousColumnIndex = 0;
          previousColumnIndex < columnIndex;
          previousColumnIndex++
        )
          currentX += cols[previousColumnIndex][1];
        return currentX;
      }
      function textX(columnIndex) {
        const [, width, align] = cols[columnIndex];
        if (align === "right") return colX(columnIndex) + width - 1;
        if (align === "center") return colX(columnIndex) + width / 2;
        return colX(columnIndex) + 1;
      }
      function drawHeader() {
        doc.setFillColor(235, 235, 235);
        doc.rect(marginLeft, currentY, contentWidth, headerHeight, "F");
        doc.setFont("helvetica", "bold");
        doc.setFontSize(8);
        doc.setTextColor(50, 50, 50);
        cols.forEach(([label, , align], columnIndex) =>
          doc.text(label, textX(columnIndex), currentY + 4.8, { align }),
        );
        currentY += headerHeight;
      }
      function drawRow(cells, shade) {
        if (currentY + rowHeight > pageHeight - 15) {
          doc.addPage();
          currentY = marginTop;
          drawHeader();
        }
        if (shade) {
          doc.setFillColor(248, 248, 248);
          doc.rect(marginLeft, currentY, contentWidth, rowHeight, "F");
        }
        doc.setFont("helvetica", "normal");
        doc.setFontSize(7.5);
        doc.setTextColor(30, 30, 30);
        cells.forEach(([text, columnIndex]) => {
          const [, , align] = cols[columnIndex];
          doc.text(String(text ?? ""), textX(columnIndex), currentY + 3.8, {
            align,
          });
        });
        doc.setDrawColor(220, 220, 220);
        doc.line(
          marginLeft,
          currentY + rowHeight,
          marginLeft + contentWidth,
          currentY + rowHeight,
        );
        currentY += rowHeight;
      }

      // Title block.
      doc.setFont("helvetica", "bold");
      doc.setFontSize(13);
      doc.setTextColor(20, 20, 20);
      doc.text($t("Timesheet"), marginLeft, currentY + 6);
      doc.setFont("helvetica", "normal");
      doc.setFontSize(9);
      doc.setTextColor(90, 90, 90);
      doc.text(
        `${fullName} - ${csvFrom} to ${csvTo}`,
        marginLeft,
        currentY + 12,
      );
      currentY += 20;
      drawHeader();

      let rowIdx = 0;
      for (const day of report.days) {
        const absence = day.absence ? absenceKindLabel(day.absence) : "";
        const holiday = day.holiday || "";
        const weekday = $t(day.weekday);
        if (!day.entries || day.entries.length === 0) {
          drawRow(
            [
              [day.date, 0],
              [weekday, 1],
              ["", 2],
              ["", 3],
              ["", 4],
              ["0:00", 5],
              [absence, 6],
              [holiday, 7],
            ],
            rowIdx % 2 === 1,
          );
          rowIdx++;
        } else {
          for (const entry of day.entries) {
            drawRow(
              [
                [day.date, 0],
                [weekday, 1],
                [entry.start_time?.slice(0, 5) ?? "", 2],
                [entry.end_time?.slice(0, 5) ?? "", 3],
                [$t(entry.category ?? ""), 4],
                [minToHM(entry.minutes || 0), 5],
                [absence, 6],
                [holiday, 7],
              ],
              rowIdx % 2 === 1,
            );
            rowIdx++;
          }
        }
      }

      // Total row.
      if (currentY + rowHeight > pageHeight - 15) {
        doc.addPage();
        currentY = marginTop;
        drawHeader();
      }
      doc.setFillColor(235, 235, 235);
      doc.rect(marginLeft, currentY, contentWidth, rowHeight, "F");
      doc.setFont("helvetica", "bold");
      doc.setFontSize(7.5);
      doc.setTextColor(20, 20, 20);
      doc.text($t("Total"), marginLeft + 1, currentY + 3.8);
      const pdfTotalMin = report.days.reduce(
        (summaryMinutes, reportDay) =>
          summaryMinutes +
          (reportDay.entries || []).reduce(
            (entryMinutes, entry) =>
              entryMinutes +
              (entry.status === "approved" && entry.counts_as_work !== false
                ? entry.minutes || 0
                : 0),
            0,
          ),
        0,
      );
      doc.text(minToHM(pdfTotalMin), textX(5), currentY + 3.8, {
        align: "right",
      });
      currentY += rowHeight;

      // Flextime balance rows.
      function drawSummaryRow(label, value) {
        if (currentY + rowHeight > pageHeight - 15) {
          doc.addPage();
          currentY = marginTop;
        }
        doc.setFont("helvetica", "normal");
        doc.setFontSize(7.5);
        doc.setTextColor(90, 90, 90);
        doc.text(label, marginLeft + 1, currentY + 3.8);
        doc.text(value, textX(5), currentY + 3.8, { align: "right" });
        currentY += rowHeight;
      }
      if (openingBalance !== null) {
        drawSummaryRow(
          $t("Flextime opening balance"),
          (openingBalance >= 0 ? "+" : "") + minToHM(openingBalance),
        );
      }
      if (closingBalance !== null) {
        drawSummaryRow(
          $t("Flextime closing balance"),
          (closingBalance >= 0 ? "+" : "") + minToHM(closingBalance),
        );
      }

      doc.save(
        `stundennachweis-${fullName.replace(/\s+/g, "-")}-${csvFrom}_${csvTo}.pdf`,
      );
      toast($t("PDF download started."), "ok");
    } catch (e) {
      csvError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }
</script>

<!-- Page header -->
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
  <!-- Card 1: employee report for the selected employee and month. -->
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400">{$t("Employee report")}</span
      >
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

    <!-- Controls row: employee dropdown (leads/admins only) + month picker -->
    <div class="field-row" style="margin-bottom:12px">
      {#if !isSelfOnlyReportsView}
        <!-- Leads and admins can select any employee. -->
        <div>
          <label class="zf-label" for="report-user-id">{$t("Employee")}</label>
          <select
            id="report-user-id"
            class="zf-select"
            bind:value={reportUserId}
          >
            {#each users as u}
              <option value={u.id}>{u.first_name} {u.last_name}</option>
            {/each}
          </select>
        </div>
      {/if}
      <div>
        <label class="zf-label" for="report-month">{$t("Month")}</label>
        <DatePicker id="report-month" mode="month" bind:value={reportMonth} />
      </div>
    </div>

    <button class="zf-btn zf-btn-primary" on:click={loadReport}
      >{$t("Show")}</button
    >

    {#if reportData}
      <!-- Summary stat cards from the employee's personal dashboard. -->
      <div
        style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-top:20px;margin-bottom:6px"
      >
        {$t("My Balance")}
      </div>
      <div class="stat-cards" style="margin-bottom:16px">
        <!-- Submitted hours vs. full-month target -->
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
            {formatHours(
              (reportData.monthReport.submitted_min || 0) / 60,
            )}
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

        <!-- Flextime balance at end of selected month -->
        {#if selectedUserHasFlextime}
          <div class="zf-card stat-card">
            <div class="stat-card-label">{$t("Flextime balance")}</div>
            <div
              class="stat-card-value tab-num"
              style="color:{(reportData.flextimeBalance ?? 0) < 0
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

        <!-- Submission status with the same wording as on the dashboard -->
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
        </div>
      </div>

      {#if activeHelp === "logged"}
        <div
          style="font-size:12px;color:var(--text-tertiary);margin-top:-6px;margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
        >
          {$t("help_logged")}
        </div>
      {/if}
      {#if activeHelp === "approvals"}
        <div
          style="font-size:12px;color:var(--text-tertiary);margin-top:-6px;margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)"
        >
          {$t("help_submission_status")}
        </div>
      {/if}

      <!-- Leave balance, if available. -->
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

      <!-- Absence stat cards for the selected month. -->
      {#if Object.keys(reportAbsenceSummary).length > 0}
        <div
          style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-bottom:6px"
        >
          {$t("Absences")}
        </div>
        <div class="stat-cards" style="margin-bottom:16px">
          {#each Object.entries(reportAbsenceSummary) as [kind, days]}
            <div class="zf-card stat-card">
              <div class="stat-card-label">{absenceKindLabel(kind)}</div>
              <div class="stat-card-value tab-num">{formatDayCount(days)}</div>
              <div class="stat-card-sub">{$t("days")}</div>
            </div>
          {/each}
        </div>
      {/if}

      <!-- Category totals as a compact bar chart. -->
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
            {#each catEntries as [cat, mins]}
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

      <!-- Time entries table. -->
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
              {#each reportData.monthReport.entries as e}
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

      <!-- Absence table. -->
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
              {#each reportData.monthReport.absences as a}
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

      <!-- Gleitzeitkonto-Verlauf für das gewählte Jahr -->
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

  <!-- Card 3: team report for leads and admins. -->
  {#if $currentUser.permissions?.can_view_team_reports}
    <div class="zf-card" style="padding:20px;margin-bottom:16px">
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
        <span style="font-size:14px;font-weight:400">{$t("Team report")}</span>
        <button
          class="zf-btn-icon-sm zf-btn-ghost"
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
          <label class="zf-label" for="team-month">{$t("Month")}</label>
          <DatePicker id="team-month" mode="month" bind:value={teamMonth} />
        </div>
        <button class="zf-btn zf-btn-primary" on:click={showTeam}
          >{$t("Show")}</button
        >
      </div>

      {#if teamReport}
        <!-- Scrollable table with all columns -->
        <div class="zf-table-wrap">
          <table class="zf-table zf-table--fit">
            <thead>
              <tr>
                <!-- Name -->
                <th style="min-width:120px">{$t("Employee")}</th>
                <!-- Current flextime balance -->
                <th style="text-align:right;white-space:nowrap"
                  >{$t("Current flextime balance")}</th
                >
                <!-- Monthly diff (overtime / minus hours) -->
                <th style="text-align:right;white-space:nowrap"
                  >{$t("Monthly diff")}</th
                >
                <!-- Sick days -->
                <th style="text-align:right;white-space:nowrap"
                  >{$t("Sick days")}</th
                >
                <!-- Vacation days taken -->
                <th style="text-align:right;white-space:nowrap"
                  >{$t("Vacation taken")}</th
                >
                <!-- Vacation days planned -->
                <th style="text-align:right;white-space:nowrap"
                  >{$t("Vacation planned")}</th
                >
                <!-- All past weeks submitted? -->
                <th style="text-align:center;white-space:nowrap"
                  >{$t("All weeks submitted")}</th
                >
              </tr>
            </thead>
            <tbody>
              {#each teamReport as r}
                <tr>
                  <td style="font-weight:500">{r.name}</td>
                  <!-- Flextime balance: red = deficit, green = zero or surplus -->
                  <td
                    class="tab-num"
                    style="text-align:right;font-weight:500;color:{r.flextime_balance_min == null
                      ? 'var(--text-tertiary)'
                      : r.flextime_balance_min < 0
                        ? 'var(--danger-text)'
                        : 'var(--success-text)'}"
                  >
                    {#if r.flextime_balance_min == null}
                      -
                    {:else}
                      {r.flextime_balance_min >= 0 ? "+" : ""}{minToHM(
                        r.flextime_balance_min,
                      )}
                    {/if}
                  </td>
                  <!-- Monthly diff -->
                  <td
                    class="tab-num"
                    style="text-align:right;color:{r.diff_min == null
                      ? 'var(--text-tertiary)'
                      : r.diff_min < 0
                        ? 'var(--danger-text)'
                        : 'var(--success-text)'}"
                  >
                    {#if r.diff_min == null}
                      -
                    {:else}
                      {r.diff_min >= 0 ? "+" : ""}{minToHM(r.diff_min)}
                    {/if}
                  </td>
                  <!-- Sick days (decimal, as half-days are possible) -->
                  <td
                    class="tab-num"
                    style="text-align:right;color:var(--text-tertiary)"
                  >
                    {r.sick_days > 0
                      ? fmtDecimal(r.sick_days, r.sick_days % 1 === 0 ? 0 : 1)
                      : "-"}
                  </td>
                  <!-- Vacation taken -->
                  <td
                    class="tab-num"
                    style="text-align:right;color:var(--text-tertiary)"
                  >
                    {r.vacation_days > 0
                      ? fmtDecimal(r.vacation_days, r.vacation_days % 1 === 0 ? 0 : 1)
                      : "-"}
                  </td>
                  <!-- Vacation planned -->
                  <td
                    class="tab-num"
                    style="text-align:right;color:var(--text-tertiary)"
                  >
                    {r.vacation_planned_days > 0
                      ? fmtDecimal(r.vacation_planned_days, r.vacation_planned_days % 1 === 0 ? 0 : 1)
                      : "-"}
                  </td>
                  <!-- All weeks submitted, rendered as text for accessibility. -->
                  <td style="text-align:center">
                    {#if r.weeks_all_submitted}
                      <span style="color:var(--success-text)">{$t("Yes")}</span>
                    {:else}
                      <span style="color:var(--danger-text)">{$t("No")}</span>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Card 4: category breakdown. -->
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400"
        >{$t("Category breakdown")}</span
      >
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
        <DatePicker id="cat-from" bind:value={catFrom} max={catTo} />
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
      <button class="zf-btn zf-btn-primary" on:click={showCat}
        >{$t("Run")}</button
      >
      <!-- Filter button: visible only when results are available -->
      {#if (catReport && catReport.length > 0) || allTeamCatColumns.length > 0}
        <button
          class="zf-btn"
          on:click={() => (catShowFilter = !catShowFilter)}
        >
          {$t("Filter")}
          {#if catFilteredCategories.length > 0}
            ({catFilteredCategories.length})
          {/if}
        </button>
      {/if}
    </div>

    <!-- Lead/admin filter panel (categories of the team matrix) -->
    {#if catShowFilter && allTeamCatColumns.length > 0}
      <div
        style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px"
      >
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          {#each allTeamCatColumns as col}
            <label
              style="display:flex;align-items:center;gap:6px;cursor:pointer"
            >
              <input
                type="checkbox"
                checked={catFilteredCategories.includes(col.category)}
                on:change={() => toggleCategoryFilter(col.category)}
              />
              <span class="cat-dot" style="background:{col.color || '#999'}"
              ></span>
              <span style="font-size:13px">{$t(col.category)}</span>
            </label>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Employee filter panel (own categories) -->
    {#if catShowFilter && catReport && catReport.length > 0}
      <div
        style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px"
      >
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          {#each catReport as cat}
            <label
              style="display:flex;align-items:center;gap:6px;cursor:pointer"
            >
              <input
                type="checkbox"
                checked={catFilteredCategories.includes(cat.category)}
                on:change={() => toggleCategoryFilter(cat.category)}
              />
              <span class="cat-dot" style="background:{cat.color || '#999'}"
              ></span>
              <span style="font-size:13px">{$t(cat.category)}</span>
            </label>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Team matrix (lead / admin) -->
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
                {#each visibleTeamCatColumns as col}
                  <th style="text-align:right">
                    <span
                      style="display:inline-flex;align-items:center;gap:4px;justify-content:flex-end"
                    >
                      <span
                        class="cat-dot"
                        style="background:{col.color || '#999'}"
                      ></span>
                      {$t(col.category)}
                    </span>
                  </th>
                {/each}
                <th style="text-align:right">{$t("Total")}</th>
              </tr>
            </thead>
            <tbody>
              {#each teamCatReport as row}
                {@const rowTotal = teamCatRowTotal(row)}
                <tr>
                  <td style="font-weight:500">{row.name}</td>
                  {#each visibleTeamCatColumns as col}
                    <td
                      class="tab-num"
                      style="text-align:right;color:var(--text-tertiary)"
                    >
                      {#if teamCatMinutes(row, col.category) > 0}
                        {minToHM(teamCatMinutes(row, col.category))}
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

    <!-- Employee category list (own categories) -->
    {#if catReport}
      {#if catReport.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
          {$t("No data.")}
        </div>
      {:else if filteredCatReport && filteredCatReport.length === 0 && catFilteredCategories.length > 0}
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

  <!-- Card 5: absences for a selectable date range. -->
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400">{$t("Absences")}</span>
      <button
        class="zf-btn-icon-sm zf-btn-ghost"
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
        {$t("help_absence_report")}
      </div>
    {/if}

    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="zf-label" for="absence-from">{$t("From")}</label>
        <DatePicker
          id="absence-from"
          bind:value={absenceFrom}
          max={absenceTo}
        />
      </div>
      <div>
        <label class="zf-label" for="absence-to">{$t("To")}</label>
        <DatePicker id="absence-to" bind:value={absenceTo} min={absenceFrom} />
      </div>
    </div>
    <button class="zf-btn zf-btn-primary" on:click={showAbsences}
      >{$t("Run")}</button
    >

    {#if absenceReport}
      {#if absenceReport.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">
          {$t("No data.")}
        </div>
      {:else}
        <div class="stat-cards" style="margin-top:16px">
          <div class="zf-card stat-card">
            <div class="stat-card-label">{$t("Total days")}</div>
            <div class="stat-card-value tab-num">{formatDayCount(absenceTotalDays)}</div>
          </div>
          {#each Object.entries(absenceByKind) as [kind, days]}
            <div class="zf-card stat-card">
              <div class="stat-card-label">{absenceKindLabel(kind)}</div>
              <div class="stat-card-value tab-num">{formatDayCount(days)}</div>
            </div>
          {/each}
        </div>

        <div class="zf-card" style="overflow-x:auto;margin-top:12px">
          <table class="zf-table">
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
                  <td class="tab-num" style="text-align:right"
                    >{fmtDate(a.start_date)}</td
                  >
                  <td class="tab-num" style="text-align:right"
                    >{fmtDate(a.end_date)}</td
                  >
                  <td class="tab-num" style="text-align:right">{formatDayCount(a.days)}</td>
                  <td
                    ><span class="zf-chip zf-chip-{a.status}"
                      >{statusLabel(a.status)}</span
                    ></td
                  >
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Card 6: timesheet export. -->
  <div class="zf-card" style="padding:20px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400"
        >{$t("Export timesheet")}</span
      >
      <button
        class="zf-btn-icon-sm zf-btn-ghost"
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

    <!-- Desktop layout: employee row first, then from/to row below.
         Mobile: everything stacked vertically. -->
    {#if !isSelfOnlyReportsView}
      <!-- First row: employee selection only -->
      <div style="margin-bottom:12px">
        <label class="zf-label" for="csv-user-id">{$t("Employee")}</label>
        <select id="csv-user-id" class="zf-select" bind:value={csvUserId}>
          {#each users as u}
            <option value={u.id}>{u.first_name} {u.last_name}</option>
          {/each}
        </select>
      </div>
    {/if}
    <!-- Second row: from / to -->
    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="zf-label" for="csv-from">{$t("From")}</label>
        <DatePicker id="csv-from" bind:value={csvFrom} max={csvTo} />
      </div>
      <div>
        <label class="zf-label" for="csv-to">{$t("To")}</label>
        <DatePicker
          id="csv-to"
          bind:value={csvTo}
          min={csvFrom}
          max={todayIso}
        />
      </div>
    </div>

    <div class="error-text">{csvError}</div>
    <div style="display:flex;gap:8px;flex-wrap:wrap">
      <button
        class="zf-btn zf-btn-primary"
        on:click={exportCsv}
        disabled={exportInProgress}
      >
        <Icon name="Download" size={14} />{$t("Export CSV")}
      </button>
      <button
        class="zf-btn zf-btn-primary"
        on:click={exportPdf}
        disabled={exportInProgress}
      >
        <Icon name="FileText" size={14} />{$t("Export PDF")}
      </button>
    </div>
  </div>
</div>

<style>
  /* Colour dot for the category legend */
  .cat-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    display: inline-block;
    flex-shrink: 0;
  }

</style>
