// Pure presentation helpers — no business rules.
import { getLocale } from "./i18n.js";
import { get } from "svelte/store";
import { settings } from "./stores.js";

const ISO_DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const UTC_ISO_DATETIME_RE = /^\d{4}-\d{2}-\d{2}T/;

function getConfiguredTimeZone(timezoneOverride) {
  const timezone =
    typeof timezoneOverride === "string" && timezoneOverride.trim()
      ? timezoneOverride
      : get(settings)?.timezone;
  if (typeof timezone !== "string" || !timezone.trim()) {
    return "Europe/Berlin";
  }
  return timezone.trim();
}

function datePartsInConfiguredTimeZone(value = new Date(), timezoneOverride) {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: getConfiguredTimeZone(timezoneOverride),
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(value);
  const year = Number(parts.find((part) => part.type === "year")?.value);
  const month = Number(parts.find((part) => part.type === "month")?.value);
  const day = Number(parts.find((part) => part.type === "day")?.value);
  return { year, month, day };
}

export function appTodayDate(timezoneOverride) {
  const { year, month, day } = datePartsInConfiguredTimeZone(
    new Date(),
    timezoneOverride,
  );
  // Create a Date at noon in browser TZ to avoid DST midnight shifts,
  // but with same Y-M-D as configured TZ – comparison uses iso string anyway.
  const d = new Date(year, month - 1, day, 12, 0, 0, 0);
  return d;
}

export function appTodayIsoDate(timezoneOverride) {
  return isoDate(appTodayDate(timezoneOverride));
}

