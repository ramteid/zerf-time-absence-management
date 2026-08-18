// Timesheet CSV building, extracted from the former Export section so the
// Reports toolbar can export "what you see" and the logic stays unit-testable.
import { minToHM } from "../../format.js";
import { absenceKindLabel, statusLabel } from "../../i18n.js";

// Cells starting with =, +, -, @, tab, CR, or with leading spaces then those, get a leading single-quote guard.
export function csvSafe(cellValue) {
  if (cellValue && /^ *[=+\-@\t\r]/.test(cellValue)) return "'" + cellValue;
  return cellValue;
}

// RFC 4180 encoding: quote any field containing a comma, quote, or newline.
export function csvEncode(fields) {
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

// Opening/closing flextime balance derived from the flextime day rows: the
// opening balance is the first day's cumulative minus that day's diff.
export function flextimeBounds(flextimeData) {
  if (!flextimeData || flextimeData.length === 0) {
    return { opening: null, closing: null };
  }
  return {
    opening: flextimeData[0].cumulative_min - flextimeData[0].diff_min,
    closing: flextimeData[flextimeData.length - 1].cumulative_min,
  };
}

// Builds the full CSV text (one row per entry, empty days included) from a
// range report plus optional flextime rows. `translate` is the current `$t`.
// `balanceAsOf` is the date the closing balance is stated as of (end of the
// last fully approved week); it gets its own row so the number cannot be
// mistaken for "as of the end of the export range".
export function buildTimesheetCsv({
  report,
  flextimeData,
  balanceAsOf = null,
  translate,
}) {
  const { opening, closing } = flextimeBounds(flextimeData);
  const header = csvEncode([
    translate("Date"),
    translate("Weekday"),
    translate("Start"),
    translate("End"),
    translate("Category"),
    translate("Duration"),
    translate("Status"),
    translate("Comment"),
    translate("Absence"),
    translate("Holiday"),
  ]);
  const rows = [header];
  for (const day of report.days) {
    const weekday = translate(day.weekday);
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
            csvSafe(translate(entry.category)),
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
  // Total row counts only approved work-crediting entries — mirrors the
  // backend's official balance rules.
  const totalMin = report.days.reduce(
    (sum, day) =>
      sum +
      (day.entries || []).reduce(
        (entrySum, entry) =>
          entrySum +
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
      translate("Total"),
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
        translate("Flextime opening balance"),
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
        translate("Flextime closing balance"),
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
    if (balanceAsOf) {
      const lastLedgerDay = flextimeData[flextimeData.length - 1].date;
      rows.push(
        csvEncode([
          "",
          translate("Flextime balance as of"),
          "",
          "",
          "",
          balanceAsOf < lastLedgerDay ? balanceAsOf : lastLedgerDay,
          "",
          "",
          "",
          "",
        ]),
      );
    }
  }
  return rows.join("\r\n");
}

// Turns arbitrary text (user names) into a safe file-name fragment.
export function safeFileNamePart(value, fallback = "report") {
  const cleaned = String(value || "")
    .trim()
    .replace(/[^A-Za-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || fallback;
}

// Triggers a browser download for a Blob via a transient anchor element.
export function downloadBlob(blob, fileName) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

// UTF-8 BOM so spreadsheet apps detect the encoding correctly.
export function timesheetCsvBlob(csvText) {
  return new Blob(["\uFEFF" + csvText], { type: "text/csv;charset=utf-8" });
}
