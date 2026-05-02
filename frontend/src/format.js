// Pure presentation helpers — no business rules.
import { getLocale } from "./i18n.js";

const ISO_DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

export function parseDate(value) {
  if (value instanceof Date) {
    return new Date(value);
  }

  if (typeof value === "string" && ISO_DATE_RE.test(value)) {
    const [year, month, day] = value.split("-").map(Number);
    return new Date(year, month - 1, day);
  }

  return new Date(value);
}

export function fmtDate(d) {
  return parseDate(d).toLocaleDateString(getLocale(), {
    weekday: "short",
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  });
}
export function fmtDateShort(d) {
  return parseDate(d).toLocaleDateString(getLocale(), {
    day: "2-digit",
    month: "2-digit",
  });
}
export function fmtMonthYear(d) {
  return parseDate(d).toLocaleDateString(getLocale(), {
    month: "long",
    year: "numeric",
  });
}
export function fmtDateTime(d) {
  return parseDate(d).toLocaleString(getLocale());
}
export function weekdayLabels() {
  const base = new Date(Date.UTC(2024, 0, 1));
  return Array.from({ length: 7 }, (_, i) =>
    new Date(base.getTime() + i * 86400000).toLocaleDateString(getLocale(), {
      weekday: "short",
      timeZone: "UTC",
    }),
  );
}
export function isoDate(d) {
  const x = parseDate(d);
  return (
    x.getFullYear() +
    "-" +
    String(x.getMonth() + 1).padStart(2, "0") +
    "-" +
    String(x.getDate()).padStart(2, "0")
  );
}
export function monday(d) {
  const x = parseDate(d);
  const wd = (x.getDay() + 6) % 7;
  x.setDate(x.getDate() - wd);
  x.setHours(0, 0, 0, 0);
  return x;
}
export function addDays(d, n) {
  const x = parseDate(d);
  x.setDate(x.getDate() + n);
  return x;
}
export function minToHM(min) {
  const sign = min < 0 ? "-" : "";
  const a = Math.abs(min);
  return sign + Math.floor(a / 60) + ":" + String(a % 60).padStart(2, "0");
}
export function durMin(start, end) {
  const [bh, bm] = start.split(":").map(Number);
  const [eh, em] = end.split(":").map(Number);
  return eh * 60 + em - (bh * 60 + bm);
}
export function isoWeek(d) {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const dn = (t.getUTCDay() + 6) % 7;
  t.setUTCDate(t.getUTCDate() - dn + 3);
  const j1 = new Date(Date.UTC(t.getUTCFullYear(), 0, 4));
  return (
    1 + Math.round(((t - j1) / 86400000 - 3 + ((j1.getUTCDay() + 6) % 7)) / 7)
  );
}
