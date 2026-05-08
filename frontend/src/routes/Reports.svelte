<script>
  // ═══════════════════════════════════════════════════════════════════════════
  // Reports – Zentrale Seite für alle monatlichen und teambezogenen Statistiken.
  //
  // Anzeigereihenfolge der Kacheln:
  //   1. Überstundenkonto  – Jahresübersicht des angemeldeten Benutzers
  //   2. Mitarbeiterbericht – Monatsdetails (Mitarbeiterdetails + Monatsbericht)
  //   3. Teambericht       – Teamübersicht (nur Teamleitungen / Admins)
  //   4. Kategorieauswertung
  //   5. Abwesenheiten
  //   6. Export Stundennachweis
  //
  // Allgemeine Regeln:
  //   - Der aktuelle Tag wird in keiner Stundenberechnung berücksichtigt.
  //   - Die Zeiterfassung ist wochenbasiert, Auswertungen monatsbasiert.
  //     Grenzwochen (Mo-So überspannen Monatswechsel) zählen tagesweise
  //     zu den jeweiligen Monaten; für die Einreichungsprüfung zählen sie
  //     zu BEIDEN Monaten.
  // ═══════════════════════════════════════════════════════════════════════════

  import { api } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { t, absenceKindLabel, statusLabel, formatHours } from "../i18n.js";
  import { isoDate, minToHM, fmtDate, fmtMonthLabel } from "../format.js";
  import { normalizeMonthReport, countWorkdays, holidayDateSet } from "../apiMappers.js";
  import Icon from "../Icons.svelte";
  import DatePicker from "../DatePicker.svelte";
  import { jsPDF } from "jspdf";

  // ── Festes Datums-Referenzobjekt für diese Sitzung ──────────────────────
  // Einmalig bei Komponenteninitialisierung gesetzt, um Drift während der
  // Sitzung zu vermeiden.
  const today = new Date();
  const currentYear = today.getFullYear();
  const currentMonthStr = `${currentYear}-${String(today.getMonth() + 1).padStart(2, "0")}`;

  // ── Benutzerliste ────────────────────────────────────────────────────────
  // Teamleitungen und Admins laden alle Benutzer für das Mitarbeiter-Dropdown.
  // Reine Mitarbeiter (role === "employee") sehen kein Dropdown – nur die
  // eigenen Daten.
  let users = [];
  async function initUsers() {
    try {
      users = $currentUser.role === "employee"
        ? [$currentUser]
        : await api("/users");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  initUsers();

  // ── Hilfe-Tooltips ───────────────────────────────────────────────────────
  let activeHelp = null;
  function toggleHelp(id) {
    activeHelp = activeHelp === id ? null : id;
  }

  // ═══════════════════════════════════════════════════════════════════════════
  // ABSCHNITT 1 – Überstundenkonto (laufendes Jahr des angemeldeten Benutzers)
  //
  // Wird einmalig beim Seitenaufruf geladen und zeigt für jeden Monat:
  //   • Soll: Summe aller Soll-Minuten (Arbeitstage ohne Feiertage/Abwesenheiten,
  //           bis gestern)
  //   • IST: Summe aller genehmigten Zeiteinträge bis gestern
  //   • Diff: IST - Soll (negativ = rot, positiv = grün)
  //   • Kumuliert: laufendes Gleitzeitguthaben inklusive Überstunden-Startsaldo
  //
  // Monatsbezeichnung: Das Backend liefert "YYYY-MM" – auf dem Frontend wird
  // daraus via fmtMonthLabel() ein lesbarer Monatsname ("Mai 2026" o.ä.).
  // ═══════════════════════════════════════════════════════════════════════════
  let overtime = [];
  // Der letzte Eintrag enthält den aktuellsten kumulierten Saldo (Stand: gestern).
  $: cumulativeBalance = overtime.length > 0
    ? overtime[overtime.length - 1].cumulative_min
    : 0;

  async function loadOvertime() {
    try {
      overtime = await api(`/reports/overtime?year=${currentYear}`);
    } catch (e) {
      toast($t(e?.message || "Overtime data unavailable."), "error");
    }
  }
  loadOvertime();

  // ═══════════════════════════════════════════════════════════════════════════
  // ABSCHNITT 2 – Mitarbeiterbericht (Monatsbericht + Mitarbeiterdetails)
  //
  // Kombiniert die bisherigen getrennten Kacheln "Mitarbeiterdetails" und
  // "Monatsbericht" in einer gemeinsamen Kachel.
  //
  // Für Teamleitungen/Admins: Mitarbeiter-Dropdown sichtbar.
  // Für Mitarbeiter: kein Dropdown – automatisch eigene Daten.
  //
  // Nach "Anzeigen" werden geladen:
  //   • Monatsbericht (Soll/IST/Diff + Einträge + Abwesenheiten)
  //   • Überstunden des Jahres (für kumulierten Kontostand)
  //   • Urlaubsstand des Jahres
  // ═══════════════════════════════════════════════════════════════════════════
  let reportUserId = $currentUser.id;
  let reportMonth = currentMonthStr;
  // reportData enthält nach dem Laden alle nötigen Informationen.
  let reportData = null;

  async function loadReport() {
    try {
      const reportYear = reportMonth.slice(0, 4);
      const [monthRaw, overtimeRows, leaveRaw] = await Promise.all([
        api(`/reports/month?user_id=${reportUserId}&month=${reportMonth}`),
        // reportYear verwenden, damit der Kontostand am Ende des gewählten Monats
        // angezeigt wird, nicht der Stand am Ende des laufenden Jahres.
        api(`/reports/overtime?user_id=${reportUserId}&year=${reportYear}`).catch(() => []),
        api(`/leave-balance/${reportUserId}?year=${reportYear}`).catch(() => null),
      ]);

      const monthReport = normalizeMonthReport(monthRaw);

      // Monatsstatus aus den Einträgen ableiten.
      const nonDraft = (monthReport.entries || []).filter(e => e.status !== "draft");
      const monthStatus = (() => {
        if (nonDraft.length === 0) return "draft";
        if (nonDraft.every(e => e.status === "approved")) return "approved";
        if (nonDraft.some(e => e.status === "submitted")) return "submitted";
        if (nonDraft.every(e => e.status === "rejected")) return "rejected";
        return "partial";
      })();

      // Kumulierten Kontostand am Ende des gewählten Monats suchen.
      // Fallback auf letzten verfügbaren Eintrag (z.B. bei Zukunftsmonat).
      const reportMonthRow = overtimeRows.find(r => r.month === reportMonth);
      const fallbackRow = overtimeRows.length > 0 ? overtimeRows[overtimeRows.length - 1] : null;

      reportData = {
        monthReport,
        monthStatus,
        cumulativeOvertimeMin: (reportMonthRow ?? fallbackRow)?.cumulative_min ?? 0,
        leaveBalance: leaveRaw,
      };
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  // Abwesenheitszusammenfassung für Kacheln: { vacation: 2, sick: 1, ... }
  $: reportAbsenceSummary = (() => {
    if (!reportData) return {};
    const map = {};
    for (const a of reportData.monthReport.absences || []) {
      map[a.kind] = (map[a.kind] || 0) + (a.days || 0);
    }
    return map;
  })();

  // Farbe für den Monatsstatus-Badge.
  $: reportStatusColor = (() => {
    switch (reportData?.monthStatus) {
      case "approved":  return "var(--success-text)";
      case "submitted": return "var(--success-text)";
      case "rejected":  return "var(--danger-text)";
      case "partial":   return "var(--warning-text)";
      default:          return "var(--danger-text)";
    }
  })();

  // ═══════════════════════════════════════════════════════════════════════════
  // ABSCHNITT 3 – Teambericht (Teamübersicht pro Mitarbeiter)
  //
  // Nur für Teamleitungen und Admins sichtbar.
  // Spalten: Gleitzeitkonto, Monatsdiff, Krankheitstage, Urlaub genommen/geplant,
  //          Alle Wochen eingereicht.
  // Bei laufendem Monat: alle Werte relativ zu Arbeitstagen vom 1. bis gestern.
  // ═══════════════════════════════════════════════════════════════════════════
  let teamMonth = currentMonthStr;
  let teamReport = null;

  async function showTeam() {
    try {
      teamReport = await api(`/reports/team?month=${teamMonth}`);
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  // ═══════════════════════════════════════════════════════════════════════════
  // ABSCHNITT 4 – Kategorieauswertung
  //
  // Mitarbeiter: eigene Buchungen als Rangliste.
  // Teamleitungen/Admins: Matrix Mitarbeiter × Kategorie.
  //
  // Hinweis: Das Backend schließt nur "rejected"-Einträge aus, sodass auch
  // eingereichte (noch nicht genehmigte) Buchungen erscheinen.
  // ═══════════════════════════════════════════════════════════════════════════
  let catFrom = isoDate(new Date(currentYear, 0, 1));
  let catTo = isoDate(today);
  let catReport = null;
  let teamCatReport = null;
  let catFilteredCategories = [];
  let catShowFilter = false;

  async function showCat() {
    if (catFrom > catTo) return;
    try {
      const params = new URLSearchParams({ from: catFrom, to: catTo });
      if ($currentUser.role === "employee") {
        // Mitarbeiter sehen nur ihre eigene Auswertung.
        params.set("user_id", $currentUser.id);
        catReport = await api(`/reports/categories?${params}`);
        teamCatReport = null;
      } else {
        // Teamleitungen / Admins sehen die Teammatrix.
        teamCatReport = await api(`/reports/team-categories?${params}`);
        catReport = null;
      }
      catFilteredCategories = [];
      catShowFilter = false;
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  function toggleCategoryFilter(categoryName) {
    catFilteredCategories = catFilteredCategories.includes(categoryName)
      ? catFilteredCategories.filter(c => c !== categoryName)
      : [...catFilteredCategories, categoryName];
  }

  // Gefilterte Mitarbeiterliste (wendet aktiven Kategoriefilter an).
  $: filteredCatReport = catFilteredCategories.length === 0
    ? catReport
    : (catReport || []).filter(c => catFilteredCategories.includes(c.category));
  $: filteredCatTotal = (filteredCatReport || []).reduce((s, x) => s + x.minutes, 0);

  // Spalten für die Teammatrix (absteigend nach Gesamtminuten sortiert).
  $: allTeamCatColumns = (() => {
    if (!teamCatReport) return [];
    const totals = new Map();
    for (const row of teamCatReport) {
      for (const c of row.categories) {
        const entry = totals.get(c.category) || { color: c.color, total: 0 };
        entry.total += c.minutes;
        totals.set(c.category, entry);
      }
    }
    return [...totals.entries()]
      .sort((a, b) => b[1].total - a[1].total)
      .map(([category, { color }]) => ({ category, color }));
  })();
  $: visibleTeamCatColumns = catFilteredCategories.length === 0
    ? allTeamCatColumns
    : allTeamCatColumns.filter(c => catFilteredCategories.includes(c.category));

  function teamCatMinutes(row, category) {
    const c = row.categories.find(x => x.category === category);
    return c ? c.minutes : 0;
  }
  function teamCatRowTotal(row) {
    return row.categories.reduce((s, c) =>
      catFilteredCategories.length === 0 || catFilteredCategories.includes(c.category)
        ? s + c.minutes : s, 0);
  }

  // ═══════════════════════════════════════════════════════════════════════════
  // ABSCHNITT 5 – Abwesenheiten
  //
  // Zeigt Abwesenheitseinträge im gewählten Zeitraum mit Typenverteilung.
  // Mitarbeiter laden nur eigene Abwesenheiten; Leitungen/Admins alle.
  // ═══════════════════════════════════════════════════════════════════════════
  let absenceFrom = isoDate(new Date(currentYear, today.getMonth(), 1));
  let absenceTo = isoDate(new Date(currentYear, 11, 31));
  let absenceReport = null;
  $: absenceTotalDays = (absenceReport || []).reduce((s, x) => s + (x.days || 0), 0);
  $: absenceByKind = (absenceReport || []).reduce((map, x) => {
    const k = x.kind || "unknown";
    map[k] = (map[k] || 0) + (x.days || 0);
    return map;
  }, {});
  $: isLeadView = $currentUser.role !== "employee";
  let absenceHolidayDates = new Set();

  // Kürzt den Abwesenheitszeitraum auf das gewählte Von-Bis-Fenster.
  function clampAbsenceRange(absence) {
    if (!absence?.start_date || !absence?.end_date) return null;
    const from = absence.start_date > absenceFrom ? absence.start_date : absenceFrom;
    const to = absence.end_date < absenceTo ? absence.end_date : absenceTo;
    if (to < from) return null;
    return { from, to };
  }

  function absenceDays(a) {
    const clamped = clampAbsenceRange(a);
    if (!clamped) return 0;
    return countWorkdays(clamped.from, clamped.to, absenceHolidayDates);
  }

  async function showAbsences() {
    if (absenceFrom > absenceTo) return;
    try {
      let raw;
      if ($currentUser.role === "employee") {
        // Eigene Abwesenheiten: das API ist jahresbasiert, daher bei
        // jahresübergreifenden Bereichen mehrere Abrufe + Deduplizierung.
        const fromYear = parseInt(absenceFrom.slice(0, 4), 10);
        const toYear = parseInt(absenceTo.slice(0, 4), 10);
        const years = Array.from({ length: toYear - fromYear + 1 }, (_, i) => fromYear + i);
        const lists = await Promise.all(years.map(y => api(`/absences?year=${y}`)));
        const seen = new Set();
        raw = lists.flat().filter(a => {
          if (seen.has(a.id)) return false;
          seen.add(a.id);
          return a.end_date >= absenceFrom && a.start_date <= absenceTo;
        });
      } else {
        const params = new URLSearchParams({ from: absenceFrom, to: absenceTo });
        raw = await api(`/absences/all?${params}`);
      }
      // Feiertage für alle beteiligten Jahre laden (für korrekte Arbeitstage).
      const allYears = [...new Set(raw.flatMap(a => [
        parseInt(a.start_date.slice(0, 4), 10),
        parseInt(a.end_date.slice(0, 4), 10),
      ]))];
      const holidayLists = await Promise.all(allYears.map(y => api(`/holidays?year=${y}`)));
      absenceHolidayDates = holidayDateSet(holidayLists.flat());
      absenceReport = raw.map(a => ({ ...a, days: absenceDays(a) }));
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  // ═══════════════════════════════════════════════════════════════════════════
  // ABSCHNITT 6 – Export Stundennachweis (CSV / PDF)
  //
  // Teamleitungen/Admins können beliebigen Mitarbeiter wählen.
  // Mitarbeiter exportieren immer die eigenen Daten.
  // Desktop-Layout: Nach dem Mitarbeiter-Dropdown beginnt eine neue Zeile.
  // ═══════════════════════════════════════════════════════════════════════════
  let csvUserId = $currentUser.id;
  let csvFrom = isoDate(new Date(currentYear, today.getMonth(), 1));
  let csvTo = isoDate(today);
  let csvError = "";
  let exportInProgress = false;

  // CSV-Formel-Injektionsschutz: Zellen, die mit =, +, -, @ usw. beginnen,
  // werden mit einem einfachen Anführungszeichen vorangestellt.
  function csvSafe(s) {
    if (s && /^[=+\-@\t\r]/.test(s)) return "'" + s;
    return s;
  }

  // Kodiert ein Array von Feldern als eine CSV-Zeile (RFC 4180).
  function csvEncode(fields) {
    return fields.map(f => {
      const s = f == null ? "" : String(f);
      return s.includes(",") || s.includes('"') || s.includes("\n")
        ? '"' + s.replace(/"/g, '""') + '"'
        : s;
    }).join(",");
  }

  // Erstellt einen temporären <a>-Link und löst den Browser-Download aus.
  function downloadBlob(blob, fileName) {
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = fileName;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  async function exportCsv() {
    if (exportInProgress) return;
    csvError = "";
    if (!csvFrom || !csvTo) { csvError = $t("Invalid date."); return; }
    if (csvFrom > csvTo) { csvError = $t("From cannot be after To."); return; }
    exportInProgress = true;
    try {
      const params = new URLSearchParams({
        user_id: String(csvUserId),
        from: csvFrom,
        to: csvTo,
      });
      const report = await api(`/reports/range?${params}`);
      const header = csvEncode([
        $t("Date"), $t("Weekday"), $t("Start"), $t("End"),
        $t("Category"), $t("Duration"), $t("Status"), $t("Comment"),
        $t("Absence"), $t("Holiday"),
      ]);
      const rows = [header];
      for (const day of report.days) {
        const weekday = $t(day.weekday);
        const absence = day.absence ? absenceKindLabel(day.absence) : "";
        const holiday = day.holiday || "";
        if (!day.entries || day.entries.length === 0) {
          rows.push(csvEncode([
            day.date, weekday, "", "", "", "0:00", "", "",
            csvSafe(absence), csvSafe(holiday),
          ]));
        } else {
          for (const e of day.entries) {
            rows.push(csvEncode([
              day.date, weekday, e.start_time, e.end_time,
              csvSafe($t(e.category)), minToHM(e.minutes || 0), statusLabel(e.status),
              csvSafe(e.comment || ""), csvSafe(absence), csvSafe(holiday),
            ]));
          }
        }
      }
      const totalMin = report.days.reduce(
        (s, d) => s + (d.entries || []).reduce((es, e) => es + (e.minutes || 0), 0), 0
      );
      rows.push(csvEncode(["", $t("Total"), "", "", "", minToHM(totalMin), "", "", "", ""]));
      const blob = new Blob(["﻿" + rows.join("\n")], { type: "text/csv;charset=utf-8" });
      downloadBlob(blob, `stundennachweis-${csvUserId}-${csvFrom}_${csvTo}.csv`);
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
    if (!csvFrom || !csvTo) { csvError = $t("Invalid date."); return; }
    if (csvFrom > csvTo) { csvError = $t("From cannot be after To."); return; }
    exportInProgress = true;
    try {
      const params = new URLSearchParams({ user_id: String(csvUserId), from: csvFrom, to: csvTo });
      const report = await api(`/reports/range?${params}`);
      const user = users.find(u => u.id === csvUserId);
      const fullName = user ? `${user.first_name} ${user.last_name}` : String(csvUserId);

      const doc = new jsPDF({ unit: "mm", format: "a4" });
      const PH = 297, ML = 15, MT = 15, CW = 180;
      const rowH = 5.5, hdrH = 7;
      let y = MT;

      // Spaltendefinitionen (Summe = 180 mm).
      // "Holiday" braucht 33 mm für lange Namen wie "Christi Himmelfahrt".
      const cols = [
        [$t("Date"),     22, "left"],
        [$t("Weekday"),  20, "left"],
        [$t("Start"),    12, "center"],
        [$t("End"),      12, "center"],
        [$t("Category"), 40, "left"],
        [$t("Duration"), 16, "right"],
        [$t("Absence"),  25, "left"],
        [$t("Holiday"),  33, "left"],
      ]; // 22+20+12+12+40+16+25+33 = 180

      function colX(i) {
        let x = ML;
        for (let j = 0; j < i; j++) x += cols[j][1];
        return x;
      }
      function textX(i) {
        const [, w, align] = cols[i];
        if (align === "right")  return colX(i) + w - 1;
        if (align === "center") return colX(i) + w / 2;
        return colX(i) + 1;
      }
      function drawHeader() {
        doc.setFillColor(235, 235, 235);
        doc.rect(ML, y, CW, hdrH, "F");
        doc.setFont("helvetica", "bold");
        doc.setFontSize(8);
        doc.setTextColor(50, 50, 50);
        cols.forEach(([label,, align], i) =>
          doc.text(label, textX(i), y + 4.8, { align })
        );
        y += hdrH;
      }
      function drawRow(cells, shade) {
        if (y + rowH > PH - 15) { doc.addPage(); y = MT; drawHeader(); }
        if (shade) {
          doc.setFillColor(248, 248, 248);
          doc.rect(ML, y, CW, rowH, "F");
        }
        doc.setFont("helvetica", "normal");
        doc.setFontSize(7.5);
        doc.setTextColor(30, 30, 30);
        cells.forEach(([text, i]) => {
          const [,, align] = cols[i];
          doc.text(String(text ?? ""), textX(i), y + 3.8, { align });
        });
        doc.setDrawColor(220, 220, 220);
        doc.line(ML, y + rowH, ML + CW, y + rowH);
        y += rowH;
      }

      // Titelblock
      doc.setFont("helvetica", "bold");
      doc.setFontSize(13);
      doc.setTextColor(20, 20, 20);
      doc.text($t("Timesheet"), ML, y + 6);
      doc.setFont("helvetica", "normal");
      doc.setFontSize(9);
      doc.setTextColor(90, 90, 90);
      doc.text(`${fullName}  ·  ${csvFrom} – ${csvTo}`, ML, y + 12);
      y += 20;
      drawHeader();

      let rowIdx = 0;
      for (const day of report.days) {
        const absence = day.absence ? absenceKindLabel(day.absence) : "";
        const holiday = day.holiday || "";
        const weekday = $t(day.weekday);
        if (!day.entries || day.entries.length === 0) {
          drawRow(
            [[day.date,0],[weekday,1],["",2],["",3],["",4],["0:00",5],[absence,6],[holiday,7]],
            rowIdx % 2 === 1
          );
          rowIdx++;
        } else {
          for (const e of day.entries) {
            drawRow([
              [day.date,0],[weekday,1],
              [e.start_time?.slice(0,5)??"",2],[e.end_time?.slice(0,5)??"",3],
              [$t(e.category??""),4],[minToHM(e.minutes||0),5],
              [absence,6],[holiday,7],
            ], rowIdx % 2 === 1);
            rowIdx++;
          }
        }
      }

      // Gesamtzeile
      if (y + rowH > PH - 15) { doc.addPage(); y = MT; drawHeader(); }
      doc.setFillColor(235, 235, 235);
      doc.rect(ML, y, CW, rowH, "F");
      doc.setFont("helvetica", "bold");
      doc.setFontSize(7.5);
      doc.setTextColor(20, 20, 20);
      doc.text($t("Total"), ML + 1, y + 3.8);
      const pdfTotalMin = report.days.reduce(
        (s, d) => s + (d.entries || []).reduce((es, e) => es + (e.minutes || 0), 0), 0
      );
      doc.text(minToHM(pdfTotalMin), textX(5), y + 3.8, { align: "right" });

      doc.save(`stundennachweis-${fullName.replace(/\s+/g, "-")}-${csvFrom}_${csvTo}.pdf`);
      toast($t("PDF download started."), "ok");
    } catch (e) {
      csvError = $t(e?.message || "Export failed.");
    } finally {
      exportInProgress = false;
    }
  }
</script>

<!-- ═══════════════════════════════════════════════════════════════════════════
     SEITENKOPF
     ═══════════════════════════════════════════════════════════════════════════ -->
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

  <!-- ═══════════════════════════════════════════════════════════════════════
       KACHEL 1 – Überstundenkonto (laufendes Jahr, immer ganz oben)
       Zeigt den angemeldeten Benutzer. Tabelle: ein Eintrag pro Monat.
       ═══════════════════════════════════════════════════════════════════════ -->
  <div class="kz-card overtime-card" style="margin-bottom:16px">
    <div class="card-header">
      <span class="card-header-title" style="display:inline-flex;align-items:center;gap:8px">
        {$t("Overtime balance {year}", { year: currentYear })}
        <button
          class="kz-btn-icon-sm kz-btn-ghost"
          title={$t("help_overtime")}
          on:click={() => toggleHelp("overtime")}
          style="color:var(--text-tertiary);font-size:14px;cursor:help"
        >
          <Icon name="Info" size={14} />
        </button>
      </span>
      <!-- Aktueller Gesamtkontostand als farbiger Badge -->
      <span
        class="kz-chip"
        class:kz-chip-approved={cumulativeBalance >= 0}
        class:kz-chip-rejected={cumulativeBalance < 0}
      >
        {minToHM(cumulativeBalance)}
      </span>
    </div>

    {#if activeHelp === "overtime"}
      <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)">
        {$t("help_overtime")}
      </div>
    {/if}

    <!-- Desktop-Tabelle: eine Zeile pro Monat -->
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
          {#each overtime as m}
            {@const cum = m.cumulative_min}
            <tr>
              <!-- Monatsname statt "YYYY-MM" -->
              <td class="tab-num">{fmtMonthLabel(m.month)}</td>
              <td class="tab-num">{minToHM(m.target_min)}</td>
              <td class="tab-num">{minToHM(m.actual_min)}</td>
              <!-- Diff: rot bei Minusstunden, grün bei Ausgeglichen oder Überstunden -->
              <td
                class="tab-num"
                style="color:{m.diff_min < 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
              >
                {m.diff_min >= 0 ? "+" : ""}{minToHM(m.diff_min)}
              </td>
              <!-- Kumuliert: rot bei Defizit, grün bei Ausgeglichen oder Guthaben -->
              <td
                class="tab-num"
                style="color:{cum < 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
              >
                {cum >= 0 ? "+" : ""}{minToHM(cum)}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <!-- Mobile-Kacheln: eine Kachel pro Monat -->
    <div class="overtime-tiles-mobile">
      {#each overtime as m}
        {@const cum = m.cumulative_min}
        <div class="overtime-tile">
          <div style="font-weight:400;font-size:13px;margin-bottom:4px">
            {fmtMonthLabel(m.month)}
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Target")}</span>
            <span class="tab-num">{minToHM(m.target_min)}</span>
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Actual")}</span>
            <span class="tab-num">{minToHM(m.actual_min)}</span>
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Diff")}</span>
            <span class="tab-num" style="color:{m.diff_min < 0 ? 'var(--danger-text)' : 'var(--success-text)'}">
              {m.diff_min >= 0 ? "+" : ""}{minToHM(m.diff_min)}
            </span>
          </div>
          <div class="overtime-tile-row">
            <span>{$t("Cumulative")}</span>
            <span class="tab-num" style="color:{cum < 0 ? 'var(--danger-text)' : 'var(--success-text)'}">
              {cum >= 0 ? "+" : ""}{minToHM(cum)}
            </span>
          </div>
        </div>
      {/each}
    </div>
  </div>

  <!-- ═══════════════════════════════════════════════════════════════════════
       KACHEL 2 – Mitarbeiterbericht
       Kombiniert die bisherigen Kacheln "Mitarbeiterdetails" (Dialog) und
       "Monatsbericht" (Inline) in einer gemeinsamen Inline-Kachel.

       Mitarbeiter-Rolle: kein Dropdown, automatisch eigene Daten.
       Teamleitung / Admin: Dropdown zur Auswahl des Mitarbeiters.
       ═══════════════════════════════════════════════════════════════════════ -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400">{$t("Employee report")}</span>
      <button
        class="kz-btn-icon-sm kz-btn-ghost"
        title={$t("help_employee_details")}
        on:click={() => toggleHelp("report")}
        style="color:var(--text-tertiary);font-size:14px;cursor:help"
      >
        <Icon name="Info" size={14} />
      </button>
    </div>

    {#if activeHelp === "report"}
      <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)">
        {$t("View detailed information about a team member including balance and statistics.")}
      </div>
    {/if}

    <!-- Steuerungszeile: Mitarbeiter-Dropdown (nur für Leitungen/Admins) + Monatsauswahl -->
    <div class="field-row" style="margin-bottom:12px">
      {#if $currentUser.role !== "employee"}
        <!-- Teamleitungen und Admins können jeden Mitarbeiter wählen. -->
        <div>
          <label class="kz-label" for="report-user-id">{$t("Employee")}</label>
          <select id="report-user-id" class="kz-select" bind:value={reportUserId}>
            {#each users as u}
              <option value={u.id}>{u.first_name} {u.last_name}</option>
            {/each}
          </select>
        </div>
      {/if}
      <div>
        <label class="kz-label" for="report-month">{$t("Month")}</label>
        <DatePicker id="report-month" mode="month" bind:value={reportMonth} />
      </div>
    </div>

    <button class="kz-btn kz-btn-primary" on:click={loadReport}>{$t("Show")}</button>

    {#if reportData}
      <!-- ── Zusammenfassungs-Kacheln (Dashboard-Kacheln des Mitarbeiters) ── -->
      <div style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-top:20px;margin-bottom:6px">
        {$t("My Balance")}
      </div>
      <div class="stat-cards" style="margin-bottom:16px">

        <!-- Erfasste vs. Soll-Stunden im Monat -->
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Logged")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{reportData.monthReport.actual_min >= reportData.monthReport.target_min ? 'var(--accent)' : 'var(--warning-text)'}"
          >
            {formatHours(((reportData.monthReport.actual_min || 0) / 60).toFixed(1))}
          </div>
          <div class="stat-card-sub">
            {$t("of {target} target", { target: formatHours(((reportData.monthReport.target_min || 0) / 60).toFixed(1)) })}
          </div>
        </div>

        <!-- Überstunden / Minusstunden dieses Monats -->
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Monthly diff")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{reportData.monthReport.diff_min < 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >
            {reportData.monthReport.diff_min >= 0 ? "+" : ""}
            {minToHM(reportData.monthReport.diff_min)}
          </div>
        </div>

        <!-- Kumulierter Gleitzeitkontostand (laufendes Jahr, bis gestern) -->
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Overtime overview")}</div>
          <div
            class="stat-card-value tab-num"
            style="color:{reportData.cumulativeOvertimeMin < 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
          >
            {formatHours(((reportData.cumulativeOvertimeMin || 0) / 60).toFixed(1))}
          </div>
        </div>

        <!-- Monatsstatus (Entwurf / Eingereicht / Genehmigt / ...) -->
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Status")}</div>
          <div
            class="stat-card-value tab-num"
            style="font-size:18px;color:{reportStatusColor}"
          >
            {statusLabel(reportData.monthStatus)}
          </div>
        </div>
      </div>

      <!-- ── Urlaubsstand (falls verfügbar) ─────────────────────────────── -->
      {#if reportData.leaveBalance}
        <div style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-bottom:6px">
          {$t("Vacation")}
        </div>
        <div class="stat-cards" style="margin-bottom:16px">
          <div class="kz-card stat-card">
            <div class="stat-card-label">{$t("Entitlement")}</div>
            <div class="stat-card-value tab-num">{reportData.leaveBalance.annual_entitlement}</div>
          </div>
          <div class="kz-card stat-card">
            <div class="stat-card-label">{$t("Taken")}</div>
            <div class="stat-card-value tab-num">{reportData.leaveBalance.already_taken}</div>
          </div>
          {#if reportData.leaveBalance.approved_upcoming > 0}
            <div class="kz-card stat-card">
              <div class="stat-card-label">{$t("Planned")}</div>
              <div class="stat-card-value tab-num">{reportData.leaveBalance.approved_upcoming}</div>
            </div>
          {/if}
          {#if reportData.leaveBalance.requested > 0}
            <div class="kz-card stat-card">
              <div class="stat-card-label">{$t("Requested")}</div>
              <div class="stat-card-value tab-num">{reportData.leaveBalance.requested}</div>
            </div>
          {/if}
          <div class="kz-card stat-card">
            <div class="stat-card-label">{$t("Remaining")}</div>
            <div
              class="stat-card-value tab-num"
              style="color:{reportData.leaveBalance.available < 0 ? 'var(--danger-text)' : 'var(--success-text)'}"
            >
              {reportData.leaveBalance.available}
            </div>
          </div>
        </div>
      {/if}

      <!-- ── Abwesenheits-Kacheln für den gewählten Monat ─────────────────── -->
      {#if Object.keys(reportAbsenceSummary).length > 0}
        <div style="font-size:12px;font-weight:400;color:var(--text-tertiary);text-transform:uppercase;letter-spacing:.05em;margin-bottom:6px">
          {$t("Absences")}
        </div>
        <div class="stat-cards" style="margin-bottom:16px">
          {#each Object.entries(reportAbsenceSummary) as [kind, days]}
            <div class="kz-card stat-card">
              <div class="stat-card-label">{absenceKindLabel(kind)}</div>
              <div class="stat-card-value tab-num">{days}</div>
              <div class="stat-card-sub">{$t("days")}</div>
            </div>
          {/each}
        </div>
      {/if}

      <!-- ── Kategoriebuchhaltung als Balkendiagramm ──────────────────────── -->
      {#if reportData.monthReport.category_totals && Object.keys(reportData.monthReport.category_totals).length > 0}
        {@const catEntries = Object.entries(reportData.monthReport.category_totals).sort((a, b) => b[1] - a[1])}
        {@const catMax = catEntries[0][1]}
        <div class="kz-card" style="padding:16px;margin-bottom:12px">
          <div style="font-weight:400;margin-bottom:12px">{$t("Category breakdown")}</div>
          <div style="display:flex;flex-direction:column;gap:8px">
            {#each catEntries as [cat, mins]}
              <div style="display:grid;grid-template-columns:130px 1fr 52px;align-items:center;gap:8px;font-size:12px">
                <span style="font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title={$t(cat)}>
                  {$t(cat)}
                </span>
                <div style="background:var(--bg-muted);border-radius:3px;height:8px;overflow:hidden">
                  <div style="height:100%;border-radius:3px;background:var(--accent);width:{catMax > 0 ? Math.round((mins / catMax) * 100) : 0}%;transition:width .3s"></div>
                </div>
                <span class="tab-num" style="color:var(--text-tertiary);text-align:right">{minToHM(mins)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- ── Zeiteinträge-Tabelle ────────────────────────────────────────── -->
      {#if reportData.monthReport.entries?.length}
        <div class="kz-card" style="overflow-x:auto;margin-bottom:12px">
          <div style="font-weight:400;padding:16px 16px 12px">{$t("Entries")}</div>
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
              {#each reportData.monthReport.entries as e}
                <tr class:entry-rejected={e.status === "rejected"}>
                  <td class="tab-num">{fmtDate(e.entry_date)}</td>
                  <td class="tab-num">{e.start_time?.slice(0, 5)}</td>
                  <td class="tab-num">{e.end_time?.slice(0, 5)}</td>
                  <td class="tab-num">{minToHM(e.minutes || 0)}</td>
                  <td>{e.category_name ? $t(e.category_name) : "–"}</td>
                  <td>
                    <span class="kz-chip kz-chip-{e.status}">{statusLabel(e.status)}</span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

      <!-- ── Abwesenheiten-Tabelle ───────────────────────────────────────── -->
      {#if reportData.monthReport.absences?.length}
        <div class="kz-card" style="overflow-x:auto">
          <div style="font-weight:400;padding:16px 16px 12px">{$t("Absences")}</div>
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
              {#each reportData.monthReport.absences as a}
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

  <!-- ═══════════════════════════════════════════════════════════════════════
       KACHEL 3 – Teambericht (nur Teamleitungen / Admins)
       Spalten: Gleitzeitkonto | Monatsdiff | Krank | Urlaub gen. | Urlaub gep. | Wochen
       Bei laufendem Monat: Hinweistext, dass Daten nur bis gestern reichen.
       ═══════════════════════════════════════════════════════════════════════ -->
  {#if $currentUser.permissions?.can_view_team_reports}
    <div class="kz-card" style="padding:20px;margin-bottom:16px">
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
        <span style="font-size:14px;font-weight:400">{$t("Team report")}</span>
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
        <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)">
          {$t("help_team_report")}
        </div>
      {/if}

      <div style="display:flex;gap:12px;align-items:flex-end;margin-bottom:12px;flex-wrap:wrap">
        <div style="flex:1">
          <label class="kz-label" for="team-month">{$t("Month")}</label>
          <DatePicker id="team-month" mode="month" bind:value={teamMonth} />
        </div>
        <button class="kz-btn kz-btn-primary" on:click={showTeam}>{$t("Show")}</button>
      </div>

      {#if teamReport}
        <!-- Hinweis bei laufendem Monat: Daten nur bis gestern -->
        {#if teamMonth === currentMonthStr}
          <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:10px;padding:6px 10px;background:var(--bg-muted);border-radius:var(--radius-sm)">
            {$t("Note: current month – data up to yesterday")}
          </div>
        {/if}

        <!-- Scrollbare Tabelle mit allen neuen Spalten -->
        <div class="kz-table-wrap">
          <table class="kz-table kz-table--fit">
            <thead>
              <tr>
                <!-- Name -->
                <th style="min-width:120px">{$t("Employee")}</th>
                <!-- Aktueller Gleitzeitkontostand -->
                <th style="text-align:right;white-space:nowrap">{$t("Current flextime balance")}</th>
                <!-- Monatsdifferenz (Überstunden / Minusstunden) -->
                <th style="text-align:right;white-space:nowrap">{$t("Monthly diff")}</th>
                <!-- Krankheitstage -->
                <th style="text-align:right;white-space:nowrap">{$t("Sick days")}</th>
                <!-- Genommene Urlaubstage -->
                <th style="text-align:right;white-space:nowrap">{$t("Vacation taken")}</th>
                <!-- Geplante Urlaubstage -->
                <th style="text-align:right;white-space:nowrap">{$t("Vacation planned")}</th>
                <!-- Alle vergangenen Wochen eingereicht? -->
                <th style="text-align:center;white-space:nowrap">{$t("All weeks submitted")}</th>
              </tr>
            </thead>
            <tbody>
              {#each teamReport as r}
                <tr>
                  <td style="font-weight:500">{r.name}</td>
                  <!-- Gleitzeitkonto: rot = Defizit, grün = ausgeglichen oder Guthaben -->
                  <td class="tab-num" style="text-align:right;font-weight:500;color:{r.flextime_balance_min < 0 ? 'var(--danger-text)' : 'var(--success-text)'}">
                    {r.flextime_balance_min >= 0 ? "+" : ""}{minToHM(r.flextime_balance_min)}
                  </td>
                  <!-- Monatsdiff -->
                  <td class="tab-num" style="text-align:right;color:{r.diff_min < 0 ? 'var(--danger-text)' : 'var(--success-text)'}">
                    {r.diff_min >= 0 ? "+" : ""}{minToHM(r.diff_min)}
                  </td>
                  <!-- Krankheitstage (Dezimalzahl, da Halbtage möglich) -->
                  <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                    {r.sick_days > 0 ? r.sick_days.toFixed(r.sick_days % 1 === 0 ? 0 : 1) : "–"}
                  </td>
                  <!-- Urlaub genommen -->
                  <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                    {r.vacation_days > 0 ? r.vacation_days.toFixed(r.vacation_days % 1 === 0 ? 0 : 1) : "–"}
                  </td>
                  <!-- Urlaub geplant -->
                  <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                    {r.vacation_planned_days > 0 ? r.vacation_planned_days.toFixed(r.vacation_planned_days % 1 === 0 ? 0 : 1) : "–"}
                  </td>
                  <!-- Alle Wochen eingereicht: grünes Häkchen oder rotes X -->
                  <td style="text-align:center">
                    {#if r.weeks_all_submitted}
                      <span style="color:var(--success-text)">✓</span>
                    {:else}
                      <span style="color:var(--danger-text)">✗</span>
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

  <!-- ═══════════════════════════════════════════════════════════════════════
       KACHEL 4 – Kategorieauswertung
       Mitarbeiter: eigene Buchungen (nicht abgelehnte Einträge).
       Teamleitungen/Admins: Matrix Mitarbeiter × Kategorie.
       Filter-Button erscheint nach dem ersten Laden mit Ergebnissen.
       ═══════════════════════════════════════════════════════════════════════ -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400">{$t("Category breakdown")}</span>
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
      <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)">
        {$t("help_category_breakdown")}
      </div>
    {/if}

    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="kz-label" for="cat-from">{$t("From")}</label>
        <DatePicker id="cat-from" bind:value={catFrom} max={catTo} />
      </div>
      <div>
        <label class="kz-label" for="cat-to">{$t("To")}</label>
        <DatePicker id="cat-to" bind:value={catTo} min={catFrom} />
      </div>
    </div>

    <div style="display:flex;gap:8px;margin-bottom:12px;flex-wrap:wrap">
      <button class="kz-btn kz-btn-primary" on:click={showCat}>{$t("Run")}</button>
      <!-- Filter-Button: nur sichtbar wenn Ergebnisse vorhanden -->
      {#if (catReport && catReport.length > 0) || (allTeamCatColumns.length > 0)}
        <button class="kz-btn" on:click={() => catShowFilter = !catShowFilter}>
          {$t("Filter")}
          {#if catFilteredCategories.length > 0}
            ({catFilteredCategories.length})
          {/if}
        </button>
      {/if}
    </div>

    <!-- Teamleitung/Admin-Filterbereich (Kategorien der Teammatrix) -->
    {#if catShowFilter && allTeamCatColumns.length > 0}
      <div style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px">
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          {#each allTeamCatColumns as col}
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

    <!-- Mitarbeiter-Filterbereich (eigene Kategorien) -->
    {#if catShowFilter && catReport && catReport.length > 0}
      <div style="padding:12px;background:var(--bg-muted);border-radius:var(--radius-sm);margin-bottom:12px">
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          {#each catReport as cat}
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

    <!-- Teammatrix (Teamleitung / Admin) -->
    {#if teamCatReport}
      {#if teamCatReport.length === 0 || visibleTeamCatColumns.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">{$t("No data.")}</div>
      {:else}
        <div class="kz-table-wrap" style="margin-top:12px">
          <table class="kz-table kz-table--fit">
            <thead>
              <tr>
                <th>{$t("Employee")}</th>
                {#each visibleTeamCatColumns as col}
                  <th style="text-align:right">
                    <span style="display:inline-flex;align-items:center;gap:4px;justify-content:flex-end">
                      <span class="cat-dot" style="background:{col.color || '#999'}"></span>
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
                    <td class="tab-num" style="text-align:right;color:var(--text-tertiary)">
                      {#if teamCatMinutes(row, col.category) > 0}
                        {minToHM(teamCatMinutes(row, col.category))}
                      {:else}
                        –
                      {/if}
                    </td>
                  {/each}
                  <td class="tab-num" style="text-align:right;font-weight:400">
                    {rowTotal > 0 ? minToHM(rowTotal) : "–"}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}

    <!-- Mitarbeiterliste (eigene Kategorien) -->
    {#if catReport}
      {#if catReport.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">{$t("No data.")}</div>
      {:else if filteredCatReport && filteredCatReport.length === 0 && catFilteredCategories.length > 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">{$t("No data.")}</div>
      {:else if filteredCatReport}
        <div class="kz-table-wrap" style="margin-top:12px">
          <table class="kz-table kz-table--fit" style="table-layout:fixed">
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
                    <span style="display:inline-flex;align-items:center;gap:6px">
                      <span class="cat-dot" style="background:{c.color || '#999'}"></span>
                      {$t(c.category)}
                    </span>
                  </td>
                  <td class="tab-num" style="text-align:right">{minToHM(c.minutes)}</td>
                  <td class="tab-num" style="text-align:right">
                    {filteredCatTotal > 0 ? ((c.minutes / filteredCatTotal) * 100).toFixed(1) : 0}%
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>

  <!-- ═══════════════════════════════════════════════════════════════════════
       KACHEL 5 – Abwesenheiten
       Zeigt Abwesenheitseinträge mit Typverteilung für einen wählbaren Zeitraum.
       ═══════════════════════════════════════════════════════════════════════ -->
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400">{$t("Absences")}</span>
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
      <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)">
        {$t("View absence entries over a selected period with type distribution.")}
      </div>
    {/if}

    <div class="field-row" style="margin-bottom:12px">
      <div>
        <label class="kz-label" for="absence-from">{$t("From")}</label>
        <DatePicker id="absence-from" bind:value={absenceFrom} max={absenceTo} />
      </div>
      <div>
        <label class="kz-label" for="absence-to">{$t("To")}</label>
        <DatePicker id="absence-to" bind:value={absenceTo} min={absenceFrom} />
      </div>
    </div>
    <button class="kz-btn kz-btn-primary" on:click={showAbsences}>{$t("Run")}</button>

    {#if absenceReport}
      {#if absenceReport.length === 0}
        <div style="padding:16px;color:var(--text-tertiary);font-size:13px">{$t("No data.")}</div>
      {:else}
        <div class="stat-cards" style="margin-top:16px">
          <div class="kz-card stat-card">
            <div class="stat-card-label">{$t("Total days")}</div>
            <div class="stat-card-value tab-num">{absenceTotalDays}</div>
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
                <tr class:entry-rejected={a.status === "rejected"}>
                  {#if isLeadView}
                    <td style="font-weight:500">
                      {absUser ? `${absUser.first_name} ${absUser.last_name}` : `#${a.user_id}`}
                    </td>
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

  <!-- ═══════════════════════════════════════════════════════════════════════
       KACHEL 6 – Export Stundennachweis (CSV / PDF)
       Desktop: Mitarbeiter-Dropdown in eigener Zeile, dann Von/Bis darunter.
       ═══════════════════════════════════════════════════════════════════════ -->
  <div class="kz-card" style="padding:20px">
    <div style="display:flex;align-items:center;gap:8px;margin-bottom:14px">
      <span style="font-size:14px;font-weight:400">{$t("Export timesheet")}</span>
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
      <div style="font-size:12px;color:var(--text-tertiary);margin-bottom:12px;padding:8px;background:var(--bg-muted);border-radius:var(--radius-sm)">
        {$t("help_csv_export")}
      </div>
    {/if}

    <!-- Desktop-Layout: Mitarbeiter steht in einer eigenen Zeile,
         dann Von/Bis-Zeile darunter. Mobile: alles untereinander. -->
    {#if $currentUser.role !== "employee"}
      <!-- Erste Zeile: nur Mitarbeiter-Auswahl -->
      <div style="margin-bottom:12px">
        <label class="kz-label" for="csv-user-id">{$t("Employee")}</label>
        <select id="csv-user-id" class="kz-select" bind:value={csvUserId}>
          {#each users as u}
            <option value={u.id}>{u.first_name} {u.last_name}</option>
          {/each}
        </select>
      </div>
    {/if}
    <!-- Zweite Zeile: Von / Bis -->
    <div class="field-row" style="margin-bottom:12px">
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
    <div style="display:flex;gap:8px;flex-wrap:wrap">
      <button class="kz-btn kz-btn-primary" on:click={exportCsv} disabled={exportInProgress}>
        <Icon name="Download" size={14} />{$t("Export CSV")}
      </button>
      <button class="kz-btn kz-btn-primary" on:click={exportPdf} disabled={exportInProgress}>
        <Icon name="FileText" size={14} />{$t("Export PDF")}
      </button>
    </div>
  </div>

</div>

<style>
  /* ── Überstundenkonto: Tabelle auf Desktop, Kacheln auf Mobile ────────── */
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

  /* Farbpunkt für Kategorie-Legende */
  .cat-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    display: inline-block;
    flex-shrink: 0;
  }

  /* Mobile: Tabelle durch gestapelte Kacheln ersetzen */
  @media (max-width: 640px) {
    .overtime-table-desktop {
      display: none;
    }
    .overtime-tiles-mobile {
      display: block;
    }
  }
</style>