export function appCurrentTimeHM(timezoneOverride) {
  const parts = new Intl.DateTimeFormat("en-GB", {
    timeZone: getConfiguredTimeZone(timezoneOverride),
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(new Date());
  const hour = parts.find((part) => part.type === "hour")?.value || "00";
  const minute = parts.find((part) => part.type === "minute")?.value || "00";
  return `${hour}:${minute}`;
}

export function parseDate(value) {
  if (value instanceof Date) {
    return new Date(value);
  }

  if (typeof value === "string" && ISO_DATE_RE.test(value)) {
    const [year, month, day] = value.split("-").map(Number);
    // Noon avoids DST midnight gaps
    return new Date(year, month - 1, day, 12, 0, 0, 0);
  }

  // For UTC ISO datetime, parse then convert to configured-TZ date parts
  // to avoid browser-TZ offset shifting the day.
  if (typeof value === "string" && UTC_ISO_DATETIME_RE.test(value)) {
    const utcDate = new Date(value);
    if (!Number.isNaN(utcDate.getTime())) {
      const { year, month, day } = datePartsInConfiguredTimeZone(
        utcDate,
        undefined,
      );
      return new Date(year, month - 1, day, 12, 0, 0, 0);
    }
  }

  return new Date(value);
}

export function fmtDate(d) {
  const options = {
    weekday: "short",
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  };
  // Always apply configured TZ – fmtDate may receive Date objects derived from UTC strings.
  options.timeZone = getConfiguredTimeZone();
  return parseDate(d).toLocaleDateString(getLocale(), options);
}
export function fmtDateShort(d) {
  const options = {
    day: "2-digit",
    month: "2-digit",
    timeZone: getConfiguredTimeZone(),
  };
  return parseDate(d).toLocaleDateString(getLocale(), options);
}
export function fmtDateWeekday(d) {
  const options = {
    weekday: "short",
    day: "2-digit",
    month: "2-digit",
    timeZone: getConfiguredTimeZone(),
  };
  return parseDate(d).toLocaleDateString(getLocale(), options);
}
export function fmtMonthYear(d) {
  return parseDate(d).toLocaleDateString(getLocale(), {
    month: "long",
    year: "numeric",
  });
}

// Converts a "YYYY-MM" string (as returned by the overtime API) to a
// localized "Month Year" label, e.g. "Mai 2026" or "May 2026".
// Appending "-01" makes it a valid ISO date for parseDate().
export function fmtMonthLabel(yearMonth) {
  return fmtMonthYear(yearMonth + "-01");
}
export function fmtDateTime(d) {
  // Unlike parseDate(), this must preserve the real time-of-day: parseDate's
  // UTC-ISO-datetime branch anchors to local noon to keep pure calendar-date
  // fields (e.g. an absence's start_date) from shifting a day near DST, but
  // occurred_at/created_at timestamps carry a meaningful hour/minute/second
  // that anchoring would silently discard.
  const value = new Date(d);
  if (Number.isNaN(value.getTime())) return "";
  return value.toLocaleString(getLocale(), {
    timeZone: getConfiguredTimeZone(),
    hour12: getTimeFormat() === "12h",
  });
}
export function weekdayLabels() {
  const base = new Date(Date.UTC(2024, 0, 1));
  return Array.from({ length: 7 }, (_, dayIndex) =>
    new Date(base.getTime() + dayIndex * 86400000).toLocaleDateString(
      getLocale(),
      {
        weekday: "short",
        timeZone: "UTC",
      },
    ),
  );
}
export function isoDate(d) {
  const parsedDate = parseDate(d);
  if (Number.isNaN(parsedDate.getTime())) {
    return "";
  }
  return (
    parsedDate.getFullYear() +
    "-" +
    String(parsedDate.getMonth() + 1).padStart(2, "0") +
    "-" +
    String(parsedDate.getDate()).padStart(2, "0")
  );
}
export function dateKey(value) {
  if (typeof value === "string") {
    const raw = value.trim();
    const isoPrefix = raw.match(/^\d{4}-\d{2}-\d{2}/);
    if (isoPrefix) {
      return isoPrefix[0];
    }

    const germanDateMatch = raw.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})$/);
    if (germanDateMatch) {
      return `${germanDateMatch[3]}-${String(Number(germanDateMatch[2])).padStart(2, "0")}-${String(
        Number(germanDateMatch[1]),
      ).padStart(2, "0")}`;
    }

    const ymdSlash = raw.match(/^(\d{4})\/(\d{1,2})\/(\d{1,2})$/);
    if (ymdSlash) {
      return `${ymdSlash[1]}-${String(Number(ymdSlash[2])).padStart(2, "0")}-${String(
        Number(ymdSlash[3]),
      ).padStart(2, "0")}`;
    }
  }

  if (typeof value === "number" && Number.isFinite(value)) {
    return isoDate(new Date(value));
  }

  if (value && typeof value === "object") {
    const year = Number(value.year);
    const month = Number(value.month);
    const day = Number(value.day);
    if (
      Number.isInteger(year) &&
      Number.isInteger(month) &&
      Number.isInteger(day) &&
      month >= 1 &&
      month <= 12 &&
      day >= 1 &&
      day <= 31
    ) {
      return `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    }
  }

  return isoDate(value);
}
export function monday(d) {
  const parsedDate = parseDate(d);
  const weekdayIndex = (parsedDate.getDay() + 6) % 7;
  parsedDate.setDate(parsedDate.getDate() - weekdayIndex);
  parsedDate.setHours(0, 0, 0, 0);
  return parsedDate;
}
export function addDays(d, n) {
  const parsedDate = parseDate(d);
  parsedDate.setDate(parsedDate.getDate() + n);
  return parsedDate;
}
export function minToHM(min) {
  if (!Number.isFinite(min)) return "0:00";
  const sign = min < 0 ? "-" : "";
  const absoluteMinutes = Math.abs(Math.trunc(min));
  return (
    sign +
    Math.floor(absoluteMinutes / 60) +
    ":" +
    String(absoluteMinutes % 60).padStart(2, "0")
  );
}
export function durMin(start, end) {
  const parse = (s) => {
    if (!s) return NaN;
    const parts = String(s).trim().split(":").map(Number);
    if (parts.length < 2 || parts.some((n) => !Number.isFinite(n))) return NaN;
    return parts[0] * 60 + (parts[1] || 0);
  };
  const bh = parse(start);
  const eh = parse(end);
  if (!Number.isFinite(bh) || !Number.isFinite(eh)) return NaN;
  return eh - bh;
}
export function isoWeek(d) {
  const utcDate = new Date(
    Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()),
  );
  const dayNumber = (utcDate.getUTCDay() + 6) % 7;
  utcDate.setUTCDate(utcDate.getUTCDate() - dayNumber + 3);
  const firstThursday = new Date(Date.UTC(utcDate.getUTCFullYear(), 0, 4));
  return (
    1 +
    Math.round(
      ((utcDate - firstThursday) / 86400000 -
        3 +
        ((firstThursday.getUTCDay() + 6) % 7)) /
        7,
    )
  );
}

/// Returns a canonical week label that includes the ISO calendar week number
/// and the full Monday–Sunday date range, e.g. "KW 21 (18.05. – 24.05.2026)".
/// Locale-aware: uses "KW" for German, "Week" for all other locales.
export function fmtWeekLabel(weekStart) {
  const d = parseDate(weekStart);
  const kw = isoWeek(d);
  const end = addDays(d, 6);
  const locale = getLocale();
  const prefix = locale.startsWith("de") ? "KW" : "Week";
  const startStr = d.toLocaleDateString(locale, {
    day: "2-digit",
    month: "2-digit",
  });
  const endStr = end.toLocaleDateString(locale, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  });
  return `${prefix} ${kw} (${startStr}–${endStr})`;
}

export function getTimeFormat() {
  return get(settings).time_format === "12h" ? "12h" : "24h";
}

export function formatTimeValue(value, timeFormat = getTimeFormat()) {
  if (typeof value !== "string") {
    return value;
  }

  const match = value.match(/^([01]\d|2[0-3]):([0-5]\d)(?::[0-5]\d)?$/);
  if (!match) return value;

  const [, hoursRaw, minutes] = match;
  const hours = Number(hoursRaw);
  if (timeFormat !== "12h") {
    return `${String(hours).padStart(2, "0")}:${minutes}`;
  }

  const suffix = hours >= 12 ? "PM" : "AM";
  const hour12 = hours % 12 || 12;
  return `${String(hour12).padStart(2, "0")}:${minutes} ${suffix}`;
}
