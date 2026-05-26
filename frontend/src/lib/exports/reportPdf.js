import { jsPDF } from "jspdf";
import { minToHM } from "../../format.js";
import { absenceKindLabel } from "../../i18n.js";

const PAGE_HEIGHT = 297;
const MARGIN_LEFT = 15;
const MARGIN_TOP = 15;
const CONTENT_WIDTH = 180;
const ROW_HEIGHT = 5.5;
const HEADER_HEIGHT = 7;
const PAGE_BOTTOM_MARGIN = 15;

function buildColumns(t) {
  return [
    [t("Date"), 22, "left"],
    [t("Weekday"), 20, "left"],
    [t("Start"), 12, "center"],
    [t("End"), 12, "center"],
    [t("Category"), 40, "left"],
    [t("Duration"), 16, "right"],
    [t("Absence"), 25, "left"],
    [t("Holiday"), 33, "left"],
  ];
}

function colX(cols, columnIndex) {
  let currentX = MARGIN_LEFT;
  for (let i = 0; i < columnIndex; i++) currentX += cols[i][1];
  return currentX;
}

function textX(cols, columnIndex) {
  const [, width, align] = cols[columnIndex];
  if (align === "right") return colX(cols, columnIndex) + width - 1;
  if (align === "center") return colX(cols, columnIndex) + width / 2;
  return colX(cols, columnIndex) + 1;
}

function approvedEntryMinutes(entry) {
  return entry.status === "approved" && entry.counts_as_work !== false
    ? entry.minutes || 0
    : 0;
}

function rangeTotalMinutes(days) {
  return (days || []).reduce(
    (sum, day) =>
      sum +
      (day.entries || []).reduce(
        (entrySum, entry) => entrySum + approvedEntryMinutes(entry),
        0,
      ),
    0,
  );
}

function flextimeBounds(flextimeData) {
  if (!flextimeData || flextimeData.length === 0) {
    return { openingBalance: null, closingBalance: null };
  }
  const opening = flextimeData[0].cumulative_min - flextimeData[0].diff_min;
  const closing = flextimeData[flextimeData.length - 1].cumulative_min;
  return { openingBalance: opening, closingBalance: closing };
}

/**
 * Build the timesheet PDF and return it as a Blob.
 *
 * @param {object} args
 * @param {{days: Array}} args.report
 * @param {Array} args.flextimeData
 * @param {string} args.userName
 * @param {string} args.from
 * @param {string} args.to
 * @param {(key: string, params?: object) => string} args.t
 * @returns {Blob}
 */
export function buildReportPdf({ report, flextimeData, userName, from, to, t }) {
  const cols = buildColumns(t);
  const doc = new jsPDF({ unit: "mm", format: "a4" });
  let currentY = MARGIN_TOP;

  function drawHeader() {
    doc.setFillColor(235, 235, 235);
    doc.rect(MARGIN_LEFT, currentY, CONTENT_WIDTH, HEADER_HEIGHT, "F");
    doc.setFont("helvetica", "bold");
    doc.setFontSize(8);
    doc.setTextColor(50, 50, 50);
    cols.forEach(([label, , align], columnIndex) =>
      doc.text(label, textX(cols, columnIndex), currentY + 4.8, { align }),
    );
    currentY += HEADER_HEIGHT;
  }

  function drawRow(cells, shade) {
    if (currentY + ROW_HEIGHT > PAGE_HEIGHT - PAGE_BOTTOM_MARGIN) {
      doc.addPage();
      currentY = MARGIN_TOP;
      drawHeader();
    }
    if (shade) {
      doc.setFillColor(248, 248, 248);
      doc.rect(MARGIN_LEFT, currentY, CONTENT_WIDTH, ROW_HEIGHT, "F");
    }
    doc.setFont("helvetica", "normal");
    doc.setFontSize(7.5);
    doc.setTextColor(30, 30, 30);
    cells.forEach(([text, columnIndex]) => {
      const [, , align] = cols[columnIndex];
      doc.text(String(text ?? ""), textX(cols, columnIndex), currentY + 3.8, {
        align,
      });
    });
    doc.setDrawColor(220, 220, 220);
    doc.line(
      MARGIN_LEFT,
      currentY + ROW_HEIGHT,
      MARGIN_LEFT + CONTENT_WIDTH,
      currentY + ROW_HEIGHT,
    );
    currentY += ROW_HEIGHT;
  }

  function drawSummaryRow(label, value) {
    if (currentY + ROW_HEIGHT > PAGE_HEIGHT - PAGE_BOTTOM_MARGIN) {
      doc.addPage();
      currentY = MARGIN_TOP;
    }
    doc.setFont("helvetica", "normal");
    doc.setFontSize(7.5);
    doc.setTextColor(90, 90, 90);
    doc.text(label, MARGIN_LEFT + 1, currentY + 3.8);
    doc.text(value, textX(cols, 5), currentY + 3.8, { align: "right" });
    currentY += ROW_HEIGHT;
  }

  // Title block.
  doc.setFont("helvetica", "bold");
  doc.setFontSize(13);
  doc.setTextColor(20, 20, 20);
  doc.text(t("Timesheet"), MARGIN_LEFT, currentY + 6);
  doc.setFont("helvetica", "normal");
  doc.setFontSize(9);
  doc.setTextColor(90, 90, 90);
  doc.text(`${userName} – ${from} – ${to}`, MARGIN_LEFT, currentY + 12);
  currentY += 20;
  drawHeader();

  let rowIdx = 0;
  for (const day of report.days || []) {
    const absence = day.absence ? absenceKindLabel(day.absence) : "";
    const holiday = day.holiday || "";
    const weekday = t(day.weekday);
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
            [t(entry.category ?? ""), 4],
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
  if (currentY + ROW_HEIGHT > PAGE_HEIGHT - PAGE_BOTTOM_MARGIN) {
    doc.addPage();
    currentY = MARGIN_TOP;
    drawHeader();
  }
  doc.setFillColor(235, 235, 235);
  doc.rect(MARGIN_LEFT, currentY, CONTENT_WIDTH, ROW_HEIGHT, "F");
  doc.setFont("helvetica", "bold");
  doc.setFontSize(7.5);
  doc.setTextColor(20, 20, 20);
  doc.text(t("Total"), MARGIN_LEFT + 1, currentY + 3.8);
  const totalMin = rangeTotalMinutes(report.days);
  doc.text(minToHM(totalMin), textX(cols, 5), currentY + 3.8, {
    align: "right",
  });
  currentY += ROW_HEIGHT;

  const { openingBalance, closingBalance } = flextimeBounds(flextimeData);
  if (openingBalance !== null) {
    drawSummaryRow(
      t("Flextime opening balance"),
      (openingBalance >= 0 ? "+" : "") + minToHM(openingBalance),
    );
  }
  if (closingBalance !== null) {
    drawSummaryRow(
      t("Flextime closing balance"),
      (closingBalance >= 0 ? "+" : "") + minToHM(closingBalance),
    );
  }

  return doc.output("blob");
}
